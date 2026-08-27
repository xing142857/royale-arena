# 商店稀有度类目改造（Shop Rarity Rework）设计规格

日期：2026-08-17
状态：已确认（用户批准设计方案与库存上限规则）

## 目标

导演不再为武器/防具上架具体物品，改为按**稀有度类目**（武器/防具 × 绿/蓝/紫/橙，共 8 类）定价上架；玩家购买稀有度条目时，后端从道具库该稀有度的候选显示名中**随机抽取**一件（全局去重），避免场上出现同名称武器/防具。其他物品类别（消耗品/功能物品/升级道具/货币）保持具体物品上架不变。

## 已确认的决策

1. **完全替换**：武器/防具只走稀有度类目；`shop_list_item` 拒绝武器/防具的具体上架；旧存档中已有的武器/防具具体条目仍可购买（原路径兼容）。
2. **限量库存**：沿用现有库存语义——卖出一件减一件，减到 0 自动移除条目。
3. **全局去重**：抽取时剔除场上已存在的名字（复用 `collect_existing_weapons_and_armor_names()`，覆盖所有玩家背包/已装备/地面道具）及本次购买已抽中的名字；候选耗尽 → 整笔购买失败。
4. **整数价格**：商店价格保持 i32、≥1（与售出系统的 0.5 步长不同，是有意为之）。
5. **库存上限**：`quantity` ≤ 道具库中该稀有度该类别所有配置的 `display_names` 数量之和（武器、防具同样规则）；超出则上架拒绝。

## 数据模型

```rust
pub struct ShopListing {
    pub id: String,
    pub item_name: String,          // 稀有度条目存固定展示文案："绿类武器（随机）"
    pub price: i32,                 // 整数 ≥1
    pub quantity: i32,
    #[serde(default)]
    pub item_kind: Option<String>,  // Some("weapon" | "armor") = 稀有度条目
    #[serde(default)]
    pub rarity: Option<String>,     // 与 item_kind 同时出现
}
```

- 两个新字段同时 `Some` = 稀有度随机条目；缺省（旧存档）= 具体物品条目。
- 稀有度条目的 `item_name` 由类别+稀有度生成展示文案，供演员端列表直接渲染。
- 不新增独立数组；`GameState.sell_prices` 与售出系统不受影响。

## 后端 — 导演动作

### 新动作 `shop_list_rarity { shop_item_kind, shop_rarity, price, quantity }`

`DirectorActionParams` 新增字段 `shop_item_kind: Option<String>`、`shop_rarity: Option<String>`；价格/数量复用现有 `price: Option<i32>`（director_action_scheduler.rs:46）与 `quantity: Option<i32>`（:47）字段。稀有度中文名复用 `GameState::rarity_display_name`（售出系统已提供）。

校验链（任一失败 → Info 仅发导演，无状态变更）：

1. `shop_item_kind` ∈ {"weapon", "armor"}；`shop_rarity` ∈ {"common","rare","epic","legendary"}。
2. `price` ≥ 1 整数；`quantity` ≥ 1。
3. **道具库非空**：`rule_engine.items_config` 中该类别、该 rarity 的配置存在且 `display_names` 非空，否则 "道具库中没有{稀有度中文名}类{武器|防具}"。
4. **库存上限**：`quantity` ≤ 该类别该稀有度全部配置的 `display_names` 数量之和，否则 "库存不能超过道具库中{稀有度中文名}类{武器|防具}的名称总数（N）"。
5. **类目唯一**：shop 中不存在同 `(item_kind, rarity)` 条目，否则 "该类目已上架"。

成功：push `ShopListing { id: uuid, item_name: "{绿}类{武器}（随机）", price, quantity, item_kind, rarity }`；SystemNotice 广播全体："导演上架了{稀有度中文名}类{武器|防具}（随机），价格 {price} 货币，库存 {quantity}"。

### `shop_list_item`（具体物品）加限制

现有校验后追加：`create_item_from_name` 解析出的 `item_type` 为武器或防具 → Info 拒绝 "武器和防具请按稀有度类目上架"。消耗品/功能物品/升级道具/货币照旧。

### `shop_delist_item` 不变

按 id 下架，两类条目通吃；稀有度条目 `quantity` 减到 0 由现有 `retain` 自动移除。

