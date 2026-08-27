# 售出系统（Sell System）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 玩家在非夜间行动时间按稀有度价格出售背包中的武器/防具换取货币；导演局内配置绿/蓝/紫/橙四类售出价格。

**Architecture:** 照搬商店（shop）的既有模式：`GameState` 上新增 `sell_prices` 运行时配置，导演动作 `sell_set_price`/`sell_remove_price`，玩家动作 `sell_item`，广播 JSON 携带 `sell_prices`。`Player.coins` 从 i32 改为 f64 以支持 0.5 步长价格。

**Tech Stack:** Rust (axum, serde, uuid), Vue 3 + TypeScript + Pinia + Element Plus

## Global Constraints

- 价格为 f64，必须 > 0 且为 0.5 的整数倍（`(price * 2).round() - price * 2` 绝对值 < 1e-9）
- rarity 只允许 `"common" | "rare" | "epic" | "legendary"`，同一稀有度最多一条配置
- 中文名映射：common=绿、rare=蓝、epic=紫、legendary=橙
- 时间规则：售出仅当 `night_start_time`/`night_end_time` 都已设置且当前时间**不在** `[start, end]` 区间内；未设置 → "导演尚未设置夜晚行动时间，无法售出"；夜间 → "售出只在非夜间行动时间可用"
- 只有武器和防具可售（按 `item_type` 判定）；道具必须在 `inventory` 中（已装备道具在 `equipped_weapon/equipped_armor`，天然排除）
- 稀有度未配置 → "该道具稀有度未开放售出"
- 成功产生三条 SystemNotice：发起方（不广播导演）"你售出了 {item}，获得 {price} 货币"；导演专属（`broadcast_players` 为空 + `broadcast_to_director=true`）"玩家 {name} 售出了 {item}（{稀有度中文名}），获得 {price} 货币"。价格展示：整数不带小数点（"2 货币"），0.5 显示 "0.5 货币"
- 拒绝路径全部返回 Info 仅发发起方，无任何状态变更
- 商店上架价格保持 i32 不变
- 构建：`export PATH="/c/msys64/mingw64/bin:$PATH"` 后再跑 cargo（dlltool.exe）
- 集成测试无需 DATABASE_URL；`test_game_api.rs` 需 DATABASE_URL（预先存在的限制，不是本次问题）
- 提交用 `git -c user.name="Wang Li" -c user.email="itx351@126.com" commit`
- `backend/tests/currency_and_movement_integration.rs` 含用户未提交的手工修改：类型适配时只做最小 i32→f64 修改，**不得回退或重写用户的其他改动**

---

### Task 1: 数据模型 — SellPriceEntry、GameState.sell_prices、Player.coins 改 f64

**Files:**
- Modify: `backend/src/websocket/models.rs`
- Modify: `backend/src/websocket/actions/director_action_scheduler.rs:27`（coins 参数类型）
- Modify: `backend/src/websocket/actions/director_common_actions.rs:410-445`（handle_set_player_coins）
- Modify: `backend/src/websocket/actions/game_state_common.rs:87-138`（死亡货币转移）
- Modify: `backend/src/websocket/actions/player_common_actions.rs:755-840`（shop_buy 扣款）
- Modify: `backend/src/websocket/actions/player_use_action.rs:280-290`（使用货币道具加款）
- Test: `backend/tests/sell_system_integration.rs`（新建）

**Interfaces:**
- Produces: `pub struct SellPriceEntry { pub id: String, pub rarity: String, pub price: f64 }`；`GameState.sell_prices: Vec<SellPriceEntry>`；`Player.coins: f64`
- Produces（测试辅助，后续任务复用）: `sell_test_rules(mode)`, `build_sell_game_state()`, `sell_add_player(&mut state, id, name)`, `sell_set_night_window(&mut state, start_offset_secs, end_offset_secs)`（相对 Utc::now() 的偏移）

- [ ] **Step 1: 新建测试文件，写失败测试**

```rust
// backend/tests/sell_system_integration.rs
use chrono::{Duration, Utc};
use royale_arena_backend::game::game_rule_engine::GameRuleEngine;
use royale_arena_backend::websocket::models::GameState;
use serde_json::json;

const SELL_RULES: &str = r#"{
    "map": { "places": ["码头"], "safe_places": [] },
    "player": { "max_life": 100, "max_strength": 100, "daily_life_recovery": 0,
                 "daily_strength_recovery": 40, "search_cooldown": 30,
                 "max_backpack_items": 6, "unarmed_damage": 5 },
    "action_costs": { "move": 5, "search": 5, "pick": 0, "attack": 0,
                       "equip": 0, "use": 0, "throw": 0, "deliver": 10 },
    "rest_mode": { "life_recovery": 25, "strength_recovery": 1000, "max_moves": 1 },
    "teammate_behavior": 0,
    "death_item_disposition": "killer_takes_loot",
    "display_names": {},
    "items_config": { "rarity_levels": [], "items": {}, "upgrade_recipes": {} }
}"#;

fn build_sell_game_state() -> GameState {
    GameState::new(json!(SELL_RULES))
}

fn sell_add_player(state: &mut GameState, id: &str, name: &str) {
    let engine = GameRuleEngine::from_json(&state.rules_config.to_string()).unwrap();
    let player = royale_arena_backend::websocket::models::Player::new(
        id.to_string(), name.to_string(), "pw".to_string(), 0, &engine,
    );
    state.players.insert(id.to_string(), player);
}

fn sell_set_night_window(state: &mut GameState, start_offset_secs: i64, end_offset_secs: i64) {
    state.night_start_time = Some(Utc::now() + Duration::seconds(start_offset_secs));
    state.night_end_time = Some(Utc::now() + Duration::seconds(end_offset_secs));
}

#[test]
fn coins_supports_fractional_values() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    let p = state.players.get_mut("p1").unwrap();
    p.coins += 0.5;
    p.coins += 0.5;
    let p = state.players.get("p1").unwrap();
    assert!((p.coins - 1.0).abs() < 1e-9, "两件 0.5 应累计为 1.0，实际 {}", p.coins);
}

#[test]
fn game_state_deserializes_without_sell_prices() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    let serialized = serde_json::to_string(&state).unwrap();
    // 模拟旧存档：手工删掉 sell_prices 字段后仍能加载
    let legacy = serialized.replace("\"sell_prices\":[]", "").replace(",}", "}");
    let restored: GameState = serde_json::from_str(&legacy)
        .expect("旧存档（无 sell_prices）必须能反序列化");
    assert!(restored.sell_prices.is_empty());
}
```

