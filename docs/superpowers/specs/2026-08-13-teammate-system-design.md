# 队友系统设计

**日期**：2026-08-13
**作者**：协同设计（用户 + Claude）
**状态**：已批准（待用户最终审阅）

## 背景与目标

Royale Arena 当前已存在 `Player.team_id: Option<u32>` 字段、`TeammateBehavior { mode: i32 }` 结构体以及导演页"组队编号"录入 UI，但 `TeammateBehavior` 仅作为占位符（带 `// TODO` 注释），后端未实现任何队友行为规则。

本次目标：

1. **导演侧**：在游戏中管理页新增"队友模式"配置卡片，包含总开关和三个子项（禁伤害、禁搜索、允许转移）。
2. **玩家侧**：队友模式开启且允许转移时，玩家背包内每件道具出现"转移"按钮，可免费转移给任意同队队友，接收方体力 -5。
3. **战斗交互**：同队玩家之间免疫普攻、橙武 AOE 溅射、遥控地雷伤害；同队玩家互相无法被搜索到。
4. **散人无感**：`team_id = 0` 的散人玩家在队友模式开关前后行为完全一致。

商店购买广播不在本次范围内——经核查现有代码（`player_common_actions.rs:839-853`）已只通知买家 + 导演，无需改动。

## 架构总览

```
┌─────────────────────────────────────────────────────────────┐
│  前端                                                        │
│                                                              │
│  导演页 [TeammateModeCard.vue]    玩家页 [InventoryPanel.vue]│
│   ├─ 总开关 (mode != 0)            ├─ 每件背包物品新增        │
│   ├─ ☐ 禁队友伤害 (位 1)            │   "转移" 按钮            │
│   ├─ ☐ 禁搜索到队友 (位 2)          ├─ 点击 → 转移对话框       │
│   └─ ☐ 允许转移物品 (位 8)          │   (同队玩家列表)         │
│       ↓ set_teammate_behavior      └─ 选定 → sendPlayerAction│
└─────────────────────────────────────────────────────────────┘
                                ↓ WebSocket
┌─────────────────────────────────────────────────────────────┐
│  后端                                                        │
│                                                              │
│  DirectorActionScheduler.dispatch                            │
│   └─ "set_teammate_behavior" → 同步更新                       │
│        ├─ game_state.rules_config["teammate_behavior"]        │
│        └─ game_state.rule_engine.teammate_behavior.mode       │
│                                                              │
│  TeammateBehavior helpers                                    │
│   ├─ is_damage_immune()  // 位 1                            │
│   ├─ is_search_filtered()// 位 2                            │
│   └─ is_transfer_enabled()// 位 8                           │
│                                                              │
│  GameState.are_teammates(a_id, b_id) -> bool                │
│   └─ team_id 相等且 > 0                                      │
│                                                              │
│  拦截点 (3 处)                                                │
│   ├─ player_attack_action.rs                                 │
│   │   ├─ 主目标伤害 = 0 (返回 info 提示)                     │
│   │   └─ aoe_targets 过滤掉同队                              │
│   ├─ player_use_action.rs (遥控地雷)                         │
│   │   └─ occupant_ids 过滤掉同队                              │
│   └─ player_common_actions.rs (handle_search_action)         │
│       └─ collect_search_targets 过滤掉同队                   │
│                                                              │
│  新 player action: "transfer_item"                           │
│   └─ handle_transfer_item_action(sender, item_id, target)    │
│       校验: 位 8 开 / 同队 / 物品在背包 /                     │
│              接收方存活 / 接收方体力 ≥ 5 /                    │
│              接收方背包未满                                   │
│       应用: item 移交 / target.strength -= 5                 │
└─────────────────────────────────────────────────────────────┘
```

**核心约束**：

