# 商店稀有度类目改造（Shop Rarity Rework）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 武器/防具改为按稀有度类目（武器/防具 × 绿/蓝/紫/橙）定价上架，玩家购买时后端从道具库随机抽取（全局去重），杜绝场上同名称武器/防具；其他类别照旧具体上架。

**Architecture:** 扩展现有 `ShopListing`（加可选 `item_kind`/`rarity` 字段，旧存档缺省=具体物品条目）；新导演动作 `shop_list_rarity`；`shop_buy` 预创建循环按条目类型分派——具体物品走 `create_item_from_name` 原路径，稀有度条目走"候选名 − 场上已有 − 本次已抽 → 随机"的抽取（复用升级合成系统的去重模式）。

**Tech Stack:** Rust (axum, serde, uuid, rand), Vue 3 + TypeScript + Pinia + Element Plus

## Global Constraints

- 价格 i32 且 ≥1；数量 ≥1 且 ≤ 道具库中该类目 `display_names` 总数（超出 → Info "库存不能超过道具库中{稀有度中文名}类{武器|防具}的名称总数（N）"）
- `item_kind` ∈ {"weapon","armor"}；`rarity` ∈ {"common","rare","epic","legendary"}；同 `(item_kind, rarity)` 只能上架一条（重复 → Info "该类目已上架"）
- 中文名映射：common=绿、rare=蓝、epic=紫、legendary=橙（复用 `GameState::rarity_display_name`）；kind 映射：weapon=武器、armor=防具
- 稀有度条目 `item_name` 存固定展示文案：`"{绿}类{武器}（随机）"`
- 抽取去重：候选名剔除 `collect_existing_weapons_and_armor_names()`（所有玩家背包/已装备/地面）**及本次购买已抽中的名字**；候选耗尽 → Info 仅发买家 "{稀有度中文名}类{武器|防具}可抽选的名称已全部在场，购买失败"，**整笔取消零状态变更**
- `shop_list_item` 拒绝武器/防具（Info "武器和防具请按稀有度类目上架"）；消耗品/功能物品/升级道具/货币照旧；旧存档具体条目仍可购买
- 新字段 `#[serde(default, skip_serializing_if = "Option::is_none")]`；拒绝路径全部 Info（上架→导演，购买→买家）且无状态变更
- 上架成功广播全体："导演上架了{绿}类{武器}（随机），价格 {price} 货币，库存 {qty}"
- 构建：`export PATH="/c/msys64/mingw64/bin:$PATH"` 后再跑 cargo（dlltool.exe）
- 集成测试无需 DATABASE_URL；`test_game_api.rs` 需 DATABASE_URL（预先存在的限制，不修）
- 提交用 `git -c user.name="Wang Li" -c user.email="itx351@126.com" commit`

---

### Task 1: 数据模型 + 导演动作 shop_list_rarity 与具体上架限制

**Files:**
- Modify: `backend/src/websocket/models.rs:66-82`（ShopListing）
- Modify: `backend/src/websocket/actions/director_action_scheduler.rs`（params 字段 + 新 arm，插在 `"shop_delist_item"` 分支 :241 后）
- Modify: `backend/src/websocket/actions/director_common_actions.rs:750-802`（handle_shop_list_item 加限制 + 新 handle_shop_list_rarity + 辅助函数）
- Test: `backend/tests/shop_rarity_integration.rs`（新建）

**Interfaces:**
- Consumes: `GameState::rarity_display_name(rarity) -> Option<&'static str>`（售出系统已有，director_common_actions.rs）；`Item::is_weapon_or_armor()`（game_rule_engine.rs:85）
- Produces: `ShopListing` 新字段 `item_kind: Option<String>`、`rarity: Option<String>`；`GameState::handle_shop_list_rarity(&mut self, item_kind: String, rarity: String, price: i32, quantity: i32) -> Result<ActionResults, String>`；`GameState::item_kind_display_name(kind) -> Option<&'static str>`；`GameState::rarity_display_names(&self, item_kind: &str, rarity: &str) -> Vec<String>`（Task 2 购买抽取复用）
- Produces（测试辅助，Task 2 复用）: `build_shop_rarity_state()`、`shop_add_player(&mut state, id, name)`、`director_dispatch(&mut state, params)`（照抄 `backend/tests/sell_system_integration.rs` 已验证的辅助函数改名）

- [ ] **Step 1: 新建测试文件，写失败测试**

先打开 `backend/tests/sell_system_integration.rs`，把文件顶部的 `build_sell_game_state`、`sell_add_player`、`director_dispatch` 三个辅助函数**照抄**到新文件并改名（`GameState::new` / `Player::new` / `DirectorActionScheduler::dispatch` 的真实签名以那个文件里的写法为准，已验证可编译）。规则 JSON 换成下面这份（含多显示名武器/防具与一个消耗品）：