注意：`GameState::new` 的真实签名以 `backend/src/websocket/models.rs` 现有代码为准（如它不接受 JsonValue 而是别的参数，参考 `backend/tests/teammate_system_integration.rs` 里 `build_empty_game_state` 的写法照抄适配——那个文件已验证可用）。`Player` 的真实导入路径同样参考该文件。

- [ ] **Step 2: 跑测试确认失败**

Run: `export PATH="/c/msys64/mingw64/bin:$PATH" && cd backend && cargo test --test sell_system_integration`
Expected: 编译失败——`sell_prices` 字段不存在 / coins 不支持小数。

- [ ] **Step 3: models.rs 加结构与字段、coins 改 f64**

在 `ShopListing` 之后（约 :82）加：

```rust
/// 售出系统稀有度价格条目
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct SellPriceEntry {
    /// 条目唯一ID
    pub id: String,
    /// 稀有度：common | rare | epic | legendary
    pub rarity: String,
    /// 售出价格（货币数，0.5 步长）
    pub price: f64,
}
```

`GameState`（:131 `shop` 字段后）加：

```rust
    /// 售出系统：稀有度 → 价格（同一稀有度最多一条）
    #[serde(default)]
    pub sell_prices: Vec<SellPriceEntry>,
```

`GameState::new`（或等价构造函数）的初始化列表加 `sell_prices: Vec::new(),`。

`Player.coins`（:180）`pub coins: i32` → `pub coins: f64`（保留 `#[serde(default)]`，default 改为 `#[serde(default)]` 即可，0 会推断为 f64 的 0.0——若编译器要求字面量，写 `coins: 0.0` 于 :216）。

- [ ] **Step 4: 全量类型适配（逐文件最小修改）**

- `director_action_scheduler.rs:27`: `pub coins: Option<i32>` → `Option<f64>`
- `director_common_actions.rs:414`: `coins: i32` → `coins: f64`（:419/422/437-439 的赋值与比较不需额外改动）
- `game_state_common.rs:118`: `let mut transferred_coins: i32 = 0` → `f64`；:131-132 `checked_add` 改为直接 `killer.coins += victim_coins; transferred_coins = victim_coins;`（f64 无需防溢出，游戏内数值极小）。若 :128 `victim_coins > 0` 因类型报错，改为 `> 0.0`；`victim_coins` 来源 :87 `let coins = player.coins` 自动变 f64
- `player_common_actions.rs:761`: `if player.coins < total_cost` → `if player.coins < total_cost as f64`；:766 format 不变；:817-820 `checked_sub(...).expect(...)` 改为 `player.coins -= total_cost as f64;`
- `player_use_action.rs:282-285`: `checked_add(properties.value).ok_or_else(...)` 改为 `player.coins += properties.value as f64;`
- 用 `grep -rn "coins" backend/src backend/tests` 复查剩余编译错误并做同样最小适配。**`backend/tests/currency_and_movement_integration.rs` 里有用户未提交的手工修改——只做类型最小修正，不要动其他逻辑。**
- 若 ` GameState` 还有其他构造/测试构造点报 `sell_prices` 缺失，同样补 `sell_prices: Vec::new()`。

- [ ] **Step 5: 跑新测试与全部回归**

Run: `cargo test --test sell_system_integration && cargo test --lib && cargo test --test teammate_system_integration`
Expected: 全部 PASS（含既有测试）。

- [ ] **Step 6: Commit**

```bash
git add backend/src/websocket/models.rs backend/src/websocket/actions/ backend/tests/sell_system_integration.rs backend/tests/currency_and_movement_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(sell): add SellPriceEntry model, sell_prices state, f64 coins"
```

---

### Task 2: 导演动作 — sell_set_price / sell_remove_price

**Files:**
- Modify: `backend/src/websocket/actions/director_action_scheduler.rs`
- Modify: `backend/src/websocket/actions/director_common_actions.rs`
- Test: `backend/tests/sell_system_integration.rs`（追加）

**Interfaces:**
- Consumes: Task 1 的 `SellPriceEntry`、`GameState.sell_prices`、测试辅助函数
- Produces: `DirectorActionParams` 新字段 `sell_rarity: Option<String>`、`sell_price: Option<f64>`；`GameState::handle_sell_set_price(&mut self, rarity: String, price: f64) -> Result<ActionResults, String>`；`GameState::handle_sell_remove_price(&mut self, rarity: &str) -> Result<ActionResults, String>`
- Produces（Task 3/5 复用）: `pub fn rarity_display_name(rarity: &str) -> Option<&'static str>`（common→绿、rare→蓝、epic→紫、legendary→橙，非法返回 None）

- [ ] **Step 1: 追加失败测试**

```rust
use royale_arena_backend::websocket::actions::director_action_scheduler::{DirectorActionParams, DirectorActionScheduler};
use royale_arena_backend::websocket::models::MessageType;

fn director_dispatch(state: &mut GameState, params: serde_json::Value) -> Result<royale_arena_backend::websocket::models::ActionResults, String> {
    let action_type = params["action_type"].as_str().unwrap_or("").to_string();
    let ap = DirectorActionParams::from_json(&params)?;
    DirectorActionScheduler::dispatch(state, "director", &action_type, ap)
}
```