- 整套机制由 `mode != 0` 总开关管控；mode = 0 时所有子规则失效，行为和现在完全一致。
- `team_id = 0` 的玩家永远不算同队（包括自己对自己）。
- `rules_config` 已在每次 `state_update` 广播给所有客户端，前端开关状态自动同步。

## 详细设计

### §1 后端：配置存储、辅助方法、导演动作

**配置存储**（沿用现有字段，不新增）：

- `GameState.rules_config: JsonValue`（已含 `teammate_behavior: i32`，默认 0）
- `GameState.rule_engine.teammate_behavior.mode: i32`（已有，去掉 `#[allow(dead_code)]` 标注）

**`TeammateBehavior` helper**（`backend/src/game/game_rule_engine.rs`）：

```rust
impl TeammateBehavior {
    pub const BIT_DAMAGE_IMMUNE: i32 = 1;
    pub const BIT_SEARCH_FILTER: i32 = 2;
    pub const BIT_TRANSFER: i32      = 8;

    pub fn is_damage_immune(&self)   -> bool { self.mode & Self::BIT_DAMAGE_IMMUNE  != 0 }
    pub fn is_search_filtered(&self) -> bool { self.mode & Self::BIT_SEARCH_FILTER  != 0 }
    pub fn is_transfer_enabled(&self)-> bool { self.mode & Self::BIT_TRANSFER       != 0 }
}
```

位 4（观看队友状态）**本次不实现**，常量与方法均不定义。

**`GameState.are_teammates`**（`backend/src/websocket/models.rs`）：

```rust
impl GameState {
    pub fn are_teammates(&self, a_id: &str, b_id: &str) -> bool {
        if a_id == b_id { return false; }
        let (a, b) = match (self.players.get(a_id), self.players.get(b_id)) {
            (Some(a), Some(b)) => (a, b),
            _ => return false,
        };
        match (a.team_id, b.team_id) {
            (Some(ta), Some(tb)) => ta > 0 && ta == tb,
            _ => false,
        }
    }
}
```

**新导演动作 `set_teammate_behavior`**：

- `DirectorActionParams` 加字段：`pub teammate_behavior: Option<i32>`
- 调度器（`director_action_scheduler.rs`）新增分支：

```rust
"set_teammate_behavior" => {
    let mode = action_params.teammate_behavior
        .ok_or_else(|| "Missing teammate_behavior parameter".to_string())?;
    if !(0..=15).contains(&mode) {
        return Err("teammate_behavior must be in 0..=15".to_string());
    }
    game_state.handle_set_teammate_behavior(mode)
}
```

- handler（`director_common_actions.rs`）：

```rust
pub fn handle_set_teammate_behavior(&mut self, mode: i32) -> Result<ActionResults, String> {
    // 1. 写回 rule_engine（让本轮后续 action 立刻生效）
    self.rule_engine.teammate_behavior.mode = mode;
    // 2. 同步到 rules_config JSON（让前端 state_update 看到新值、让存档保留）
    if let Some(obj) = self.rules_config.as_object_mut() {
        obj.insert("teammate_behavior".into(), serde_json::json!(mode));
    }
    // 3. 全员广播 SystemNotice
    Ok(ActionResult::new_system_message(
        serde_json::json!({ "teammate_behavior": mode }),
        self.players.keys().cloned().collect(),
        format!("导演已更新队友行为规则（位掩码={}）", mode),
        true,
    ).as_results())
}
```

**注意**：`rules_config` 是 `JsonValue`，写入时用 `as_object_mut().insert()`，不替换整个对象，避免破坏其他字段。

### §2 后端：4 个拦截点

#### (1) 普通攻击主目标 — `backend/src/websocket/actions/player_attack_action.rs`

在"验证目标玩家是否已死亡"之后、计算伤害之前插入：

```rust
if self.rule_engine.teammate_behavior.is_damage_immune()
    && self.are_teammates(player_id, &target_player_id)
{
    return Ok(ActionResult::new_info_message(
        serde_json::json!({}),
        vec![player_id.to_string()],
        "队友伤害免疫已开启，无法攻击队友".to_string(),
        false,
    ).as_results());
}
```

