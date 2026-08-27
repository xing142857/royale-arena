# 永久增益道具类别设计（Permanent Buff Items）

日期：2026-08-25
状态：已与用户确认设计，待实现

## 背景与目标

新增一类"永久增益"道具：使用后永久提升玩家的血量上限 / 体力上限 / 背包上限。设计方法参照现有血包（Consumable，`effect_type: "heal"`）的完整管道，但作为独立的第 7 类道具，便于导演在空投、商店界面单独识别和投放。

## 已确认的设计决策

| 决策点 | 结论 |
|--------|------|
| 实现路径 | 方案 A：新增独立道具类别 `permanent_buff`（不扩展消耗品 effect_type） |
| 数值档位 | 三档按比例：血上限 +20/+50/+100，体力上限 +20/+50/+100，背包 +2/+4/+6 |
| 当前值 | 只涨上限，当前血量/体力不变 |
| 使用次数 | 不限次数，但受属性硬上限约束 |
| 硬上限 | 血量 300 / 体力 300 / 背包 12 格（默认值，初始上限与硬上限均在规则配置中可由导演自定义） |
| 溢出处理 | 溢出浪费：clamp 到硬上限，道具照常消耗（与满血用血包行为一致）；完全到上限也可使用（+0 浪费） |
| 商店上架 | 允许。现有"拒绝武器/防具、其余按名称上架"逻辑自动放行，无需改动 |
| 稀有度 | 默认清单不设稀有度，与血包一致；`rarity` 作为可选字段保留在配置结构中，导演可自行添加 |
| 体力消耗 | 走现有 `action_costs.use` 结算路径，默认配置为 0（不扣体力），导演可配置 |

## 数据模型

### 后端 `backend/src/game/game_rule_engine.rs`

仿照消耗品模式新增：

```rust
pub enum ItemType {
    // ... 现有 6 类
    PermanentBuff(PermanentBuffProperties),  // serde tag: "permanent_buff"
}

pub struct PermanentBuffProperties {
    pub effect_type: String,   // "max_life" | "max_strength" | "max_backpack"
    pub effect_value: i32,
}

pub struct PermanentBuffConfig {
    pub name: String,
    pub internal_name: Option<String>,
    pub rarity: Option<String>,
    pub properties: PermanentBuffProperties,
}
```

`ItemsByCategory` 新增 `permanent_buffs: Vec<PermanentBuffConfig>`（serde default 空数组，兼容旧配置）。`create_item_from_name` 增加对应搜索分支。

### 玩家上限配置 `PlayerConfig`

新增三个可选字段（serde default，旧规则 JSON 不写取默认值）：

- `max_life_cap: i32`（默认 300）
- `max_strength_cap: i32`（默认 300）
- `max_backpack_items_cap: usize`（默认 12）

### 玩家模型 `backend/src/websocket/models.rs`

- `Player` 新增 `max_backpack_items: usize` 字段，与 `max_life`/`max_strength` 并列；出生时从 `player_config.max_backpack_items` 初始化
- 旧玩家状态兼容：反序列化后对缺失该字段的玩家补为规则初始值（在现有的状态重建钩子处处理），避免旧存档玩家背包上限为 0

### 默认道具清单（9 个）

命名风格仿血包 `[HP30]绷带` / `[MP20]矿泉水`：

| 属性 | 一档 | 二档 | 三档 |
|------|------|------|------|
| 血量上限 | [HP上限+20]养生丸 | [HP上限+50]壮骨丹 | [HP上限+100]金钟罩 |
| 体力上限 | [MP上限+20]干粮 | [MP上限+50]行军丹 | [MP上限+100]龙力丸 |
| 背包上限 | [背包+2]腰包 | [背包+4]行囊 | [背包+6]百宝袋 |

三份配置文件同步更新（各加 `player_config` 三个 cap 字段与 `permanent_buffs` 数组）：

- `director_rules_config.json`
- `full-feature-rules-template.json`
- `frontend/src/constants/defaultRulesConfig.ts`