（`DirectorActionScheduler::dispatch` 的真实签名以现有代码为准——参考现有 `"shop_list_item"` 分支的调用方式，`"director"` 位置传导演连接 ID；如果签名不同就照 `teammate_system_integration.rs` 里调用 `set_teammate_behavior` 的方式写。）

```rust
#[test]
fn sell_set_price_adds_entry() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    let results = director_dispatch(&mut state, json!({
        "action_type": "sell_set_price", "sell_rarity": "common", "sell_price": 2.5
    })).expect("dispatch ok");
    assert_eq!(results.results[0].message_type, MessageType::SystemNotice);
    assert_eq!(state.sell_prices.len(), 1);
    assert_eq!(state.sell_prices[0].rarity, "common");
    assert!((state.sell_prices[0].price - 2.5).abs() < 1e-9);
}

#[test]
fn sell_set_price_updates_existing_entry() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    director_dispatch(&mut state, json!({"action_type":"sell_set_price","sell_rarity":"rare","sell_price":1.0})).unwrap();
    director_dispatch(&mut state, json!({"action_type":"sell_set_price","sell_rarity":"rare","sell_price":3.0})).unwrap();
    assert_eq!(state.sell_prices.len(), 1, "同一稀有度只有一条");
    assert!((state.sell_prices[0].price - 3.0).abs() < 1e-9, "改价生效");
}

#[test]
fn sell_set_price_rejects_bad_rarity_and_price() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    for (rarity, price) in [("purple", 1.0), ("common", 0.0), ("common", -0.5), ("common", 0.3)] {
        let results = director_dispatch(&mut state, json!({
            "action_type":"sell_set_price","sell_rarity":rarity,"sell_price":price
        })).expect("ok");
        assert_eq!(results.results[0].message_type, MessageType::Info, "{}@{} 应被拒绝", rarity, price);
    }
    assert!(state.sell_prices.is_empty(), "拒绝时不产生条目");
}

#[test]
fn sell_remove_price_works() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    director_dispatch(&mut state, json!({"action_type":"sell_set_price","sell_rarity":"epic","sell_price":4.0})).unwrap();
    let results = director_dispatch(&mut state, json!({"action_type":"sell_remove_price","sell_rarity":"epic"})).unwrap();
    assert_eq!(results.results[0].message_type, MessageType::SystemNotice);
    assert!(state.sell_prices.is_empty());
    // 删除不存在的稀有度 → Info
    let results = director_dispatch(&mut state, json!({"action_type":"sell_remove_price","sell_rarity":"epic"})).unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
}
```

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test sell_system_integration`
Expected: 编译失败——字段/动作不存在。

- [ ] **Step 3: 实现调度与处理**

`director_action_scheduler.rs`：
- `DirectorActionParams` 加字段（`teammate_behavior` 字段旁）：

```rust
    /// 售出系统：稀有度与价格（0.5 步长）
    pub sell_rarity: Option<String>,
    pub sell_price: Option<f64>,
```

- `"set_teammate_behavior"` 分支后加：

```rust
            "sell_set_price" => {
                let rarity = action_params
                    .sell_rarity
                    .clone()
                    .ok_or_else(|| "Missing sell_rarity parameter".to_string())?;
                let price = action_params
                    .sell_price
                    .ok_or_else(|| "Missing sell_price parameter".to_string())?;
                game_state.handle_sell_set_price(rarity, price)
            }

            "sell_remove_price" => {
                let rarity = action_params
                    .sell_rarity
                    .clone()
                    .ok_or_else(|| "Missing sell_rarity parameter".to_string())?;
                game_state.handle_sell_remove_price(&rarity)
            }
```

`director_common_actions.rs`（`handle_set_teammate_behavior` 后面加；`SellPriceEntry` 从 models 导入）：

```rust
    /// 售出价格校验与显示辅助
    pub fn rarity_display_name(rarity: &str) -> Option<&'static str> {
        match rarity {
            "common" => Some("绿"),
            "rare" => Some("蓝"),
            "epic" => Some("紫"),
            "legendary" => Some("橙"),
            _ => None,
        }
    }

    fn format_price(price: f64) -> String {
        if (price - price.round()).abs() < 1e-9 {
            format!("{}", price.round() as i64)
        } else {
            format!("{}", price)
        }
    }

    /// 导演设置售出价格（新增或改价）
    pub fn handle_sell_set_price(
        &mut self,
        rarity: String,
        price: f64,
    ) -> Result<ActionResults, String> {
        let display = Self::rarity_display_name(&rarity).ok_or_else(|| {
            format!("稀有度必须为 common/rare/epic/legendary，当前值为 {}", rarity)
        })?;
        if price <= 0.0 || (price * 2.0 - (price * 2.0).round()).abs() >= 1e-9 {
            return Ok(ActionResult::new_info_message(
                serde_json::json!({}),
                vec![],
                format!("售出价格必须为 0.5 的倍数且大于 0，当前值为 {}", price),
                true,
            )
            .as_results());
        }

        if let Some(entry) = self.sell_prices.iter_mut().find(|e| e.rarity == rarity) {
            entry.price = price;
        } else {
            self.sell_prices.push(SellPriceEntry {
                id: uuid::Uuid::new_v4().to_string(),
                rarity,
                price,
            });
        }

        let broadcast_players: Vec<String> = self.players.keys().cloned().collect();
        let mut action_result = ActionResult::new_system_message(
            serde_json::json!({ "rarity": rarity, "price": price }),
            broadcast_players,
            format!("导演更新了售出价格：{}类道具 {} 货币", display, Self::format_price(price)),
            true,
        );
        action_result.broadcast_to_all = true;
        Ok(action_result.as_results())
    }

    /// 导演删除某稀有度的售出配置
    pub fn handle_sell_remove_price(&mut self, rarity: &str) -> Result<ActionResults, String> {
        let display = Self::rarity_display_name(rarity)
            .ok_or_else(|| format!("稀有度必须为 common/rare/epic/legendary，当前值为 {}", rarity))?;
        let Some(pos) = self.sell_prices.iter().position(|e| e.rarity == rarity) else {
            return Ok(ActionResult::new_info_message(
                serde_json::json!({}),
                vec![],
                format!("{}类道具尚未配置售出价格", display),
                true,
            )
            .as_results());
        };
        self.sell_prices.remove(pos);

        let broadcast_players: Vec<String> = self.players.keys().cloned().collect();
        let mut action_result = ActionResult::new_system_message(
            serde_json::json!({ "rarity": rarity }),
            broadcast_players,
            format!("导演关闭了{}类道具的售出", display),
            true,
        );
        action_result.broadcast_to_all = true;
        Ok(action_result.as_results())
    }