```rust
// backend/tests/shop_rarity_integration.rs（头部 imports 照抄 sell 测试 + 下面新增）
use royale_arena_backend::websocket::models::{GameState, ShopListing, MessageType};

const SHOP_RARITY_RULES: &str = r#"{
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
    "items_config": {
        "rarity_levels": [],
        "items": {
            "weapons": [
                { "internal_name": "test_sword", "display_names": ["青钢剑", "铁刃短剑"],
                  "rarity": "common", "properties": { "damage": 20, "votes": 1 } },
                { "internal_name": "test_rare_blade", "display_names": ["秘银长刃"],
                  "rarity": "rare", "properties": { "damage": 30, "votes": 1 } }
            ],
            "armors": [
                { "internal_name": "test_leather", "display_names": ["旧皮甲", "硬革胸甲"],
                  "rarity": "common", "properties": { "defense": 5, "votes": 1 } }
            ],
            "utilities": [], "upgraders": [], "currencies": [],
            "consumables": [
                { "name": "[HP10]测试药水",
                  "properties": { "effect_type": "heal", "effect_value": 10 } }
            ]
        },
        "upgrade_recipes": {}
    }
}"#;

#[test]
fn shop_list_rarity_succeeds() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    let results = director_dispatch(&mut state, json!({
        "action_type": "shop_list_rarity",
        "shop_item_kind": "weapon", "shop_rarity": "common",
        "price": 2, "quantity": 2
    })).expect("dispatch ok");
    assert_eq!(results.results[0].message_type, MessageType::SystemNotice);
    assert_eq!(state.shop.len(), 1);
    let l = &state.shop[0];
    assert_eq!(l.item_kind.as_deref(), Some("weapon"));
    assert_eq!(l.rarity.as_deref(), Some("common"));
    assert_eq!(l.item_name, "绿类武器（随机）");
    assert_eq!(l.price, 2);
    assert_eq!(l.quantity, 2);
}

#[test]
fn shop_list_rarity_rejects_invalid_inputs() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    let cases = vec![
        // (kind, rarity, price, quantity, 期望消息片段)
        ("shield", "common", 1, 1, "weapon 或 armor"),
        ("weapon", "purple", 1, 1, "common/rare/epic/legendary"),
        ("weapon", "common", 0, 1, "上架价格必须 >= 1"),
        ("weapon", "legendary", 1, 1, "道具库中没有橙类武器"),
        ("armor", "legendary", 1, 1, "道具库中没有橙类防具"),
        ("weapon", "common", 1, 3, "名称总数"),      // common 武器池只有 2 个名字
        ("armor", "common", 1, 5, "名称总数"),        // common 防具池只有 2 个名字
    ];
    for (kind, rarity, price, quantity, fragment) in cases {
        let results = director_dispatch(&mut state, json!({
            "action_type": "shop_list_rarity",
            "shop_item_kind": kind, "shop_rarity": rarity,
            "price": price, "quantity": quantity
        })).expect("dispatch ok");
        assert_eq!(results.results[0].message_type, MessageType::Info,
            "{}@{}@{}@{} 应被拒绝", kind, rarity, price, quantity);
        assert!(results.results[0].log_message.contains(fragment),
            "消息 {:?} 应包含 {:?}", results.results[0].log_message, fragment);
    }
    assert!(state.shop.is_empty(), "拒绝时不产生条目");
}

#[test]
fn shop_list_rarity_rejects_duplicate_category() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    director_dispatch(&mut state, json!({
        "action_type": "shop_list_rarity",
        "shop_item_kind": "weapon", "shop_rarity": "common", "price": 2, "quantity": 1
    })).unwrap();
    let results = director_dispatch(&mut state, json!({
        "action_type": "shop_list_rarity",
        "shop_item_kind": "weapon", "shop_rarity": "common", "price": 3, "quantity": 1
    })).unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(results.results[0].log_message.contains("该类目已上架"));
    assert_eq!(state.shop.len(), 1, "同稀有度只有一条");
    // 不同稀有度可再上架
    director_dispatch(&mut state, json!({
        "action_type": "shop_list_rarity",
        "shop_item_kind": "weapon", "shop_rarity": "rare", "price": 4, "quantity": 1
    })).unwrap();
    assert_eq!(state.shop.len(), 2);
}

#[test]
fn shop_list_item_rejects_weapon_and_armor_but_allows_consumable() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    for name in ["青钢剑", "旧皮甲"] {
        let results = director_dispatch(&mut state, json!({
            "action_type": "shop_list_item", "item_name": name, "price": 1, "quantity": 1
        })).expect("dispatch ok");
        assert_eq!(results.results[0].message_type, MessageType::Info, "{} 应被拒绝", name);
        assert!(results.results[0].log_message.contains("按稀有度类目上架"));
    }
    assert!(state.shop.is_empty());
    let results = director_dispatch(&mut state, json!({
        "action_type": "shop_list_item", "item_name": "[HP10]测试药水", "price": 1, "quantity": 5
    })).unwrap();
    assert_eq!(results.results[0].message_type, MessageType::SystemNotice);
    assert_eq!(state.shop.len(), 1);
}

#[test]
fn shop_listing_deserializes_without_new_fields() {
    let l: ShopListing = serde_json::from_str(
        r#"{"id":"l1","item_name":"青钢剑","price":2,"quantity":1}"#,
    )
    .expect("旧存档条目必须能反序列化");
    assert!(l.item_kind.is_none());
    assert!(l.rarity.is_none());
}

#[test]
fn client_json_carries_rarity_listing_fields() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    director_dispatch(&mut state, json!({
        "action_type": "shop_list_rarity",
        "shop_item_kind": "armor", "shop_rarity": "common", "price": 1, "quantity": 2
    })).unwrap();
    let json = state.to_player_client_json();
    let arr = json["shop"].as_array().expect("shop 数组存在");
    assert_eq!(arr[0]["item_kind"], "weapon".replace("weapon", "armor"));
    assert_eq!(arr[0]["rarity"], "common");
    // 具体物品条目不携带新字段（skip_serializing_if）
    director_dispatch(&mut state, json!({
        "action_type": "shop_list_item", "item_name": "[HP10]测试药水", "price": 1, "quantity": 1
    })).unwrap();
    let json = state.to_player_client_json();
    let exact = json["shop"].as_array().unwrap().iter()
        .find(|e| e["item_name"] == "[HP10]测试药水").unwrap();
    assert!(exact.get("item_kind").is_none());
    assert!(exact.get("rarity").is_none());
}
```