**行为**：和现有失败分支（不同地点 / 目标已死 / 安全区）一致——返回 Info 提示，**不消耗体力、不消耗武器耐久、不清空 `last_search_result`**。玩家可以撤销这次操作去做别的。

#### (2) 橙武 AOE 溅射 — `player_attack_action.rs`

溅射目标收集处（`aoe_targets` 的 `filter_map`）加一个过滤：

```rust
.filter_map(|other_id| {
    if other_id.as_str() == player_id || other_id == &target_player_id { return None; }
    if self.rule_engine.teammate_behavior.is_damage_immune()
        && self.are_teammates(player_id, other_id) { return None; }
    // ... 原有存活判断
})
```

**行为**：主目标如果是非队友，正常挨打；同队的其他玩家被溅射列表剔除，不会收到任何伤害消息。攻击者那条"...；溅射命中 X，Y"日志里也不会出现队友名字。

#### (3) 遥控地雷 — `backend/src/websocket/actions/player_use_action.rs`

`for target_id in occupant_ids` 之前先把同队玩家过滤掉：

```rust
let occupant_ids: Vec<String> = self.places.get(player_location)
    .map(|p| p.players.clone()).unwrap_or_default()
    .into_iter()
    .filter(|id| {
        id != player_id
        && !(self.rule_engine.teammate_behavior.is_damage_immune()
             && self.are_teammates(player_id, id))
    })
    .collect();
```

**行为**：地雷引爆照常发生（消耗物品、消耗体力），但同队玩家不受伤害。日志只列实际受伤的非队友。

#### (4) 搜索行动 — `player_common_actions.rs`

`collect_search_targets` 内部，加入玩家候选前判断：

```rust
if self.rule_engine.teammate_behavior.is_search_filtered()
    && self.are_teammates(searcher_id, candidate_id) { continue; }
```

**行为**：搜索仍然消耗体力和冷却时间，但不会随机到队友。如果地点只有队友和物品，可能仍然抽到物品；如果整个地点全是队友，返回空搜索结果。

#### 共通设计原则

- 主目标免疫 → 整个 action 不发生（不扣体力、不扣耐久）
- 溅射 / 地雷 / 搜索 → action 正常发生，只把队友从影响池里剔除
- 三处免疫判断都先检查 `is_damage_immune()` 再检查 `are_teammates()`，避免位 1 没开时白算一次

### §3 后端：新玩家动作 `transfer_item`

**调度器注册**（`player_action_scheduler.rs`）：

`ActionParams` 复用已有的 `item_id` 和 `target_player_id` 字段。新分支（不做 Strength 前置校验，发起方免费）：

```rust
"transfer_item" => {
    let item_id = action_params.item_id
        .ok_or("Missing item_id parameter".to_string())?;
    let target_player_id = action_params.target_player_id
        .ok_or("Missing target_player_id parameter".to_string())?;
    return game_state.handle_transfer_item_action(player_id, &item_id, &target_player_id);
}
```

**handler**（`player_common_actions.rs`）：