```

（`rarity_display_name`/`format_price` 若放 `impl GameState` 块外更合适就放块外作为自由函数——以编译通过、可被 `handle_sell_item_action` 调用为准。）

- [ ] **Step 4: 跑测试通过 + 回归**

Run: `cargo test --test sell_system_integration && cargo test --lib`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add backend/src/websocket/actions/director_action_scheduler.rs backend/src/websocket/actions/director_common_actions.rs backend/tests/sell_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(sell): director sell_set_price/sell_remove_price actions"
```

---

### Task 3: 玩家动作 — sell_item

**Files:**
- Modify: `backend/src/websocket/actions/player_action_scheduler.rs`
- Modify: `backend/src/websocket/actions/player_common_actions.rs`
- Test: `backend/tests/sell_system_integration.rs`（追加）

**Interfaces:**
- Consumes: Task 1 `sell_prices`/f64 coins；Task 2 `rarity_display_name`/`format_price`
- Produces: `GameState::handle_sell_item_action(&mut self, player_id: &str, item_id: &str) -> Result<ActionResults, String>`

- [ ] **Step 1: 追加失败测试**

道具构造辅助（放测试文件顶部辅助函数区；`Item`/`ItemType`/`WeaponProperties` 的真实构造以 `game_rule_engine.rs` 现有定义为准，若字段更多则给默认值）：

```rust
use royale_arena_backend::game::game_rule_engine::{Item, ItemType, WeaponProperties, ArmorProperties};

fn sell_put_weapon(state: &mut GameState, player_id: &str, item_id: &str, rarity: Option<&str>) {
    let item = Item {
        id: item_id.to_string(),
        name: format!("[W]{}", item_id),
        internal_name: Some("test_weapon".to_string()),
        rarity: rarity.map(|r| r.to_string()),
        item_type: ItemType::Weapon(WeaponProperties { damage: 5, votes: 1, uses: None, aoe_damage: None, bleed_damage: None }),
    };
    state.players.get_mut(player_id).unwrap().inventory.push(item);
}

fn sell_put_armor(state: &mut GameState, player_id: &str, item_id: &str, rarity: Option<&str>) {
    let item = Item {
        id: item_id.to_string(),
        name: format!("[A]{}", item_id),
        internal_name: Some("test_armor".to_string()),
        rarity: rarity.map(|r| r.to_string()),
        item_type: ItemType::Armor(ArmorProperties { defense: 5, votes: 1, uses: None }),
    };
    state.players.get_mut(player_id).unwrap().inventory.push(item);
}

fn sell_configure(state: &mut GameState, rarity: &str, price: f64) {
    state.handle_sell_set_price(rarity.to_string(), price).unwrap();
}
```

（`WeaponProperties`/`ArmorProperties` 的真实字段以 `game_rule_engine.rs` 为准，缺漏字段按其类型给默认值；若结构是 `Default` 可用 `..Default::default()`。）