（`to_player_client_json`、`MessageType`、`director_dispatch` 的用法与 sell 测试一致；若 `MessageType` 在 sell 文件里从别处导入，照抄。）

- [ ] **Step 2: 跑测试确认失败**

Run: `export PATH="/c/msys64/mingw64/bin:$PATH" && cd backend && cargo test --test shop_rarity_integration`
Expected: 编译失败——`item_kind`/`rarity` 字段不存在、`handle_shop_list_rarity` 不存在。

- [ ] **Step 3: models.rs 扩展 ShopListing**

`backend/src/websocket/models.rs:66-82` 改为：

```rust
/// 商店上架物品结构
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct ShopListing {
    /// 上架条目唯一ID
    pub id: String,
    /// 物品名称（来自规则配置；稀有度条目存展示文案如"绿类武器（随机）"）
    pub item_name: String,
    /// 价格（货币数，单价）
    pub price: i32,
    /// 库存数量
    #[serde(default = "default_quantity")]
    pub quantity: i32,
    /// 稀有度随机条目：Some("weapon" | "armor")；具体物品条目为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item_kind: Option<String>,
    /// 与 item_kind 同时出现的稀有度（common | rare | epic | legendary）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rarity: Option<String>,
}
```

- [ ] **Step 4: director_common_actions.rs 加辅助函数与 handler**

在 `rarity_display_name`（售出系统加的）旁边加：

```rust
    /// 商店稀有度类目：类别中文名
    pub fn item_kind_display_name(kind: &str) -> Option<&'static str> {
        match kind {
            "weapon" => Some("武器"),
            "armor" => Some("防具"),
            _ => None,
        }
    }

    /// 道具库中该类别该稀有度的全部候选显示名（多条目合并）
    pub fn rarity_display_names(&self, item_kind: &str, rarity: &str) -> Vec<String> {
        match item_kind {
            "weapon" => self
                .rule_engine
                .items_config
                .items
                .weapons
                .iter()
                .filter(|w| w.rarity.as_deref() == Some(rarity))
                .flat_map(|w| w.display_names.clone())
                .collect(),
            "armor" => self
                .rule_engine
                .items_config
                .items
                .armors
                .iter()
                .filter(|a| a.rarity.as_deref() == Some(rarity))
                .flat_map(|a| a.display_names.clone())
                .collect(),
            _ => Vec::new(),
        }
    }
```

`handle_shop_list_item`（:769-772）中原来只做校验的那段：

```rust
        // 验证物品名称是否存在于规则配置中
        self.rule_engine
            .create_item_from_name(&item_name)
            .map_err(|err| format!("物品 {} 不存在于规则配置中: {}", item_name, err))?;
```

改为（创建预览并拒绝武器/防具）：

```rust
        // 验证物品名称是否存在于规则配置中
        let preview = self
            .rule_engine
            .create_item_from_name(&item_name)
            .map_err(|err| format!("物品 {} 不存在于规则配置中: {}", item_name, err))?;
        if preview.is_weapon_or_armor() {
            let data = serde_json::json!({});
            return Ok(ActionResult::new_info_message(
                data,
                vec![],
                "武器和防具请按稀有度类目上架".to_string(),
                true,
            )
            .as_results());
        }
```

`handle_shop_list_item` 之后新增：