```rust
pub fn handle_transfer_item_action(
    &mut self,
    sender_id: &str,
    item_id: &str,
    target_player_id: &str,
) -> Result<ActionResults, String> {
    // 1. 位 8 必须开
    if !self.rule_engine.teammate_behavior.is_transfer_enabled() {
        return Ok(info_msg(sender_id, "队友物品转移未开启"));
    }
    // 2. 必须是同队
    if !self.are_teammates(sender_id, target_player_id) {
        return Ok(info_msg(sender_id, "目标不是你的队友"));
    }
    // 3. 接收方必须存活
    if !self.players.get(target_player_id).map(|p| p.is_alive).unwrap_or(false) {
        return Ok(info_msg(sender_id, "对方已阵亡，无法接收"));
    }
    // 4. 物品必须在背包（不含已装备）
    let item = match self.players.get(sender_id)
        .and_then(|p| p.inventory.iter().find(|i| i.id == item_id))
    {
        Some(it) => it.clone(),
        None => return Ok(info_msg(sender_id, "物品不在背包中")),
    };
    // 5. 接收方体力 ≥ 5
    let target_strength = self.players.get(target_player_id).map(|p| p.strength).unwrap_or(0);
    if target_strength < 5 {
        return Ok(info_msg(sender_id, "对方体力不足，无法接收"));
    }
    // 6. 接收方背包未满
    let target = self.players.get(target_player_id).unwrap();
    let max = self.rule_engine.player_config.max_backpack_items as usize;
    if target.get_total_item_count() >= max {
        return Ok(info_msg(sender_id, "对方背包已满，无法接收"));
    }

    // 应用：sender 移除、target 加入并扣体力
    let sender_name = self.players.get(sender_id).unwrap().name.clone();
    let target_name = self.players.get(target_player_id).unwrap().name.clone();
    let item_name = item.name.clone();

    self.players.get_mut(sender_id).unwrap().inventory
        .retain(|i| i.id != item_id);
    let target = self.players.get_mut(target_player_id).unwrap();
    target.inventory.push(item);
    target.strength -= 5;

    let sender_msg = format!("你将 {} 转移给了 {}", item_name, target_name);
    let target_msg = format!("队友 {} 将 {} 转移给你，体力 -5", sender_name, item_name);

    let sender_data = json!({ "item_name": item_name, "target": target_name });
    let target_data = json!({
        "item_name": item_name, "sender": sender_name,
        "strength": target.strength, "life": target.life,
    });

    Ok(ActionResults { results: vec![
        ActionResult::new_system_message(sender_data, vec![sender_id.to_string()], sender_msg, true),
        ActionResult::new_system_message(target_data, vec![target_player_id.to_string()], target_msg, true),
    ]})
}
```

**关键决策**：

| 决策点 | 选择 | 理由 |
|---|---|---|
| 发起方是否消耗体力 | 否 | 用户明确"被转移道具的队友体力-5"，发起方无成本 |
| 接收方扣 5 体力后能否致死 | 否 | 体力本身就是 0 也不会死，无生命影响 |
| 物品类型限制 | 无 | 货币/消耗品/武器/防具/升级器/陷阱全能转 |
| 已装备物品 | 不能转 | 用户已确认"只能转背包内" |
| 转移是否清空 `last_search_result` | 否 | 不影响后续搜索 |
| 失败时是否消耗体力 | 否 | 全部前置校验在写状态前 |

**注意**：借用冲突由 Rust 编译器保证——先 `get_mut(sender_id)` 改完，再 `get_mut(target_player_id)` 改，顺序错开会编译失败。

### §4 前端：导演配置卡片 + 玩家转移按钮

#### 4.1 后端先补一个口子（暴露 team_id 给玩家视角）

修改 `backend/src/websocket/broadcaster.rs` 的 `to_player_client_json_for_other_players()`：

```rust
pub fn to_player_client_json_for_other_players(&self) -> JsonValue {
    json!({
        "id": self.id,
        "name": self.name,
        "team_id": self.team_id,
    })
}
```

**理由**：team_id=0 视为散人，本来就从导演页可见；非 0 时导演已明确组队，不算隐私。这样前端逻辑可以统一处理，不用按"队友模式开/关"切换 JSON 形状。

TS 类型 `ActorPlayer` 加 `team_id?: number`。

#### 4.2 导演页：新 `TeammateModeCard.vue`

放在 `frontend/src/views/director/components/TeammateModeCard.vue`，由 `InGameManagement.vue` 在 ShopManagement 上方引入。

UI：

