# 售出系统绿色配对约束（Sell Green Pair）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 绿色武器/防具只能恰好 2 件成对售出（每件各计价），其他稀有度保持单件售出不变。

**Architecture:** `sell_item` 玩家动作载荷从单 `item_id` 改为 `item_ids` 数组（1 件=非绿单卖、2 件=纯绿对），后端在现有校验链后追加配对约束并原子应用；前端售出对话框从 radio 单选改 checkbox 多选 + 实时校验提示。

**Tech Stack:** Rust (axum, serde), Vue 3 + TypeScript + Pinia + Element Plus

## Global Constraints

- 一笔售出：1 件非绿（现状）或恰好 2 件纯绿（武+武/甲+甲/武+甲均可）；其余一律拒绝
- 计价不变：每件按其稀有度价格，一对绿色共得 2× 绿价（f64，0.5 步长）
- 拒绝消息（Info 仅发买家，零状态变更）：长度非法 → "一次只能售出 1 件非绿色物品或 2 件绿色物品"；单绿 → "绿色物品需成对售出"；2 件含非绿（含绿+非绿、双非绿）→ "绿色物品不能与其他稀有度混合售出"
- 保留现有消息与校验：时间窗/背包/武器防具/稀有度有价；成功消息合并为一条（名字用"、"连接），卖家 + 导演两条结果结构不变
- `PlayerActionParams.item_id` 字段保留（其他动作在用），新增 `item_ids: Option<Vec<String>>`，`sell_item` 分支只读 `item_ids`
- 前端 store：`sellItem(itemIds: string[])` → `sendPlayerAction('sell_item', { item_ids: itemIds })`
- 构建：`export PATH="/c/msys64/mingw64/bin:$PATH"` 后再跑 cargo（dlltool.exe）
- 集成测试无需 DATABASE_URL
- 提交用 `git -c user.name="Wang Li" -c user.email="itx351@126.com" commit`

---

### Task 1: 后端 — 多件售出与绿色配对约束

**Files:**
- Modify: `backend/src/websocket/actions/player_action_scheduler.rs:19`（params 字段）与 `:345-361`（sell_item 分支）
- Modify: `backend/src/websocket/actions/player_common_actions.rs:1026-1140`（handle_sell_item_action 重写）
- Test: `backend/tests/sell_system_integration.rs`（改 2 个现有用例 + 新增 6 个）

**Interfaces:**
- Consumes: `GameState::rarity_display_name(rarity) -> Option<&'static str>`、`GameState::format_price(f64) -> String`、`Item::is_weapon_or_armor()`；测试辅助 `build_sell_game_state`/`sell_add_player`/`sell_put_weapon`/`sell_put_armor`/`sell_configure`/`sell_set_night_window`（本文件已有）
- Produces: `GameState::handle_sell_item_action(&mut self, player_id: &str, item_ids: &[String]) -> Result<ActionResults, String>`（签名变更，Task 2 前端通过 WS 载荷间接使用）；`PlayerActionParams.item_ids: Option<Vec<String>>`

- [ ] **Step 1: 改现有测试 + 写失败的新测试**

`backend/tests/sell_system_integration.rs`：

1. `sell_item_succeeds_in_daytime`（:228-250）三处改动——`sell_configure(&mut state, "common", 2.5)` → `sell_configure(&mut state, "rare", 2.5)`；`sell_put_weapon(&mut state, "p1", "w1", Some("common"))` → `Some("rare")`；调用改 `state.handle_sell_item_action("p1", &["w1".to_string()]).expect("sell ok")`；断言 `contains("绿")` → `contains("蓝")`。其余不变。

2. `sell_item_via_scheduler_dispatch`（:327-342）四处改动——`sell_configure(&mut state, "common", 0.5)` → `sell_configure(&mut state, "rare", 0.5)`；`sell_put_weapon(&mut state, "p1", "w1", Some("common"))` → `Some("rare")`；`ActionParams::from_json(&json!({ "item_id": "w1" }))` → `ActionParams::from_json(&json!({ "item_ids": ["w1"] }))`；coins 断言不变（0.5）。

3. 文件末尾追加（`#[test]` 每个）：

