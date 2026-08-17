# 售出系统（Sell System）设计规格

日期：2026-08-17
状态：已确认（用户批准设计方向与 0.5 步长价格精度）

## 目标

玩家在**非夜间行动时间**可将背包中的武器/防具按稀有度价格售出换取货币；导演在局内配置各稀有度（绿/蓝/紫/橙）的售出价格。与商店互补：商店=夜间导演卖玩家买，售出=白天玩家卖系统收。

## 范围

- 后端：GameState 新增 `sell_prices` 配置；导演动作 `sell_set_price` / `sell_remove_price`；玩家动作 `sell_item`；`Player.coins` 改为 f64。
- 前端：导演端"售出系统"配置卡片；演员端[售出]按钮 + 售出对话框。
- 不做：消耗品/功能物品售出、已装备道具售出、次数/体力限制、导演总开关（无配置=禁用）。

## 数据模型

```rust
pub struct SellPriceEntry {
    pub id: String,       // uuid
    pub rarity: String,   // "common" | "rare" | "epic" | "legendary"
    pub price: f64,       // > 0，且 price * 2 为整数（0.5 步长）
}
```

- `GameState.sell_prices: Vec<SellPriceEntry>`，`#[serde(default)]`；同一稀有度最多一条。
- `Player.coins: i32` → `f64`（`#[serde(default)]`）。coins 仅存在于游戏状态 JSON，非数据库列；商店整数价格自动兼容；余额支持一位小数。
- 旧存档兼容：`sell_prices` 缺省为空（=售出禁用）；`coins` 缺省走 serde default 0。

## 后端 — 导演动作

与 `shop_list_item` / `shop_delist_item` 同模式（director_action_scheduler.rs 分发 → director_common_actions.rs 处理）：

### `sell_set_price { rarity, price }`
- rarity 必须为四值之一；price > 0 且 `price * 2` 为整数（浮点容差 1e-9）。
- 该稀有度已存在 → 更新价格；不存在 → 新增。
- 成功后 SystemNotice 广播全体玩家（`broadcast_to_all`）："导演更新了售出价格：{稀有度中文名} {价格} 货币"。
- 拒绝时 Info 仅返回发起失败原因给导演（照 shop_list_item 模式）。

### `sell_remove_price { rarity }`
- 删除该稀有度配置（不存在则 Info 提示）。
- 成功后 SystemNotice 广播全体："导演关闭了{稀有度中文名}的售出"。

稀有度中文名映射：common=绿、rare=蓝、epic=紫、legendary=橙。

## 后端 — 玩家动作 `sell_item { item_id }`

分发（player_action_scheduler.rs，`"sell_item"` arm）：校验 `Alive, Born, NotBound`，提取 `item_id`，调用 `handle_sell_item_action(player_id, &item_id)`。

校验链（任一失败 → Info 仅发发起方，无状态变更）：

1. **时间窗**：`night_start_time`/`night_end_time` 必须都已设置且 start < now < end 之外才可售。
   - 未设置 → "导演尚未设置夜晚行动时间，无法售出"
   - 当前处于夜间 → "售出只在非夜间行动时间可用"
2. **道具在 inventory 中**：不在 → "物品不在背包中"（已装备道具不在 inventory，天然排除）。
3. **道具为武器或防具**（按 `item_type` 判定，其他类型 → "只有武器和防具可以售出"）。
4. **稀有度已配置**：`item.rarity` 为 None 或 `sell_prices` 无对应条目 → "该道具稀有度未开放售出"。

成功：
- `inventory.retain` 移除该道具；`coins += price`。
- 价格展示格式化：整数显示不带小数（"2 货币"），0.5 显示 "0.5 货币"。
- 三条 SystemNotice：
  - 发起方（不广播导演）："你售出了 {item_name}，获得 {price} 货币"
  - 导演专属（`broadcast_players` 为空 + `broadcast_to_director=true`，走导演专属日志通道）："玩家 {player_name} 售出了 {item_name}（{稀有度中文名}），获得 {price} 货币"

## 广播 JSON

`GameState::to_director_client_json` 与 `to_player_client_json` 均新增 `"sell_prices": self.sell_prices`（与 `"shop"` 并列）。条目含 `id, rarity, price`。

## 前端 — 导演端

`InGameManagement.vue` 在商店管理卡片下方新增"售出系统"卡片（新组件 `SellSystemCard.vue`）：

- 列表：已配置条目（稀有度色标 + 中文名 + 价格 + 删除按钮）。
- "+"按钮 → 弹出选择器只列出**未配置**的稀有度 → 选定后输入价格（`el-input-number`，step 0.5，min 0.5）→ 保存调 `sendDirectorAction('sell_set_price', { rarity, price })`。
- 删除调 `sendDirectorAction('sell_remove_price', { rarity })`。
- 数据源 `store.globalState.sell_prices`（实时）。

## 前端 — 演员端

`CompactActionPanel.vue` 商店按钮旁新增[售出]按钮：

- 可用条件：`!nightActionActive && hasSpawned && !player.is_bound`（与商店互补；夜晚时间未设置时 `nightActionActive` 为 false，但按需求"未设夜=禁售"，按钮禁用并提示）。
- 点击弹出 `SellItemDialog.vue`：
  - 列出背包中武器/防具且稀有度已配置价格的道具，显示每件可得的货币数（按 sell_prices）。
  - 选择一件 → 确认 → `sendPlayerAction('sell_item', { item_id })`。
  - 无可售道具 → 空状态文案"没有可售出的道具"。
- 状态栏货币显示 `coins` 支持一位小数。

## 错误处理汇总

| 场景 | 行为 |
|---|---|
| 夜间售出 / 未设夜 | 后端 Info 拒绝；前端按钮禁用 |
| 已装备道具 | 不在 inventory，天然不可选（对话框不列出） |
| 非武器/防具 | 对话框不列出；后端兜底拒绝 |
| 稀有度未配置 | 对话框不列出；后端兜底拒绝 |
| 价格非法（≤0 / 非 0.5 倍数） | 导演动作 Info 拒绝 |

## 测试计划

- 后端集成测试 `sell_system_integration.rs`：
  - 配置：set（新增/改价/非法 rarity/非法价格）、remove（存在/不存在）、同稀有度唯一。
  - 售出：白天成功（扣道具+加货币+三条消息）、夜间拒绝、未设夜拒绝、非武器拒绝、未配置稀有度拒绝、不在背包拒绝。
  - coins f64：0.5 累加正确（卖两件 0.5 得 1.0）。
  - 广播 JSON 含 sell_prices。
  - 旧存档兼容（无 sell_prices 字段的 JSON 反序列化）。
- 前端：`pnpm build`（vue-tsc）通过；导演卡片与演员对话框按组件树静态验证（无浏览器环境）。

## 实施约束

- 遵循现有 shop/transfer 代码模式（TDD、小步提交）。
- 构建：`export PATH="/c/msys64/mingw64/bin:$PATH"`。
- 集成测试无需 DATABASE_URL；test_game_api 需 DATABASE_URL（预先存在的限制）。