```
┌─ 队友模式 ────────────────────────────┐
│  总开关: [☐ 开启]                      │
│                                        │
│  ☐ 禁止队友伤害      (位 1)            │
│  ☐ 禁止搜索到队友    (位 2)            │
│  ☐ 允许转移物品      (位 8)            │
└────────────────────────────────────────┘
```

```ts
const rulesConfig = computed(() => store.globalState?.rules_config ?? {})
const mode = computed(() => rulesConfig.value.teammate_behavior ?? 0)
const lastNonZero = ref(1)  // 缓存上次非 0 值

watch(mode, (v) => { if (v !== 0) lastNonZero.value = v }, { immediate: true })

const masterOn = computed({
  get: () => mode.value !== 0,
  set: (v) => store.setTeammateBehavior(v ? lastNonZero.value : 0)
})
const bit = (b: number) => computed({
  get: () => (mode.value & b) !== 0,
  set: (v) => store.setTeammateBehavior(v ? mode.value | b : mode.value & ~b)
})
const bit1 = bit(1), bit2 = bit(2), bit8 = bit(8)
```

#### 4.3 玩家页：背包"转移"按钮

`InventoryPanel.vue` 在每件物品的"丢弃"按钮**左边**加：

```vue
<el-button
  v-if="transferAvailable"
  type="warning" size="small"
  @click="openTransferDialog(item.id)"
  :loading="loadingItems.includes(item.id)"
>
  转移
</el-button>
```

```ts
const teammateMode  = computed(() => store.globalState?.rules_config?.teammate_behavior ?? 0)
const masterOn      = computed(() => teammateMode.value !== 0)
const transferBitOn = computed(() => (teammateMode.value & 8) !== 0)
const transferAvailable = computed(() => masterOn.value && transferBitOn.value)
```

**按钮显隐条件**：只看两个全局开关（总开关 + 位 8），不掺入"我自己有没有队友"。点击后如果没队友，对话框显示空状态。

`myTeamId` 和 `teammates` 在对话框组件里实时计算：

```ts
const myTeamId = computed(() => store.actorPlayer?.team_id ?? 0)
const teammates = computed(() =>
  store.actorPlayerList.filter(p =>
    p.team_id && p.team_id > 0 && p.team_id === myTeamId.value)
)
```

#### 4.4 转移对话框 `TransferTeammateDialog.vue`

放在 `frontend/src/views/actor/components/TransferTeammateDialog.vue`，可基于现有 `PlayerSelectionDialog.vue` 改造：

```
┌─ 转移 [橙]自然之力.晓 给队友 ─────────────┐
│  选择队友:                                 │
│  ◯ 玩家A                                  │
│  ◯ 玩家B                                  │
│  (对方将扣除 5 点体力)                     │
└────────────────────────────────────────────┘
```

空状态："暂无队友可转移"。

确认时：
```ts
gameStateStore.sendPlayerAction('transfer_item', {
  item_id: selectedItemId,
  target_player_id: selectedTeammateId
})
```

#### 4.5 玩家页日志呈现

后端发的 `SystemNotice`（"你将 X 转移给了 Y"、"队友 X 将 Y 转移给你，体力 -5"）会自动走现有 `state_update` → `addLogMessage` 流程，**无需额外前端代码**。

### §5 前端文件改动清单

| 文件 | 改动 |
|---|---|
| `frontend/src/types/gameStateTypes.ts` | `ActorPlayer` 加 `team_id?: number` |
| `frontend/src/stores/gameState.ts` | 新增 `setTeammateBehavior(mode)` 方法，封装 `sendDirectorAction('set_teammate_behavior', { teammate_behavior: mode })` |
| `frontend/src/views/director/components/TeammateModeCard.vue` | 新建 |
| `frontend/src/views/director/management/InGameManagement.vue` | 引入 `TeammateModeCard`，放 ShopManagement 上方 |
| `frontend/src/views/actor/components/InventoryPanel.vue` | 加转移按钮 + `transferAvailable` 计算属性 |
| `frontend/src/views/actor/components/TransferTeammateDialog.vue` | 新建（或复用 `PlayerSelectionDialog`） |