```rust
#[test]
fn sell_item_succeeds_in_daytime() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 2.5);
    sell_put_weapon(&mut state, "p1", "w1", Some("common"));
    // 夜窗设在未来 → 当前是白天
    sell_set_night_window(&mut state, 3600, 7200);

    let results = state.handle_sell_item_action("p1", "w1").expect("sell ok");
    let p = state.players.get("p1").unwrap();
    assert!(p.inventory.iter().all(|i| i.id != "w1"), "道具已移除");
    assert!((p.coins - 2.5).abs() < 1e-9, "货币 +2.5，实际 {}", p.coins);
    assert_eq!(results.results.len(), 2, "发起方 + 导演专属");
    assert_eq!(results.results[0].message_type, MessageType::SystemNotice);
    assert_eq!(results.results[0].broadcast_players, vec!["p1".to_string()]);
    assert!(!results.results[0].broadcast_to_director);
    let director_msg = &results.results[1];
    assert!(director_msg.broadcast_players.is_empty());
    assert!(director_msg.broadcast_to_director);
    assert!(director_msg.log_message.contains("玩家一"));
    assert!(director_msg.log_message.contains("绿"));
    assert!(director_msg.log_message.contains("2.5"));
}

#[test]
fn sell_item_rejected_at_night() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.0);
    sell_put_weapon(&mut state, "p1", "w1", Some("common"));
    sell_set_night_window(&mut state, -3600, 3600); // 当前在夜内
    let results = state.handle_sell_item_action("p1", "w1").unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(results.results[0].log_message.contains("非夜间"));
    assert_eq!(state.players["p1"].inventory.len(), 1, "道具保留");
}

#[test]
fn sell_item_rejected_when_night_unset() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.0);
    sell_put_weapon(&mut state, "p1", "w1", Some("common"));
    let results = state.handle_sell_item_action("p1", "w1").unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(results.results[0].log_message.contains("尚未设置夜晚"));
}

#[test]
fn sell_item_rejects_non_weapon_armor_and_unconfigured_rarity_and_missing_item() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.0);
    // 未配置稀有度
    sell_put_armor(&mut state, "p1", "a1", Some("epic"));
    let r = state.handle_sell_item_action("p1", "a1").unwrap();
    assert_eq!(r.results[0].message_type, MessageType::Info);
    assert!(r.results[0].log_message.contains("未开放售出"));
    // rarity 为 None
    sell_put_weapon(&mut state, "p1", "w2", None);
    let r = state.handle_sell_item_action("p1", "w2").unwrap();
    assert_eq!(r.results[0].message_type, MessageType::Info);
    // 不在背包
    let r = state.handle_sell_item_action("p1", "nope").unwrap();
    assert_eq!(r.results[0].message_type, MessageType::Info);
    assert!(r.results[0].log_message.contains("不在背包"));
    // 已装备武器（不在 inventory）也不可售
    let mut w = state.players.get("p1").unwrap().inventory[1].clone();
    state.players.get_mut("p1").unwrap().equipped_weapon = Some(w.clone());
    w.id = "w2".to_string(); // inventory[1] 即 w2，上面已入包
    let r = state.handle_sell_item_action("p1", "w2").unwrap();
    assert!(state.players["p1"].inventory.len() == 2, "无状态变更");
}

#[test]
fn sell_item_rejects_non_weapon_types() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.0);
    // 用货币道具（ItemType::Currency）验证非武器/防具被拒
    use royale_arena_backend::game::game_rule_engine::CurrencyProperties;
    let item = Item {
        id: "c1".to_string(),
        name: "金币".to_string(),
        internal_name: Some("gold_coin".to_string()),
        rarity: Some("common".to_string()),
        item_type: ItemType::Currency(CurrencyProperties { value: 1 }),
    };
    state.players.get_mut("p1").unwrap().inventory.push(item);
    sell_set_night_window(&mut state, 3600, 7200);
    let r = state.handle_sell_item_action("p1", "c1").unwrap();
    assert_eq!(r.results[0].message_type, MessageType::Info);
    assert!(r.results[0].log_message.contains("武器和防具"));
}

#[test]
fn sell_item_via_scheduler_dispatch() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 0.5);
    sell_put_weapon(&mut state, "p1", "w1", Some("common"));
    sell_set_night_window(&mut state, 3600, 7200);
    use royale_arena_backend::websocket::actions::player_action_scheduler::{ActionParams, PlayerActionScheduler};
    let params = ActionParams::from_json(&json!({ "item_id": "w1" })).unwrap();
    let results = PlayerActionScheduler::dispatch(&mut state, "p1", "sell_item", params).expect("dispatch ok");
    assert!((state.players["p1"].coins - 0.5).abs() < 1e-9);
    assert_eq!(results.results.len(), 2);
}
```

（`PlayerActionScheduler::dispatch` 与 `ActionParams` 的用法参考现有 `transfer_item` 测试的调用方式。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test sell_system_integration`
Expected: 编译失败——`handle_sell_item_action` 不存在。

- [ ] **Step 3: 实现**

`player_action_scheduler.rs`：`ActionParams` 无需新字段（`item_id: Option<String>` 已存在）。`"shop_buy"` 分支后加：

```rust
            "sell_item" => {
                validate_or_return!(
                    game_state,
                    player_id,
                    vec![
                        ValidationType::Alive,
                        ValidationType::Born,
                        ValidationType::NotBound,
                    ]
                );
                let item_id = action_params
                    .item_id
                    .clone()
                    .ok_or("Missing item_id parameter")?;
                game_state.end_rest_mode_for_action(player_id);
                return game_state.handle_sell_item_action(player_id, &item_id);
            }
```

`player_common_actions.rs`（`handle_transfer_item_action` 后面加；`ItemType` 从 `crate::game::game_rule_engine` 导入，`rarity_display_name`/`format_price` 从 director_common_actions 复用或移到共享位置）：

```rust
    /// 处理售出行动：白天按稀有度价格出售背包中的武器/防具
    pub fn handle_sell_item_action(
        &mut self,
        player_id: &str,
        item_id: &str,
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

        // 1. 时间窗：夜窗已设置且当前不在夜间
        match (self.night_start_time, self.night_end_time) {
            (Some(_), Some(_)) => {
                let now = chrono::Utc::now();
                if now >= self.night_start_time.unwrap() && now <= self.night_end_time.unwrap() {
                    return Ok(info_message("售出只在非夜间行动时间可用".to_string(), player_id));
                }
            }
            _ => {
                return Ok(info_message(
                    "导演尚未设置夜晚行动时间，无法售出".to_string(),
                    player_id,
                ));
            }
        }

        // 2. 道具必须在背包（已装备道具不在 inventory，天然排除）
        let Some(item) = self
            .players
            .get(player_id)
            .ok_or("Player not found")?
            .inventory
            .iter()
            .find(|i| i.id == item_id)
            .cloned()
        else {
            return Ok(info_message("物品不在背包中".to_string(), player_id));
        };

        // 3. 只有武器和防具可售
        let is_weapon_or_armor = matches!(
            item.item_type,
            crate::game::game_rule_engine::ItemType::Weapon(_) | crate::game::game_rule_engine::ItemType::Armor(_)
        );
        if !is_weapon_or_armor {
            return Ok(info_message("只有武器和防具可以售出".to_string(), player_id));
        }

        // 4. 稀有度已配置价格
        let Some(rarity) = item.rarity.as_deref() else {
            return Ok(info_message("该道具稀有度未开放售出".to_string(), player_id));
        };
        let Some(entry) = self.sell_prices.iter().find(|e| e.rarity == rarity) else {
            return Ok(info_message("该道具稀有度未开放售出".to_string(), player_id));
        };
        let price = entry.price;

        // 应用：移除道具、加货币
        let player_name = self.players.get(player_id).unwrap().name.clone();
        let item_name = item.name.clone();
        let rarity_display = crate::websocket::actions::director_common_actions::GameState::rarity_display_name(rarity)
            .unwrap_or(rarity);

        {
            let player = self.players.get_mut(player_id).unwrap();
            player.inventory.retain(|i| i.id != item_id);
            player.coins += price;
            let coins_after = player.coins;
        }

        let price_str = crate::websocket::actions::director_common_actions::GameState::format_price(price);
        let seller_msg = format!("你售出了 {}，获得 {} 货币", item_name, price_str);
        let director_msg = format!(
            "玩家 {} 售出了 {}（{}类），获得 {} 货币",
            player_name, item_name, rarity_display, price_str
        );

        let seller_data = serde_json::json!({
            "item_name": item_name,
            "price": price,
            "coins": coins_after,
        });
        let director_data = serde_json::json!({
            "item_name": item_name,
            "player": player_name,
            "rarity": rarity,
            "price": price,
        });

        Ok(ActionResults {
            results: vec![
                ActionResult::new_system_message(
                    seller_data,
                    vec![player_id.to_string()],
                    seller_msg,
                    false,
                ),
                ActionResult::new_system_message(
                    director_data,
                    vec![],
                    director_msg,
                    true,
                ),
            ],
        })
    }