## 后端 — 玩家购买（shop_buy 扩展）

现有原子性流程保持：聚合重复 listing → 库存/价格/余额/背包空间检查 → **预先创建全部物品** → 一次性入包/扣款/扣库存。仅预创建循环按条目类型分派：

- **具体物品条目**：`create_item_from_name(item_name)`（原路径；兼容旧存档武器/防具条目）。
- **稀有度条目**：每件抽取——
  1. 候选名 = 该类别全部 rarity 匹配配置的 `display_names` 展平；
  2. 剔除 `collect_existing_weapons_and_armor_names()`（场上全局）+ 本次购买已抽中的名字；
  3. 随机选一，`create_item_from_name` 创建；
  4. 剩余为空 → 整笔取消，Info 仅发买家："{稀有度中文名}类{武器|防具}可抽选的名称已全部在场，购买失败"，零状态变更（现有"先创建后提交"结构天然保证）。

成功消息沿用现有购买明细（自然列出抽到的具体名称）与导演日志，不改。

## 广播与前端类型

- 广播 JSON：`shop` 数组条目新增 `item_kind`/`rarity`（可缺省），结构不变。
- TS：`ShopListing` 加 `item_kind?: 'weapon' | 'armor'; rarity?: 'common' | 'rare' | 'epic' | 'legendary'`。
- store 新增：`shopListRarity(itemKind: 'weapon' | 'armor', rarity: string, price: number, quantity: number)` → `sendDirectorAction('shop_list_rarity', { shop_item_kind, shop_rarity, price, quantity })`。

## 前端 — 导演端 ShopManagement.vue

"+"对话框改为先选类别：

- **武器/防具** → 稀有度选择器（只列**未上架**且**道具库非空**的稀有度，已上架/空库禁用或隐藏）+ 价格（整数 ≥1）+ 数量（`el-input-number`，max = 该稀有度名称总数）→ `store.shopListRarity(...)`。
- **其他类别** → 原具体物品选择器 + 价格 + 数量 → `store.shopListItem(...)` 照旧。
- 列表：稀有度条目显示展示文案 + 稀有度色标；删除按钮照旧（`shop_delist_item`）。

## 前端 — 演员端

- 商店列表结构未变，直接渲染（展示名/价格/库存/数量输入照旧）；稀有度条目加稀有度色标。
- 无需新动作（`shop_buy` payload 不变，listing_id 通吃两类）。

## 错误处理汇总

| 场景 | 行为 |
|---|---|
| 具体武器/防具上架 | `shop_list_item` Info 拒绝 |
| 非法 item_kind / rarity | `shop_list_rarity` Info 拒绝 |
| 道具库无该稀有度该类别 | 上架时 Info 拒绝 |
| 库存超过道具库名称总数 | 上架时 Info 拒绝 |
| 重复类目上架 | Info "该类目已上架" |
| 购买时名称耗尽 | 整笔取消 Info 仅发买家，无状态变更 |
| 旧存档武器/防具具体条目 | 仍可购买（原路径） |

## 测试计划

- 后端集成测试 `shop_rarity_integration.rs`（新建）：
  - 上架：成功（字段/文案/广播）；非法 kind/rarity；价格/数量非法；道具库空拒绝；库存超上限拒绝；重复类目拒绝；具体武器上架被拒；具体消耗品上架仍成功。
  - 购买：成功（抽中道具稀有度正确、名称不在场上已有集合、扣款扣库存）；同条目买多件互不同名；名称耗尽 → 整笔失败零状态变更；库存减到 0 自动移除条目；余额不足照旧。
  - 兼容：无 `item_kind`/`rarity` 字段的旧条目 JSON 反序列化为具体物品条目；旧武器条目可买。
  - 广播 JSON 含新字段。
- 前端：`pnpm build`（vue-tsc）通过；导演 + 对话框与演员列表按组件树静态验证（无浏览器环境）。

## 实施约束

- 遵循现有 shop/sell 代码模式（TDD、小步提交）。
- 构建：`export PATH="/c/msys64/mingw64/bin:$PATH"` 后再跑 cargo。
- 集成测试无需 DATABASE_URL；`test_game_api.rs` 需 DATABASE_URL（预先存在的限制）。
- 提交用 `git -c user.name="Wang Li" -c user.email="itx351@126.com" commit`。