```rust
#[test]
fn sell_green_pair_succeeds() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.5);
    sell_put_weapon(&mut state, "p1", "w1", Some("common"));
    sell_put_armor(&mut state, "p1", "a1", Some("common"));
    sell_set_night_window(&mut state, 3600, 7200);

    let results = state
        .handle_sell_item_action("p1", &["w1".to_string(), "a1".to_string()])
        .expect("sell ok");
    let p = state.players.get("p1").unwrap();
    assert!(p.inventory.is_empty(), "两件都已移除");
    assert!((p.coins - 3.0).abs() < 1e-9, "1.5 × 2，实际 {}", p.coins);
    assert_eq!(results.results.len(), 2);
    let seller_msg = &results.results[0].log_message;
    assert!(seller_msg.contains("、"), "名字用顿号连接: {}", seller_msg);
    assert!(seller_msg.contains("[W]w1") && seller_msg.contains("[A]a1"));
    let director_msg = &results.results[1].log_message;
    assert!(director_msg.contains("绿"));
    assert!(director_msg.contains("3"));
}

#[test]
fn sell_single_green_rejected() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.5);
    sell_put_weapon(&mut state, "p1", "w1", Some("common"));
    sell_set_night_window(&mut state, 3600, 7200);

    let results = state.handle_sell_item_action("p1", &["w1".to_string()]).unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(results.results[0].log_message.contains("绿色物品需成对售出"));
    let p = state.players.get("p1").unwrap();
    assert_eq!(p.inventory.len(), 1, "零状态变更");
    assert!((p.coins - 0.0).abs() < 1e-9);
}

#[test]
fn sell_green_mixed_with_rare_rejected() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.5);
    sell_configure(&mut state, "rare", 3.0);
    sell_put_weapon(&mut state, "p1", "w1", Some("common"));
    sell_put_weapon(&mut state, "p1", "w2", Some("rare"));
    sell_set_night_window(&mut state, 3600, 7200);

    let results = state
        .handle_sell_item_action("p1", &["w1".to_string(), "w2".to_string()])
        .unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(results.results[0].log_message.contains("绿色物品不能与其他稀有度混合售出"));
    let p = state.players.get("p1").unwrap();
    assert_eq!(p.inventory.len(), 2, "零状态变更");
    assert!((p.coins - 0.0).abs() < 1e-9);
}

#[test]
fn sell_two_rare_rejected() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "rare", 3.0);
    sell_put_weapon(&mut state, "p1", "w1", Some("rare"));
    sell_put_weapon(&mut state, "p1", "w2", Some("rare"));
    sell_set_night_window(&mut state, 3600, 7200);

    let results = state
        .handle_sell_item_action("p1", &["w1".to_string(), "w2".to_string()])
        .unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(results.results[0].log_message.contains("绿色物品不能与其他稀有度混合售出"));
    assert_eq!(state.players.get("p1").unwrap().inventory.len(), 2);
}

#[test]
fn sell_invalid_length_rejected() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.5);
    for id in ["w1", "w2", "w3"] {
        sell_put_weapon(&mut state, "p1", id, Some("common"));
    }
    sell_set_night_window(&mut state, 3600, 7200);

    let three = state
        .handle_sell_item_action("p1", &["w1".to_string(), "w2".to_string(), "w3".to_string()])
        .unwrap();
    assert_eq!(three.results[0].message_type, MessageType::Info);
    assert!(three.results[0].log_message.contains("一次只能售出 1 件非绿色物品或 2 件绿色物品"));
    let empty = state.handle_sell_item_action("p1", &[]).unwrap();
    assert_eq!(empty.results[0].message_type, MessageType::Info);
    assert!(empty.results[0].log_message.contains("一次只能售出 1 件非绿色物品或 2 件绿色物品"));
    assert_eq!(state.players.get("p1").unwrap().inventory.len(), 3);
}

#[test]
fn sell_pair_with_missing_item_rejected() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.5);
    sell_put_weapon(&mut state, "p1", "w1", Some("common"));
    sell_set_night_window(&mut state, 3600, 7200);

    let results = state
        .handle_sell_item_action("p1", &["w1".to_string(), "nope".to_string()])
        .unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(results.results[0].log_message.contains("不在背包中"));
    assert_eq!(state.players.get("p1").unwrap().inventory.len(), 1, "零状态变更");
    assert!((state.players.get("p1").unwrap().coins - 0.0).abs() < 1e-9);
}
```