```

（`coins_after` 的作用域按编译器提示调整——把 `let coins_after` 提到块外或留在块内后在块外重新读取 `self.players.get(player_id).unwrap().coins`。`rarity_display_name`/`format_price` 若是 `impl GameState` 的关联函数，调用路径为 `GameState::rarity_display_name(...)`——以 Task 2 实际落点为准。）

- [ ] **Step 4: 跑测试 + 回归**

Run: `cargo test --test sell_system_integration && cargo test --lib && cargo test --test teammate_system_integration`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add backend/src/websocket/actions/player_action_scheduler.rs backend/src/websocket/actions/player_common_actions.rs backend/tests/sell_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(sell): player sell_item action with day-window validation"
```

---

### Task 4: 广播 JSON + 前端类型与 store 方法

**Files:**
- Modify: `backend/src/websocket/broadcaster.rs:148-173`
- Modify: `frontend/src/types/gameStateTypes.ts`
- Modify: `frontend/src/stores/gameState.ts`
- Test: `backend/tests/sell_system_integration.rs`（追加）

**Interfaces:**
- Consumes: Task 1-3 全部
- Produces: 广播 JSON `"sell_prices": [...]`；TS `SellPriceEntry { id: string; rarity: string; price: number }`；`GlobalState.sell_prices: SellPriceEntry[]`；store：`sellPrices` computed、`sellSetPrice(rarity: string, price: number)`、`sellRemovePrice(rarity: string)`、`sellItem(itemId: string)`

- [ ] **Step 1: 追加失败测试**

```rust
#[test]
fn client_json_includes_sell_prices() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    sell_configure(&mut state, "common", 1.5);
    let json = state.to_player_client_json();
    let arr = json["sell_prices"].as_array().expect("sell_prices 必须存在");
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["rarity"], "common");
    assert!((arr[0]["price"].as_f64().unwrap() - 1.5).abs() < 1e-9);
    let djson = state.to_director_client_json();
    assert!(djson["sell_prices"].as_array().unwrap().len() == 1);
}
```

（`to_player_client_json` 是 `GameState` 的方法，在 broadcaster.rs 的 `impl GameState` 块中——测试直接调用即可。）

- [ ] **Step 2: 确认失败**

Run: `cargo test --test sell_system_integration client_json`
Expected: FAIL——`sell_prices` 为 null。

- [ ] **Step 3: broadcaster.rs 两个 json! 各加一行**

`to_director_client_json` 与 `to_player_client_json` 的 `json!({...})` 里 `"shop": self.shop,` 之后各加：

```rust
            "sell_prices": self.sell_prices,
```

- [ ] **Step 4: 前端类型（gameStateTypes.ts）**

`ShopListing` 接口后加：

```ts
// 售出系统稀有度价格条目
export interface SellPriceEntry {
  id: string;
  rarity: 'common' | 'rare' | 'epic' | 'legendary';
  price: number;
}
```

`GlobalState` 的 `shop: ShopListing[];` 后加 `sell_prices: SellPriceEntry[];`（`Player.coins` 已是 number，无需改）。

- [ ] **Step 5: store 方法（gameState.ts）**

`shopListings` computed 后加：

```ts
  const sellPrices = computed<SellPriceEntry[]>(() => {
    return globalState.value?.sell_prices || []
  })
```

`shopBuy` 方法后加（并在文件底部 return 块导出 `sellPrices, sellSetPrice, sellRemovePrice, sellItem,`）：

```ts
  // 导演设置售出价格（新增或改价）
  const sellSetPrice = (rarity: string, price: number) => {
    sendDirectorAction('sell_set_price', { sell_rarity: rarity, sell_price: price })
  }

  // 导演删除售出价格
  const sellRemovePrice = (rarity: string) => {
    sendDirectorAction('sell_remove_price', { sell_rarity: rarity })
  }

  // 玩家售出道具
  const sellItem = (itemId: string) => {
    sendPlayerAction('sell_item', { item_id: itemId })
  }
```

（`SellPriceEntry` 加入文件顶部的 type 导入。）

- [ ] **Step 6: 验证**