## 边界情况与设计保证

### 散人玩家在队友模式 ON 时完全无感

| 场景 | 后端行为 | 是否符合"散人无感" |
|---|---|---|
| 散人 A 攻击散人 B（mode=1） | `are_teammates(A,B)` = false，正常伤害 | ✅ |
| 散人 A 攻击队友组玩家 C（mode=1） | `are_teammates(A,C)` = false（A 的 team_id=0），A 正常伤害 C | ✅ |
| 队伍 X 内的玩家攻击同队队友（mode=1） | `are_teammates` = true，伤害=0，Info 拒绝 | ✅ |
| 散人 A 搜索（mode=2） | 候选池里所有非自己玩家都 `are_teammates=false`，全部保留 | ✅ |
| 散人 A 试图 transfer_item（mode=8） | `are_teammates` 永远 false，直接拒绝 | ✅ |
| 散人 A 的玩家列表（前端） | `myTeamId=0`，`teammates=[]`，不渲染队友标签、不显示转移按钮 | ✅ |

核心保证来自 `are_teammates()` 的实现：`team_id` 必须相等 **且 > 0**。

### 其他边界

| 场景 | 处理 |
|---|---|
| 同时多人给同一队友转道具 | 不需要额外锁：单进程内 `handle_action_results` 串行执行；后到的请求会看到更新后的目标状态 |
| 转移的物品是货币 | 货币在背包里也是独立 Item 实例，ID 唯一，正常转移 |
| 导演中途把 mode 从 11 改回 0 | 立即生效，下一次攻击/搜索/转移就走原逻辑；已经在路上的 action 不存在（dispatch 是同步的） |
| 玩家 team_id > 0 但队友模式 OFF | `is_*_enabled()` 全 false，行为和散人完全一致 |
| 搜索结果只剩队友（mode=2） | `collect_search_targets` 过滤后候选池可能为空 → 走现有 `handle_empty_search_result` 分支 |
| 橙武溅射清空（mode=1，所有非主目标都是队友） | `aoe_targets=[]`，`aoe_impacts=[]`，攻击者日志里不会出现"溅射命中"段 |
| 接收方背包差一个空位但发起方准备转两个 | 当前 handler 一次只转一件，不存在该问题；批量转移不在范围内 |
| 转移期间目标玩家死亡 | 加前置：`target.is_alive` 必须为 true，否则 Info 拒绝 |

## 测试计划

### 后端单元测试

```rust
// game_rule_engine.rs
mod tests {
    #[test] fn bit_helpers() {
        let b = TeammateBehavior { mode: 0 };
        assert!(!b.is_damage_immune() && !b.is_search_filtered() && !b.is_transfer_enabled());
        let b = TeammateBehavior { mode: 1 };
        assert!(b.is_damage_immune() && !b.is_search_filtered() && !b.is_transfer_enabled());
        let b = TeammateBehavior { mode: 2 };
        assert!(!b.is_damage_immune() && b.is_search_filtered() && !b.is_transfer_enabled());
        let b = TeammateBehavior { mode: 8 };
        assert!(!b.is_damage_immune() && !b.is_search_filtered() && b.is_transfer_enabled());
        let b = TeammateBehavior { mode: 11 };  // 位 1+2+8 全开
        assert!(b.is_damage_immune() && b.is_search_filtered() && b.is_transfer_enabled());
        let b = TeammateBehavior { mode: 9 };   // 位 1+8
        assert!(b.is_damage_immune() && !b.is_search_filtered() && b.is_transfer_enabled());
    }
}

// websocket/models.rs
mod tests {
    fn build_state(/* 配置 players 的辅助函数 */) -> GameState;

    #[test] fn are_teammates_same_team() {
        // team_a = {p1: 1, p2: 1} → assert are_teammates(p1, p2)
    }
    #[test] fn are_teammates_zero_team_is_solo() {
        // team_a = {p1: 0, p2: 0} → assert !are_teammates(p1, p2)
        // 自己对自己：assert !are_teammates(p1, p1)
    }
    #[test] fn are_teammates_different_teams() {
        // team_a = {p1: 1, p2: 2} → assert !are_teammates(p1, p2)
    }
    #[test] fn are_teammates_one_has_none_team_id() {
        // team_a = {p1: Some(1), p2: None} → assert !are_teammates(p1, p2)
    }
    #[test] fn are_teammates_unknown_player() {
        // team_a = {p1: 1} → assert !are_teammates(p1, "unknown_id")
    }
}
```