（`MessageType` 已在本文件导入；`sell_put_armor` 已有 :201。）

- [ ] **Step 2: 跑测试确认失败**

Run: `export PATH="/c/msys64/mingw64/bin:$PATH" && cd backend && cargo test --test sell_system_integration`
Expected: 编译失败——`handle_sell_item_action` 仍是 `&str` 签名、`ActionParams` 无 `item_ids` 字段。

- [ ] **Step 3: 调度器加字段并切换分支**

`player_action_scheduler.rs` :19 `pub item_id: Option<String>,` 后加：

```rust
    pub item_ids: Option<Vec<String>>,
```

`"sell_item"` 分支（:345-361）中 `let item_id = ...` 到 `return ...` 改为：

```rust
                let item_ids = action_params
                    .item_ids
                    .clone()
                    .ok_or("Missing item_ids parameter".to_string())?;
                game_state.end_rest_mode_for_action(player_id);
                return game_state.handle_sell_item_action(player_id, &item_ids);
```

（`validate_or_return!` 块不动。）

- [ ] **Step 4: 重写 handle_sell_item_action**

`player_common_actions.rs` :1026-1140 整个函数替换为：

```rust
    /// 处理售出行动：白天按稀有度价格出售背包中的武器/防具（绿色需成对）
    pub fn handle_sell_item_action(
        &mut self,
        player_id: &str,
        item_ids: &[String],
    ) -> Result<ActionResults, String> {
        let info_message = |message: String, sender: &str| -> ActionResults {
            ActionResult::new_info_message(
                serde_json::json!({}),
                vec![sender.to_string()],
                message,
                false,
            )
            .as_results()
        };

        // 1. 时间窗：夜窗已设置且当前不在夜间才可售出
        match (self.night_start_time, self.night_end_time) {
            (Some(start_time), Some(end_time)) => {
                let now = chrono::Utc::now();
                if now >= start_time && now <= end_time {
                    return Ok(info_message(
                        "售出只在非夜间行动时间可用".to_string(),
                        player_id,
                    ));
                }
            }
            _ => {
                return Ok(info_message(
                    "导演尚未设置夜晚行动时间，无法售出".to_string(),
                    player_id,
                ));
            }
        }

        // 2. 笔内数量
        if item_ids.is_empty() || item_ids.len() > 2 {
            return Ok(info_message(
                "一次只能售出 1 件非绿色物品或 2 件绿色物品".to_string(),
                player_id,
            ));
        }

        // 3. 逐件校验：在背包、武器/防具、稀有度有价
        let mut items = Vec::new();
        for id in item_ids {
            let Some(item) = self
                .players
                .get(player_id)
                .ok_or("Player not found")?
                .inventory
                .iter()
                .find(|i| i.id == *id)
                .cloned()
            else {
                return Ok(info_message(format!("物品 {} 不在背包中", id), player_id));
            };
            if !crate::game::game_rule_engine::Item::is_weapon_or_armor(&item) {
                return Ok(info_message("只有武器和防具可以售出".to_string(), player_id));
            }
            let Some(rarity) = item.rarity.as_deref() else {
                return Ok(info_message(
                    "该道具稀有度未开放售出".to_string(),
                    player_id,
                ));
            };
            if !self.sell_prices.iter().any(|e| e.rarity == rarity) {
                return Ok(info_message(
                    "该道具稀有度未开放售出".to_string(),
                    player_id,
                ));
            }
            items.push(item);
        }

        // 4. 配对约束：1 件不得为绿；2 件必须全绿
        let is_green =
            |i: &crate::game::game_rule_engine::Item| i.rarity.as_deref() == Some("common");
        if items.len() == 1 && is_green(&items[0]) {
            return Ok(info_message("绿色物品需成对售出".to_string(), player_id));
        }
        if items.len() == 2 && !items.iter().all(|i| is_green(i)) {
            return Ok(info_message(
                "绿色物品不能与其他稀有度混合售出".to_string(),
                player_id,
            ));
        }

        // 5. 原子应用：移除全部、累计入账
        let total_price: f64 = items
            .iter()
            .map(|i| {
                let rarity = i.rarity.as_deref().unwrap();
                self.sell_prices
                    .iter()
                    .find(|e| e.rarity == rarity)
                    .unwrap()
                    .price
            })
            .sum();
        let player_name = self.players.get(player_id).unwrap().name.clone();
        let item_names = items.iter().map(|i| i.name.clone()).collect::<Vec<_>>();
        let names_str = item_names.join("、");
        let rarity = items[0].rarity.clone().unwrap();
        let rarity_display = GameState::rarity_display_name(&rarity)
            .unwrap_or(rarity.as_str())
            .to_string();

        {
            let player = self.players.get_mut(player_id).unwrap();
            player.inventory.retain(|i| !item_ids.contains(&i.id));
            player.coins += total_price;
        }
        let coins_after = self.players.get(player_id).unwrap().coins;

        let price_str = GameState::format_price(total_price);
        let seller_msg = format!("你售出了 {}，获得 {} 货币", names_str, price_str);
        let director_msg = format!(
            "玩家 {} 售出了 {}（{}类），获得 {} 货币",
            player_name, names_str, rarity_display, price_str
        );

        let seller_data = serde_json::json!({
            "item_names": item_names,
            "price": total_price,
            "coins": coins_after,
        });
        let director_data = serde_json::json!({
            "item_names": item_names,
            "player": player_name,
            "items": items
                .iter()
                .map(|i| serde_json::json!({ "name": i.name, "rarity": i.rarity }))
                .collect::<Vec<_>>(),
            "price": total_price,
        });

        Ok(ActionResults {
            results: vec![
                ActionResult::new_system_message(
                    seller_data,
                    vec![player_id.to_string()],
                    seller_msg,
                    false,
                ),
                ActionResult::new_system_message(director_data, vec![], director_msg, true),
            ],
        })
    }
```