Run: `cargo test --test sell_system_integration && cd ../frontend && pnpm build`
Expected: 后端 PASS，前端构建成功。

- [ ] **Step 7: Commit**

```bash
git add backend/src/websocket/broadcaster.rs backend/tests/sell_system_integration.rs frontend/src/types/gameStateTypes.ts frontend/src/stores/gameState.ts
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(sell): broadcast sell_prices, frontend types and store actions"
```

---

### Task 5: 导演端 — SellSystemCard.vue

**Files:**
- Create: `frontend/src/views/director/components/SellSystemCard.vue`
- Modify: `frontend/src/views/director/management/InGameManagement.vue`

**Interfaces:**
- Consumes: Task 4 的 `store.sellPrices` / `store.sellSetPrice` / `store.sellRemovePrice`
- Produces: 无（叶子组件）

- [ ] **Step 1: 创建组件**

```vue
<template>
  <el-card class="sell-system-card">
    <template #header>
      <div class="card-header">
        <h4>售出系统</h4>
        <el-button
          type="primary"
          size="small"
          :icon="Plus"
          :disabled="availableRarities.length === 0"
          @click="openAddDialog"
        >
          添加稀有度价格
        </el-button>
      </div>
    </template>

    <el-table v-if="sellPrices.length > 0" :data="sellPrices" size="small" stripe border>
      <el-table-column label="稀有度" width="120">
        <template #default="{ row }">
          <span :class="['rarity-tag', row.rarity]">{{ rarityLabel(row.rarity) }}</span>
        </template>
      </el-table-column>
      <el-table-column label="售出价（币）" width="140">
        <template #default="{ row }">{{ formatPrice(row.price) }}</template>
      </el-table-column>
      <el-table-column label="操作" width="160">
        <template #default="{ row }">
          <el-button type="primary" size="small" @click="openEditDialog(row)">改价</el-button>
          <el-button type="danger" size="small" @click="handleRemove(row.rarity)">删除</el-button>
        </template>
      </el-table-column>
    </el-table>
    <el-empty v-else description="暂未配置售出价格（玩家将无法售出道具）" :image-size="60" />

    <el-dialog
      v-model="dialogVisible"
      :title="editingRarity ? '修改售出价格' : '添加售出价格'"
      width="420px"
      :close-on-click-modal="false"
    >
      <el-form label-width="80px">
        <el-form-item label="稀有度">
          <el-select
            v-model="selectedRarity"
            :disabled="!!editingRarity"
            placeholder="选择稀有度"
            style="width: 100%"
          >
            <el-option
              v-for="r in editingRarity ? [editingRarity] : availableRarities"
              :key="r"
              :label="rarityLabel(r)"
              :value="r"
            />
          </el-select>
        </el-form-item>
        <el-form-item label="价格（币）">
          <el-input-number
            v-model="price"
            :min="0.5"
            :max="9999"
            :step="0.5"
            style="width: 100%"
          />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button type="primary" :disabled="!selectedRarity || price < 0.5" @click="handleSave">
          保存
        </el-button>
      </template>
    </el-dialog>
  </el-card>
</template>

<script setup lang="ts">
import { ref, computed } from 'vue'
import { Plus } from '@element-plus/icons-vue'
import { useGameStateStore } from '@/stores/gameState'
import type { SellPriceEntry } from '@/types/gameStateTypes'

const store = useGameStateStore()

const ALL_RARITIES = ['common', 'rare', 'epic', 'legendary'] as const

const sellPrices = computed(() => store.sellPrices)

const availableRarities = computed(() =>
  ALL_RARITIES.filter((r) => !sellPrices.value.some((e) => e.rarity === r))
)

const dialogVisible = ref(false)
const editingRarity = ref<string | null>(null)
const selectedRarity = ref('')
const price = ref(0.5)

const rarityLabel = (rarity: string) =>
  ({ common: '绿', rare: '蓝', epic: '紫', legendary: '橙' })[rarity] || rarity

const formatPrice = (p: number) => (Number.isInteger(p) ? String(p) : String(p))

const openAddDialog = () => {
  editingRarity.value = null
  selectedRarity.value = ''
  price.value = 0.5
  dialogVisible.value = true
}

const openEditDialog = (row: SellPriceEntry) => {
  editingRarity.value = row.rarity
  selectedRarity.value = row.rarity
  price.value = row.price
  dialogVisible.value = true
}

const handleSave = () => {
  if (!selectedRarity.value || price.value < 0.5) return
  store.sellSetPrice(selectedRarity.value, price.value)
  dialogVisible.value = false
}

const handleRemove = (rarity: string) => {
  store.sellRemovePrice(rarity)
}
</script>

<style scoped>
.sell-system-card {
  width: 100%;
}
.card-header {
  display: flex;
  align-items: center;
  justify-content: space-between;
  gap: 12px;
}
.card-header h4 {
  margin: 0;
  color: #606266;
  font-size: 16px;
  font-weight: 600;
}
.rarity-tag {
  display: inline-block;
  padding: 2px 8px;
  border-radius: 4px;
  color: #fff;
  font-size: 12px;
}
.rarity-tag.common { background-color: #67c23a; }
.rarity-tag.rare { background-color: #409eff; }
.rarity-tag.epic { background-color: #9b59b6; }
.rarity-tag.legendary { background-color: #e6a23c; }
</style>
```

- [ ] **Step 2: 挂载到 InGameManagement.vue**

在 `<ShopManagement />` 所在的 `full-width-section` div **之后**加：

```vue
        <!-- 售出系统 -->
        <div class="full-width-section">
          <SellSystemCard />
        </div>
```

`<script setup>` 导入区（`ShopManagement` 导入旁）加：

```ts
import SellSystemCard from '../components/SellSystemCard.vue'
```

- [ ] **Step 3: 验证构建**