```rust
    /// 商店上架稀有度类目（武器/防具 × 稀有度，购买时随机抽取）
    pub fn handle_shop_list_rarity(
        &mut self,
        item_kind: String,
        rarity: String,
        price: i32,
        quantity: i32,
    ) -> Result<ActionResults, String> {
        let info = |message: String| -> ActionResults {
            ActionResult::new_info_message(serde_json::json!({}), vec![], message, true).as_results()
        };
        let Some(kind_cn) = Self::item_kind_display_name(&item_kind) else {
            return Ok(info(format!(
                "类目必须为 weapon 或 armor，当前值为 {}",
                item_kind
            )));
        };
        let Some(rarity_cn) = Self::rarity_display_name(&rarity) else {
            return Ok(info(format!(
                "稀有度必须为 common/rare/epic/legendary，当前值为 {}",
                rarity
            )));
        };

        if price < 1 {
            return Ok(info(format!("上架价格必须 >= 1，当前值为 {}", price)));
        }

        let pool = self.rarity_display_names(&item_kind, &rarity);
        if pool.is_empty() {
            return Ok(info(format!("道具库中没有{}类{}", rarity_cn, kind_cn)));
        }

        let qty = quantity.max(1);
        if qty > pool.len() as i32 {
            return Ok(info(format!(
                "库存不能超过道具库中{}类{}的名称总数（{}）",
                rarity_cn,
                kind_cn,
                pool.len()
            )));
        }

        if self
            .shop
            .iter()
            .any(|l| l.item_kind.as_deref() == Some(item_kind.as_str())
                && l.rarity.as_deref() == Some(rarity.as_str()))
        {
            return Ok(info("该类目已上架".to_string()));
        }

        let listing = ShopListing {
            id: uuid::Uuid::new_v4().to_string(),
            item_name: format!("{}类{}（随机）", rarity_cn, kind_cn),
            price,
            quantity: qty,
            item_kind: Some(item_kind),
            rarity: Some(rarity),
        };
        self.shop.push(listing.clone());

        let data = serde_json::json!({ "shop_listing": listing });
        let broadcast_players: Vec<String> = self.players.keys().cloned().collect();
        let mut action_result = ActionResult::new_system_message(
            data,
            broadcast_players,
            format!(
                "导演上架了{}类{}（随机），价格 {} 货币，库存 {}",
                rarity_cn, kind_cn, price, qty
            ),
            true,
        );
        action_result.broadcast_to_all = true;

        Ok(action_result.as_results())
    }
```

- [ ] **Step 5: director_action_scheduler.rs 加参数与 arm**

`DirectorActionParams` 的 `sell_rarity`/`sell_price` 字段旁加：

```rust
    /// 商店稀有度类目上架：类别与稀有度
    pub shop_item_kind: Option<String>,
    pub shop_rarity: Option<String>,
```

`"shop_delist_item"` 分支（:236-241）后加：

```rust
            "shop_list_rarity" => {
                let item_kind = action_params
                    .shop_item_kind
                    .clone()
                    .ok_or_else(|| "Missing shop_item_kind parameter".to_string())?;
                let rarity = action_params
                    .shop_rarity
                    .clone()
                    .ok_or_else(|| "Missing shop_rarity parameter".to_string())?;
                let price = action_params
                    .price
                    .ok_or_else(|| "Missing price parameter".to_string())?;
                let quantity = action_params.quantity.unwrap_or(1);
                game_state.handle_shop_list_rarity(item_kind, rarity, price, quantity)
            }
```

- [ ] **Step 6: 编译驱动的字段适配**

`grep -rn "ShopListing {" backend/src backend/tests` 找出所有结构体字面量（`handle_shop_list_item` 的构造已在上面补了；`backend/tests/currency_and_movement_integration.rs` 的 `make_shop_listing` 等测试构造点），每处补 `item_kind: None,` 与 `rarity: None,`。`grep -rn "DirectorActionParams {" backend/tests` 同样补 `shop_item_kind: None, shop_rarity: None,`。

- [ ] **Step 7: 跑测试 + 回归**

Run: `cargo test --test shop_rarity_integration && cargo test --lib && cargo test --test sell_system_integration && cargo test --test teammate_system_integration`
Expected: 全部 PASS。

- [ ] **Step 8: Commit**

```bash
git add backend/src/websocket/models.rs backend/src/websocket/actions/director_action_scheduler.rs backend/src/websocket/actions/director_common_actions.rs backend/tests/shop_rarity_integration.rs backend/tests/currency_and_movement_integration.rs backend/tests/teammate_system_integration.rs backend/tests/sell_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(shop): rarity-class listings with pool validation, reject exact weapon/armor listing"
```

---

### Task 2: 玩家购买随机抽取

**Files:**
- Modify: `backend/src/websocket/actions/player_common_actions.rs:681-800`（purchase_plan 扩展 + 预创建循环分派）
- Test: `backend/tests/shop_rarity_integration.rs`（追加）

**Interfaces:**
- Consumes: Task 1 的 `rarity_display_names`/`item_kind_display_name`/`rarity_display_name`；`collect_existing_weapons_and_armor_names()`（game_state_common.rs:565）；`create_item_from_name`；测试辅助 `build_shop_rarity_state`/`shop_add_player`/`director_dispatch`
- Produces: 无新接口（`handle_shop_buy_action` 行为扩展）