（函数末尾保留原函数的收尾花括号结构；若原实现最后一段与上面 `Ok(ActionResults {...})` 的字段有差异——如 `timestamp`/`broadcast_to_all` 显式赋值——保持原实现的构造写法，只替换消息与数据部分，以编译通过且两结果结构不变为准。）

- [ ] **Step 5: 编译驱动的调用点适配**

Run: `grep -rn "handle_sell_item_action(" backend/src backend/tests`
预期调用点：player_action_scheduler.rs（Step 3 已改）、sell_system_integration.rs（Step 1 已改）、sell_system_integration.rs 内其余直接调用（`sell_item_rejected_at_night` :259、`sell_item_rejected_when_night_unset` :271、`sell_item_rejects_non_weapon...` :284/:289/:293/:300、`sell_item_rejects_non_weapon_types` :321）——这些用例不触达配对约束（时间窗/背包/类型/价格校验在前），仅把 `"w1"` 类字面量参数改为 `&["w1".to_string()]` 形式。`grep -rn "ActionParams {" backend/tests` 检查结构体字面量是否需补 `item_ids: None,`。

- [ ] **Step 6: 跑测试 + 回归**

Run: `cargo test --test sell_system_integration && cargo test --lib && cargo test --test shop_rarity_integration && cargo test --test teammate_system_integration`
Expected: 全部 PASS（sell 19 个：原 13 改 2 增 6）。

- [ ] **Step 7: Commit**

```bash
git add backend/src/websocket/actions/player_action_scheduler.rs backend/src/websocket/actions/player_common_actions.rs backend/tests/sell_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(sell): green items must be sold in pairs via multi-item sell_item"
```

---

### Task 2: 前端 — 售出对话框多选与配对提示

**Files:**
- Modify: `frontend/src/stores/gameState.ts:367-369`（sellItem 签名与载荷）及 return 块（:485，名字不变无需改）
- Modify: `frontend/src/views/actor/components/SellItemDialog.vue`（整体改造）

**Interfaces:**
- Consumes: Task 1 的 WS 载荷 `{ item_ids: string[] }`
- Produces: `sellItem(itemIds: string[])`

- [ ] **Step 1: store 改签名**

`gameState.ts:367-369` 替换为：

```ts
  const sellItem = (itemIds: string[]) => {
    sendPlayerAction('sell_item', { item_ids: itemIds })
  }
```

Run: `grep -rn "sellItem(" frontend/src` 确认调用点只有 SellItemDialog.vue（CompactActionPanel 只开对话框不直接调用；若有其他调用点同步改数组传参）。

- [ ] **Step 2: SellItemDialog.vue 改造**

`<script setup>` 中 `selected`、`sellableItems`、`handleConfirm` 部分替换为（`SellableItem` 接口加 `rarity` 字段）：