Run: `cd frontend && pnpm build`
Expected: 成功。

- [ ] **Step 4: Commit**

```bash
git add frontend/src/views/director/components/SellSystemCard.vue frontend/src/views/director/management/InGameManagement.vue
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(sell): director SellSystemCard for rarity price config"
```

---

### Task 6: 演员端 — 售出按钮 + SellItemDialog.vue

**Files:**
- Create: `frontend/src/views/actor/components/SellItemDialog.vue`
- Modify: `frontend/src/views/actor/components/CompactActionPanel.vue`

**Interfaces:**
- Consumes: Task 4 的 `store.sellItem`、`store.sellPrices`；`props.player.inventory`；`globalState.night_start_time/night_end_time`
- Produces: 无（叶子组件）

- [ ] **Step 1: 创建 SellItemDialog.vue**

```vue
<template>
  <el-dialog
    :model-value="modelValue"
    @update:model-value="$emit('update:modelValue', $event)"
    title="售出道具"
    width="460px"
  >
    <div v-if="sellableItems.length === 0" class="empty-state">
      没有可售出的道具（只有已配置售出价格的武器和防具可售）
    </div>
    <el-radio-group v-else v-model="selected" class="radio-list">
      <el-radio v-for="it in sellableItems" :key="it.id" :value="it.id">
        {{ it.name }}
        <span class="price">→ {{ formatPrice(it.price) }} 币</span>
      </el-radio>
    </el-radio-group>
    <template #footer>
      <el-button @click="$emit('update:modelValue', false)">取消</el-button>
      <el-button
        type="primary"
        :disabled="!selected || sellableItems.length === 0"
        @click="handleConfirm"
      >
        确认售出
      </el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useGameStateStore } from '@/stores/gameState'
import type { Item } from '@/types/gameStateTypes'

const props = defineProps<{
  modelValue: boolean
  inventory: Item[]
}>()

const emit = defineEmits<{
  'update:modelValue': [boolean]
}>()

const store = useGameStateStore()
const selected = ref('')

interface SellableItem {
  id: string
  name: string
  price: number
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
    .map((it) => ({ id: it.id, name: it.name, price: priceOf(it.rarity) as number }))
})

const formatPrice = (p: number) => (Number.isInteger(p) ? String(p) : String(p))

watch(
  () => props.modelValue,
  (open) => {
    if (open) selected.value = ''
  }
)

const handleConfirm = () => {
  if (!selected.value) return
  store.sellItem(selected.value)
  emit('update:modelValue', false)
}
</script>

<style scoped>
.radio-list {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}
.radio-list :deep(.el-radio) {
  margin-right: 0;
}
.price {
  margin-left: 8px;
  color: #e6a23c;
  font-size: 12px;
}
.empty-state {
  color: #909399;
  text-align: center;
  padding: 16px;
}
</style>
```

- [ ] **Step 2: CompactActionPanel.vue 加按钮与挂载**

`<script setup>` 加（`isCoarsePointer` 定义旁）：

```ts
import SellItemDialog from './SellItemDialog.vue'

const nightTimesSet = computed(
  () => nightStartMs.value !== null && nightEndMs.value !== null
)
const sellAvailable = computed(
  () =>
    nightTimesSet.value &&
    !nightActionActive.value &&
    !props.player.is_bound &&
    props.player.is_alive
)
const sellDialogVisible = ref(false)
```

模板里商店 `</el-popover>` 之后（同属 `primary-actions` div 内）加：

```vue
          <el-button
            type="warning"
            size="small"
            :disabled="!sellAvailable"
            @click="sellDialogVisible = true"
          >
            售出
          </el-button>
```

模板底部（组件根元素闭合标签前）挂对话框：

```vue
  <SellItemDialog v-model="sellDialogVisible" :inventory="props.player.inventory" />
```

- [ ] **Step 3: 验证构建**

Run: `cd frontend && pnpm build`
Expected: 成功。同时静态检查：售出按钮在 `hasSpawned` 的 `v-if` 区块内（与商店同级）、`disabled` 用 `sellAvailable` 而非 `actionsDisabled`（后者夜间才可用，正好相反）。

- [ ] **Step 4: Commit**

```bash
git add frontend/src/views/actor/components/SellItemDialog.vue frontend/src/views/actor/components/CompactActionPanel.vue
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(sell): actor sell button with item picker dialog"
```

---

## Self-Review

**Spec coverage:**
- §数据模型（SellPriceEntry/sell_prices/coins f64）→ Task 1
- §导演动作（set/remove、校验、广播、中文名）→ Task 2
- §玩家动作（时间窗 4 校验、三消息、导演专属日志）→ Task 3
- §广播 JSON sell_prices → Task 4
- §前端类型与 store → Task 4
- §导演端卡片（+只列未配置稀有度、0.5 步进、改价/删除）→ Task 5
- §演员端按钮（白天可用、未设夜禁用）与对话框（只列可售、空状态）→ Task 6
- §测试计划 → 各任务内联测试 + 集成回归
- §兼容（旧存档）→ Task 1 Step 1 的 legacy 反序列化测试

**Placeholder scan:** 无 TBD/TODO。构造函数签名（GameState::new、WeaponProperties 字段、dispatch 签名）标注了"以现有代码为准 + 参考已验证的 teammate 测试文件"的适配指令——这是核实步骤，不是占位符。

**Type consistency:** `SellPriceEntry { id, rarity, price }` 前后端字段一致；`sell_set_price`/`sell_remove_price`/`sell_item` 动作名与参数名（sell_rarity/sell_price/item_id）前后端一致；`rarity_display_name`/`format_price` 在 Task 2 定义、Task 3 复用；f64/number 对应关系明确。

**Scope:** 单一计划 6 个任务，每个任务独立可测、可单独评审。