- [ ] **Step 1: 追加失败测试**

测试文件追加（imports 补 `ShopBuyItem`；道具构造参照 sell 测试里 `sell_put_weapon` 的写法，`WeaponProperties` 等从 `royale_arena_backend::game::game_rule_engine` 导入）：

```rust
use royale_arena_backend::websocket::models::ShopBuyItem;

fn buy(state: &mut GameState, player_id: &str, listing_id: &str, qty: i32) -> royale_arena_backend::websocket::models::ActionResults {
    state
        .handle_shop_buy_action(player_id, &[ShopBuyItem { listing_id: listing_id.to_string(), quantity: qty }])
        .expect("buy dispatch ok")
}

fn list_rarity(state: &mut GameState, kind: &str, rarity: &str, price: i32, qty: i32) -> String {
    director_dispatch(state, json!({
        "action_type": "shop_list_rarity",
        "shop_item_kind": kind, "shop_rarity": rarity, "price": price, "quantity": qty
    })).unwrap();
    state.shop.iter().find(|l| l.item_kind.as_deref() == Some(kind)).unwrap().id.clone()
}

fn put_item_on_player(state: &mut GameState, player_id: &str, name: &str) {
    // 占用名字：任意武器道具，名字即关键（去重只看名字）
    let item = state
        .rule_engine_placeholder_none
        ;
    let _ = item;
    todo!("由实现者按 sell_put_weapon 模式实现：create_item_from_name(name) 后 push 进背包")
}

#[test]
fn buy_rarity_listing_success() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    state.players.get_mut("p1").unwrap().coins = 10.0;
    let listing_id = list_rarity(&mut state, "weapon", "common", 2, 2);

    let results = buy(&mut state, "p1", &listing_id, 1);
    let p = state.players.get("p1").unwrap();
    assert_eq!(p.inventory.len(), 1);
    let item = &p.inventory[0];
    assert!(matches!(item.item_type, royale_arena_backend::game::game_rule_engine::ItemType::Weapon(_)));
    assert_eq!(item.rarity.as_deref(), Some("common"));
    assert!(["青钢剑", "铁刃短剑"].contains(&item.name.as_str()));
    assert!((p.coins - 8.0).abs() < 1e-9, "扣款 2，实际 {}", p.coins);
    assert_eq!(state.shop[0].quantity, 1, "库存减一");
    assert!(results.results[0].log_message.contains("从商店购买"));
}

#[test]
fn buy_multiple_units_draw_distinct_names() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    state.players.get_mut("p1").unwrap().coins = 10.0;
    let listing_id = list_rarity(&mut state, "weapon", "common", 2, 2);

    let _ = buy(&mut state, "p1", &listing_id, 2);
    let p = state.players.get("p1").unwrap();
    assert_eq!(p.inventory.len(), 2);
    assert_ne!(p.inventory[0].name, p.inventory[1].name, "同批购买名字互不相同");
    assert!((p.coins - 6.0).abs() < 1e-9);
    assert!(state.shop.is_empty(), "库存归零自动移除");
}

#[test]
fn buy_fails_atomically_when_pool_exhausted() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    state.players.get_mut("p1").unwrap().coins = 10.0;
    // 占满 common 武器池的两个名字
    for name in ["青钢剑", "铁刃短剑"] {
        let item = build_weapon_by_name(&mut state, name);
        state.players.get_mut("p1").unwrap().inventory.push(item);
    }
    let listing_id = list_rarity(&mut state, "weapon", "common", 2, 1);

    let results = buy(&mut state, "p1", &listing_id, 1);
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(results.results[0].log_message.contains("已全部在场"));
    let p = state.players.get("p1").unwrap();
    assert_eq!(p.inventory.len(), 2, "无状态变更");
    assert!((p.coins - 10.0).abs() < 1e-9);
    assert_eq!(state.shop[0].quantity, 1, "库存不变");
}

#[test]
fn mixed_exact_and_rarity_purchase() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    state.players.get_mut("p1").unwrap().coins = 10.0;
    director_dispatch(&mut state, json!({
        "action_type": "shop_list_item", "item_name": "[HP10]测试药水", "price": 1, "quantity": 3
    })).unwrap();
    let rarity_id = list_rarity(&mut state, "armor", "common", 2, 1);
    let exact_id = state.shop.iter().find(|l| l.item_kind.is_none()).unwrap().id.clone();

    let _ = buy2(&mut state, "p1", &[(rarity_id, 1), (exact_id, 2)]);
    let p = state.players.get("p1").unwrap();
    assert_eq!(p.inventory.len(), 3, "1 件随机防具 + 2 瓶药水");
    assert_eq!(p.inventory.iter().filter(|i| matches!(i.item_type, royale_arena_backend::game::game_rule_engine::ItemType::Armor(_))).count(), 1);
    assert_eq!(p.inventory.iter().filter(|i| i.name == "[HP10]测试药水").count(), 2);
    assert!((p.coins - 5.0).abs() < 1e-9, "2 + 1×2 = 4，实际 {}", p.coins);
}

#[test]
fn legacy_exact_weapon_listing_still_buyable() {
    let mut state = build_shop_rarity_state();
    shop_add_player(&mut state, "p1", "玩家一");
    state.players.get_mut("p1").unwrap().coins = 5.0;
    state.shop.push(ShopListing {
        id: "legacy-w1".to_string(),
        item_name: "青钢剑".to_string(),
        price: 1,
        quantity: 1,
        item_kind: None,
        rarity: None,
    });
    let _ = buy(&mut state, "p1", "legacy-w1", 1);
    let p = state.players.get("p1").unwrap();
    assert_eq!(p.inventory.len(), 1);
    assert_eq!(p.inventory[0].name, "青钢剑");
    assert!((p.coins - 4.0).abs() < 1e-9);
}

fn buy2(state: &mut GameState, player_id: &str, items: &[(String, i32)]) -> royale_arena_backend::websocket::models::ActionResults {
    let buys: Vec<ShopBuyItem> = items
        .iter()
        .map(|(id, q)| ShopBuyItem { listing_id: id.clone(), quantity: *q })
        .collect();
    state.handle_shop_buy_action(player_id, &buys).expect("buy ok")
}
```