### 后端集成测试

| 测试名 | 断言 |
|---|---|
| `attack_teammate_with_damage_immune_returns_info` | mode=1，攻击队友返回 Info，体力/武器耐久未消耗 |
| `attack_splash_skips_teammates` | mode=1，主目标非队友受伤，队友不出现 aoe_impacts |
| `remote_mine_skips_teammates` | mode=1，地雷爆炸后非队友受伤、队友无伤 |
| `search_filters_teammates` | mode=2，搜索候选池不含同队玩家 |
| `transfer_item_happy_path` | mode=8，物品从 A 转到 B，B 体力 -5 |
| `transfer_item_rejected_when_bit8_off` | mode=0，返回 Info |
| `transfer_item_rejected_when_target_low_strength` | 接收方体力=3，返回 Info，物品不动 |
| `transfer_item_rejected_when_target_backpack_full` | 接收方背包满，返回 Info |
| `transfer_item_rejected_when_target_dead` | 接收方 is_alive=false，返回 Info |
| `set_teammate_behavior_broadcasts_to_all` | 导演改 mode=11，所有玩家收到 SystemNotice |
| `solo_player_unaffected_when_mode_on` | mode=11，散人之间所有行动正常 |

### 前端手动测试清单

部署 dev 环境后人工走查：

1. 导演页打开队友模式 = OFF → 玩家页背包**无转移按钮** ✅
2. 导演页打开总开关 + 勾选位 8 → 玩家页**出现转移按钮** ✅
3. 散人玩家（team_id=0）打开转移对话框 → "暂无队友可转移"
4. 同队玩家互相转移 → 双方收到 SystemNotice 日志
5. 接收方体力=3 时点转移 → 发起方收到"对方体力不足"Info 弹窗
6. 关掉总开关 → 转移按钮消失；正在打开的对话框保留但下一次操作被后端拒绝
7. 位 1 ON：同队两人尝试普攻/橙武溅射/地雷 → 都被屏蔽
8. 位 2 ON：搜索同地点的队友 → 永远搜不到

## 不在本次范围内（YAGNI）

明确**不做**的：

- 位 4（观看队友状态）的前端按钮和后端实现 — 完全不涉及
- 转移物品的批量操作
- 转移已装备的武器/防具
- 转移时的体力上限保护（扣到 0 就 0，不会负）
- 队友之间私聊频道
- 转移历史/审计日志（除现有 game_log）
- 商店购买广播改动（已验证现状正确）

## 商店购买广播验证（问题 1 结论）

经核查 `backend/src/websocket/actions/player_common_actions.rs:839-853` 与 `backend/src/websocket/service.rs:486-511`，当前实现：

- `broadcast_players: vec![player_id.to_string()]` —— 仅购买者本人
- `broadcast_to_director: true` —— 通知导演
- `broadcast_to_all: false`（默认）—— 不会广播给其他玩家

`service.rs` 的 `handle_action_results` 只对 `broadcast_players` 列表中的玩家发消息，外加按 `broadcast_to_director` 通知导演。`broadcast_to_all` 仅在玩家断线时使用。

**结论**：其他玩家本来就不会收到商店购买弹窗，本次不动代码。