```ts
const selected = ref<string[]>([])

interface SellableItem {
  id: string
  name: string
  price: number
  rarity: string | null
}

const sellableItems = computed<SellableItem[]>(() => {
  const priceOf = (rarity: string | null) =>
    rarity ? store.sellPrices.find((e) => e.rarity === rarity)?.price : undefined
  return props.inventory
    .filter(
      (it) =>
        (it.item_type?.type === 'weapon' || it.item_type?.type === 'armor') &&
        priceOf(it.rarity) !== undefined
    )
    .map((it) => ({ id: it.id, name: it.name, price: priceOf(it.rarity) as number, rarity: it.rarity }))
})

const selectedItems = computed(() =>
  sellableItems.value.filter((it) => selected.value.includes(it.id))
)

const validationError = computed(() => {
  const greens = selectedItems.value.filter((it) => it.rarity === 'common')
  const others = selectedItems.value.filter((it) => it.rarity !== 'common')
  if (greens.length === 1 && others.length === 0)
    return '绿色物品需成对售出，请再勾选 1 件绿色物品'
  if (greens.length >= 1 && others.length >= 1) return '绿色物品不能与其他稀有度混合售出'
  if (others.length >= 2) return '非绿色物品一次只能售出 1 件'
  return ''
})

const canConfirm = computed(
  () => selectedItems.value.length > 0 && validationError.value === ''
)

watch(
  () => props.modelValue,
  (open) => {
    if (open) selected.value = []
  }
)

const handleConfirm = () => {
  if (!canConfirm.value) return
  store.sellItem(selected.value)
  emit('update:modelValue', false)
}
```

模板中 radio 部分与 footer 替换为：

```vue
    <el-checkbox-group v-else v-model="selected" class="check-list">
      <el-checkbox v-for="it in sellableItems" :key="it.id" :value="it.id">
        {{ it.name }}
        <span class="price">→ {{ formatPrice(it.price) }} 币</span>
      </el-checkbox>
    </el-checkbox-group>
    <div v-if="validationError" class="pair-hint">{{ validationError }}</div>
    <template #footer>
      <el-button @click="$emit('update:modelValue', false)">取消</el-button>
      <el-button type="primary" :disabled="!canConfirm" @click="handleConfirm">
        确认售出
      </el-button>
    </template>
```

scoped style 中 `.radio-list` 选择器改名 `.check-list`（保留 flex 纵向布局与 gap），追加：

```css
.pair-hint {
  margin-top: 8px;
  color: #e6a23c;
  font-size: 12px;
}
```

- [ ] **Step 3: 验证构建**

Run: `cd F:/github/royale-arena/frontend && pnpm build`
Expected: 成功（仅预先存在的 chunk 大小警告）。静态检查：单勾绿件按钮禁用并显示配对提示；混合勾选显示混合提示；勾 2 绿可提交。

- [ ] **Step 4: Commit**

```bash
git add frontend/src/stores/gameState.ts frontend/src/views/actor/components/SellItemDialog.vue
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(sell): checkbox multi-select dialog with green-pair validation"
```

---

## Self-Review

**Spec coverage:** §交易规则 6 行 → Task 1 Step 4 配对约束 + Step 1 六个新测试；§载荷与签名 → Task 1 Step 3；§校验链 6 步顺序与实现一致（长度在逐件校验前）；§应用与消息（合并顿号、2× 价、data 数组）→ Step 4；§前端提示三条 + 按钮联动 → Task 2 Step 2；§错误处理表各行均有测试；§测试计划 7 项 → Step 1（蓝单件回归 = 改造后的 succeeds_in_daytime；对中缺件 = missing_item 用例；pnpm build = Task 2 Step 3）。
**Placeholder scan:** 无 TBD/TODO；Step 4 的"保持原实现构造写法"注记是有界适配指令（原函数末段已完整给出替换体）。
**Type consistency:** `item_ids: Option<Vec<String>>`（调度器）↔ `&[String]`（handler，调度器传 `&item_ids` 自动 deref）↔ WS 载荷 `{ item_ids }` ↔ TS `sellItem(itemIds: string[])` 一致；`validationError`/`canConfirm` 命名前后一致。
**Scope:** 单计划两任务，各自独立可测可评审。