辅助（放文件辅助区，替换上面 `put_item_on_player` 占位——测试里实际用的是这个）：

```rust
fn build_weapon_by_name(state: &GameState, name: &str) -> royale_arena_backend::game::game_rule_engine::Item {
    // 直接用规则引擎按名字创建（测试规则里武器名都在道具库中）
    state.rule_engine_create_item_fallback(name)
}
```

（注：`GameState` 上访问 `rule_engine` 若为私有，改用 `GameRuleEngine::from_json(&state.rules_config.to_string()).unwrap().create_item_from_name(name).unwrap()`——与 sell 测试构建 engine 的方式一致。实现时二选一，删掉另一个。）

- [ ] **Step 2: 跑测试确认失败**

Run: `cargo test --test shop_rarity_integration`
Expected: 行为失败——随机条目按 `item_name`（"绿类武器（随机）"）去 create_item_from_name 报"物品不存在"或创建失败，交易取消。

- [ ] **Step 3: 实现购买分派**

`player_common_actions.rs`：

1. purchase_plan（:682）类型从 `Vec<(String, String, i32, i32)>` 改为 `Vec<(String, String, i32, i32, Option<(String, String)>)>`；:740-745 的 push 改为：

```rust
            purchase_plan.push((
                listing.id.clone(),
                listing.item_name.clone(),
                listing.price,
                buy_qty,
                listing.item_kind.clone().zip(listing.rarity.clone()),
            ));
```

2. 预创建循环（约 :783-800，`// 预先创建所有物品（原子性检查）` 注释处）改为：

```rust
        // 预先创建所有物品（原子性检查），任何一个失败则中止整笔交易
        let player_name = self.players.get(player_id).unwrap().name.clone();
        let mut created_items = Vec::new();
        let existing_names = self.collect_existing_weapons_and_armor_names();
        let mut drawn_names: std::collections::HashSet<String> = std::collections::HashSet::new();
        for (_id, item_name, _price, qty, kind_rarity) in &purchase_plan {
            for _ in 0..*qty {
                let item = match kind_rarity {
                    None => self.rule_engine.create_item_from_name(item_name).map_err(|err| {
                        format!("创建物品 {} 失败，交易取消: {}", item_name, err)
                    })?,
                    Some((kind, rarity)) => {
                        let candidates = self.rarity_display_names(kind, rarity);
                        let available: Vec<&String> = candidates
                            .iter()
                            .filter(|n| !existing_names.contains(*n) && !drawn_names.contains(*n))
                            .collect();
                        if available.is_empty() {
                            let kind_cn = Self::item_kind_display_name(kind).unwrap_or(kind.as_str());
                            let rarity_cn = Self::rarity_display_name(rarity).unwrap_or(rarity.as_str());
                            return Ok(info_message(format!(
                                "{}类{}可抽选的名称已全部在场，购买失败",
                                rarity_cn, kind_cn
                            )));
                        }
                        let mut rng = rand::rng();
                        let name = available[rng.random_range(0..available.len())].clone();
                        drawn_names.insert(name.clone());
                        self.rule_engine.create_item_from_name(&name).map_err(|err| {
                            format!("创建物品 {} 失败，交易取消: {}", name, err)
                        })?
                    }
                };
                created_items.push(item);
            }
        }
```

（`rand::rng()`/`random_range` 的用法照抄 `player_upgrade_action.rs:137-140`；若该文件顶部有 `use rand::...` 导入则照抄。原有循环里创建失败分支返回 `Ok(info_message(...))` 的写法若与上面 `?` 不一致，保持与原代码一致的成功路径、把 Err 分支照原样返回 Info——以编译通过且原子性不变为准。）