## 使用逻辑（`backend/src/websocket/actions/player_use_action.rs`）

`handle_use_action` 的类型分发处新增 `ItemType::PermanentBuff` 分支，逻辑独立为 `handle_permanent_buff_use`：

1. `match effect_type`：
   - `"max_life"` → `player.max_life = min(player.max_life + 效果值, 生效上限)`
   - `"max_strength"` → 同理作用于 `max_strength`
   - `"max_backpack"` → `player.max_backpack_items = min(当前 + 效果值, 生效上限)`
   - 其他值 → 返回错误（走现有失败路径：道具原位插回背包、不扣体力）
2. **生效上限** = `max(cap, 规则初始上限)`，防止导演把 cap 配得比初始值低时使用道具反而降上限
3. 溢出浪费：clamp 后即浪费，道具照常消耗；完全到 cap 也可使用（+0）
4. 当前血量/体力保持不变
5. 成功后走现有体力结算（`action_costs.use_item`，默认 0）+ 道具销毁（`with_reinsert(false)`）
6. 日志 outcome 返回 `max_life` + `max_life_delta`（或 `max_strength`/`max_backpack_items` 对应字段）供前端展示

## 背包上限引用点改造

背包上限从"全局规则"变为"玩家字段"，替换 5 处 `rule_engine.player_config.max_backpack_items` 引用为 `player.max_backpack_items`：

- `player_common_actions.rs` 3 处（拾取物品检查、相关上限判断）
- `player_action_scheduler.rs` 1 处
- `game_state_common.rs` 1 处（击杀者收缴战利品上限）

## 商店

无需改动：按名称上架逻辑自动放行新类别；按稀有度类目上架（`shop_list_rarity`）仅支持武器/防具，不涉及。

## 前端改动

- `frontend/src/utils/itemConfigUtils.ts`：`PermanentBuffConfig` 接口 + snake_case→camelCase 归一化；类别汇总类型加 `permanent_buffs`
- `frontend/src/utils/itemParser.ts`：`parseAllItems` 解析 `permanent_buffs` 纳入 `allItems`；`hasAnyItem` 检查加新类别
- 导演空投界面（`BatchAirdropDialog.vue` 等）：道具选择列表新增"永久增益"分组（若组件按类别写死需补一项）
- 玩家端使用入口为通用 `use` 动作，无需特判；背包/道具类型标签展示新类别名称
- `GameRulesPreview.vue` 等规则预览组件按需补充新类别

## 文档更新

- `frontend/public/docs/game-rules-explain.md`：新增"永久增益"章节（字段说明、三种 effect_type、cap 语义、溢出浪费规则）
- `docs/api/data-models.md`：Player 的 `max_backpack_items`、PlayerConfig 的三个 cap 字段
- `docs/api/game-rules-config.md`：`permanent_buffs` 配置说明
- `docs/api/ws/player-actions.md`：use 动作返回字段补充（如有必要）

## 测试计划（`backend/tests/`）

新增 `permanent_buff_integration.rs`：

1. 三类道具各使用一次 → 上限提升、当前血量/体力不变、道具销毁、体力按配置结算（默认 0）
2. 连续使用至 cap → 上限 clamp、溢出浪费
3. 背包扩容后 → 可拾取超过原上限的物品
4. 未知 `effect_type` → 报错、道具回插、不扣体力
5. 旧规则配置（无 cap 字段、无 `permanent_buffs` 数组）→ 正常加载，cap 取默认值 300/300/12
6. 旧玩家状态反序列化 → `max_backpack_items` 补为规则值而非 0
7. 商店按名称上架永久增益道具 → 成功
8. `game_rule_engine_integration.rs` 补充 `permanent_buffs` 解析断言（仿血包断言写法）

## 错误处理汇总

- 未知 `effect_type`：报错回插，不消耗体力
- 导演配置 `cap < 初始上限`：生效上限取两者较大值，道具永不降低上限
- 中途修改规则 cap：已提升的玩家上限为绝对值，不受影响；再次使用道具时按新 cap clamp