3. 若后续 `for (_id, item_name, _price, qty) in &purchase_plan` 之外还有对 plan 元组的解构使用（grep `purchase_plan`），同步补第 5 个元素。

- [ ] **Step 4: 跑测试 + 回归**

Run: `cargo test --test shop_rarity_integration && cargo test --lib && cargo test --test sell_system_integration && cargo test --test teammate_system_integration && cargo test --test currency_and_movement_integration`
Expected: 全部 PASS。

- [ ] **Step 5: Commit**

```bash
git add backend/src/websocket/actions/player_common_actions.rs backend/tests/shop_rarity_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(shop): random draw with global name dedup for rarity purchases"
```

---

### Task 3: 前端 — 类型/store/导演上架对话框/演员列表色标

**Files:**
- Modify: `frontend/src/types/gameStateTypes.ts:4-9`（ShopListing）
- Modify: `frontend/src/stores/gameState.ts:327-334`（shopListItem 旁加 shopListRarity + return 导出）
- Modify: `frontend/src/views/director/components/ShopManagement.vue`（上架对话框改造 + 列表色标）
- Modify: `frontend/src/views/actor/components/CompactActionPanel.vue:160-161`（商店行色标）

**Interfaces:**
- Consumes: Task 1/2 的后端动作 `shop_list_rarity`（参数 `shop_item_kind`/`shop_rarity`/`price`/`quantity`）
- Produces: `store.shopListRarity(itemKind: 'weapon' | 'armor', rarity: string, price: number, quantity?: number)`；TS `ShopListing.item_kind?: 'weapon' | 'armor'; rarity?: 'common' | 'rare' | 'epic' | 'legendary'`

- [ ] **Step 1: TS 类型（gameStateTypes.ts）**

```ts
export interface ShopListing {
  id: string
  item_name: string
  price: number
  quantity: number
  item_kind?: 'weapon' | 'armor'
  rarity?: 'common' | 'rare' | 'epic' | 'legendary'
}
```

- [ ] **Step 2: store 方法（gameState.ts）**

`shopListItem` 后加，并加入文件底部 return 块：

```ts
  // 商店上架稀有度类目（武器/防具随机）
  const shopListRarity = (
    itemKind: 'weapon' | 'armor',
    rarity: string,
    price: number,
    quantity: number = 1
  ) => {
    sendDirectorAction('shop_list_rarity', {
      shop_item_kind: itemKind,
      shop_rarity: rarity,
      price,
      quantity
    })
  }
```

- [ ] **Step 3: ShopManagement.vue 改造**

`<script setup>` 新增/修改（保留 `parsedItems`、`shopListings`、`handleDelist` 不变；`itemGroups` **删掉武器/防具两个分组**，只剩功能道具/消耗品/货币/升级器）：

```ts
type ListMode = 'weapon' | 'armor' | 'exact'

const listMode = ref<ListMode>('exact')
const selectedRarity = ref('')

const ALL_RARITIES = ['common', 'rare', 'epic', 'legendary'] as const
const rarityLabel = (r: string) =>
  ({ common: '绿', rare: '蓝', epic: '紫', legendary: '橙' })[r] || r

const rarityPool = computed(() =>
  listMode.value === 'weapon'
    ? parsedItems.value?.rarityItems.weapons
    : parsedItems.value?.rarityItems.armors
)

// 只列未上架且道具库非空的稀有度
const rarityOptions = computed(() =>
  ALL_RARITIES.map((r) => ({ rarity: r, size: (rarityPool.value?.[r] || []).length })).filter(
    (o) =>
      o.size > 0 &&
      !shopListings.value.some((l) => l.item_kind === listMode.value && l.rarity === o.rarity)
  )
)

const maxQuantity = computed(() => {
  if (listMode.value === 'exact') return 999
  const size = (rarityPool.value?.[selectedRarity.value] || []).length
  return size > 0 ? size : 1
})

const switchMode = (mode: ListMode) => {
  listMode.value = mode
  selectedRarity.value = ''
  selectedItem.value = ''
  quantity.value = 1
}

const handleListItem = () => {
  if (price.value < 1 || quantity.value < 1) return
  if (listMode.value === 'exact') {
    if (!selectedItem.value) return
    store.shopListItem(selectedItem.value, price.value, quantity.value)
  } else {
    if (!selectedRarity.value) return
    store.shopListRarity(listMode.value, selectedRarity.value, price.value, quantity.value)
  }
  dialogVisible.value = false
}
```

对话框模板改为（"上架类型"优先，武器/防具模式显示稀有度选择器，数量上限联动）：

```vue
      <el-form label-width="80px">
        <el-form-item label="上架类型">
          <el-select :model-value="listMode" style="width: 100%" @update:model-value="switchMode($event as ListMode)">
            <el-option label="具体物品（消耗品等）" value="exact" />
            <el-option label="武器（按稀有度随机）" value="weapon" />
            <el-option label="防具（按稀有度随机）" value="armor" />
          </el-select>
        </el-form-item>
        <el-form-item v-if="listMode === 'exact'" label="物品">
          <el-select v-model="selectedItem" placeholder="选择物品" filterable style="width: 100%">
            <el-option-group v-for="group in itemGroups" :key="group.label" :label="group.label">
              <el-option v-for="name in group.items" :key="name" :label="name" :value="name" />
            </el-option-group>
          </el-select>
        </el-form-item>
        <el-form-item v-else label="稀有度">
          <el-select v-model="selectedRarity" placeholder="选择稀有度" style="width: 100%">
            <el-option
              v-for="o in rarityOptions"
              :key="o.rarity"
              :label="`${rarityLabel(o.rarity)}（可抽 ${o.size} 件）`"
              :value="o.rarity"
            />
          </el-select>
        </el-form-item>
        <el-form-item label="单价">
          <el-input-number v-model="price" :min="1" :max="9999" style="width: 100%" />
        </el-form-item>
        <el-form-item label="数量">
          <el-input-number v-model="quantity" :min="1" :max="maxQuantity" style="width: 100%" />
        </el-form-item>
      </el-form>
      <template #footer>
        <el-button @click="dialogVisible = false">取消</el-button>
        <el-button
          type="primary"
          :disabled="!canSubmit"
          @click="handleListItem"
        >
          上架
        </el-button>
      </template>
```

`canSubmit` computed：

```ts
const canSubmit = computed(() => {
  if (price.value < 1 || quantity.value < 1 || quantity.value > maxQuantity.value) return false
  return listMode.value === 'exact' ? !!selectedItem.value : !!selectedRarity.value
})
```

列表"物品名称"列加色标：

```vue
      <el-table-column label="物品名称">
        <template #default="{ row }">
          <span v-if="row.rarity" :class="['rarity-dot', row.rarity]"></span>
          {{ row.item_name }}
        </template>
      </el-table-column>
```

scoped style 追加：

```css
.rarity-dot {
  display: inline-block;
  width: 8px;
  height: 8px;
  border-radius: 50%;
  margin-right: 6px;
  vertical-align: middle;
}
.rarity-dot.common { background-color: #67c23a; }
.rarity-dot.rare { background-color: #409eff; }
.rarity-dot.epic { background-color: #9b59b6; }
.rarity-dot.legendary { background-color: #e6a23c; }
```

- [ ] **Step 4: CompactActionPanel.vue 商店行色标**

商店 popover 的行（:160-161）：

```vue
                    <span class="shop-item-name">
                      <span v-if="listing.rarity" :class="['rarity-dot', listing.rarity]"></span>
                      {{ listing.item_name }}
                    </span>
```

scoped style 追加同上 4 色的 `.rarity-dot`（尺寸 8px 圆点，margin-right 6px）。

- [ ] **Step 5: 验证构建**

Run: `cd frontend && pnpm build`
Expected: 成功（仅预先存在的 chunk 大小警告）。静态检查：itemGroups 不再包含武器/防具；上架按钮 disabled 由 `canSubmit` 控制；数量上限在稀有度模式为池大小。

- [ ] **Step 6: Commit**

```bash
git add frontend/src/types/gameStateTypes.ts frontend/src/stores/gameState.ts frontend/src/views/director/components/ShopManagement.vue frontend/src/views/actor/components/CompactActionPanel.vue
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(shop): rarity-class listing UI with pool-aware quantity cap"
```

---

## Self-Review

**Spec coverage:**
- §数据模型（ShopListing 扩展、缺省=具体条目）→ Task 1 Step 3 + 反序列化测试
- §导演动作（shop_list_rarity 六项校验、文案、广播；shop_list_item 拒绝武器/防具；delist 不变）→ Task 1
- §玩家购买（分派、全局去重+本次去重、耗尽原子取消、库存扣减自动移除、旧条目兼容）→ Task 2
- §广播与前端类型 → Task 1（client_json 测试）+ Task 3 Step 1/2
- §导演端 UI（类别优先、稀有度只列未上架且非空、数量上限=池大小、色标）→ Task 3 Step 3
- §演员端（色标；结构不变无新动作）→ Task 3 Step 4
- §错误处理汇总表 → 各校验分支与测试一一对应
- §测试计划 → 三任务内联测试 + 回归

**Placeholder scan:** Task 2 Step 1 的 `put_item_on_player`/`build_weapon_by_name` 两处给出了占位与两条明确落地路径（rule_engine 可达或经 GameRuleEngine::from_json），实现者二选一后删除另一个——这是有界的核实步骤，不是空洞占位；其余步骤全部为具体代码。

**Type consistency:** `item_kind`/`rarity` 字段名前后端一致；`shop_item_kind`/`shop_rarity`/`price`/`quantity` 参数名与 DirectorActionParams 一致；`rarity_display_names`/`item_kind_display_name` Task 1 定义、Task 2 复用；purchase_plan 5 元组在 Step 3 内部自洽（解构同步改）。

**Scope:** 单一计划 3 个任务，各自独立可测、可评审。
