# 永久增益道具类别实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增第 7 类道具"永久增益"（`permanent_buff`），使用后永久提升血量上限 / 体力上限 / 背包容量，受规则配置的硬上限约束。

**Architecture:** 完全仿照消耗品（血包）管道：规则引擎新增 `PermanentBuffProperties` + `ItemType::PermanentBuff` 变体 + `permanent_buffs` 配置数组；使用逻辑独立为 `handle_permanent_buff_use`；背包容量从全局规则改为玩家字段 `Player.max_backpack_items`（5 处引用点切换）；`PlayerConfig` 新增三个带默认值的 cap 字段；前端解析/校验/UI 各加一个类别。

**Tech Stack:** Rust (axum 后端，serde)，Vue 3 + TypeScript + Element Plus (前端)，集成测试用 `cargo test`。

**规格文档:** `docs/superpowers/specs/2026-08-25-permanent-buff-items-design.md`

## Global Constraints

- 提交身份（本仓库未配置 git user，必须用一次性参数）：
  `git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "..."`
- 后端目录 `backend/`，运行测试：`cargo test --test <测试文件名> [测试名]`；全量：`cargo test`
- 前端目录 `frontend/`，构建+类型检查：`pnpm build`（等价 `vue-tsc -b && vite build`）
- serde 标签格式：`{"type": "permanent_buff", "properties": {"effect_type": "...", "effect_value": N}}`（枚举已有 `#[serde(tag = "type", content = "properties", rename_all = "snake_case")]`，变体名 `PermanentBuff` 自动序列化为 `permanent_buff`）
- effect_type 合法值：`"max_life"` | `"max_strength"` | `"max_backpack"`
- 三档数值：血上限 +20/+50/+100，体力上限 +20/+50/+100，背包 +2/+4/+6
- cap 默认值：`max_life_cap` 300、`max_strength_cap` 300、`max_backpack_items_cap` 12（serde default，旧配置兼容）
- 只涨上限不动当前值；溢出浪费（clamp 到 `max(cap, 规则基础值)`）；道具用后销毁；体力走现有 `action_costs.use_item` 结算（默认 0）
- 9 个默认道具名（写死进三份配置）：[HP上限+20]养生丸 / [HP上限+50]壮骨丹 / [HP上限+100]金钟罩 / [MP上限+20]干粮 / [MP上限+50]行军丹 / [MP上限+100]龙力丸 / [背包+2]腰包 / [背包+4]行囊 / [背包+6]百宝袋
- 不设稀有度（与血包一致；`rarity` 保留为可选字段）
- 注释风格遵循现有文件：中文 doc 注释

---

### Task 1: 后端规则引擎——新类别类型定义与解析

**Files:**
- Modify: `backend/src/game/game_rule_engine.rs`
- Test: `backend/tests/game_rule_engine_integration.rs`

**Interfaces:**
- Consumes: 现有 `ItemsByCategory`、`ItemType`、`create_item_from_name`
- Produces（后续任务依赖的确切签名）:
  - `pub struct PermanentBuffProperties { pub effect_type: String, pub effect_value: i32 }`
  - `pub struct PermanentBuffConfig { pub name: String, pub internal_name: Option<String>, pub rarity: Option<String>, pub properties: PermanentBuffProperties }`
  - `ItemType::PermanentBuff(PermanentBuffProperties)` 变体
  - `ItemsByCategory.permanent_buffs: Vec<PermanentBuffConfig>`（`#[serde(default)]`）

- [ ] **Step 1: 写失败测试**

在 `backend/tests/game_rule_engine_integration.rs` 末尾追加两个测试。先确认文件顶部 use 语句包含 `ItemType`，若为 `use royale_arena_backend::game::game_rule_engine::GameRuleEngine;` 则改为 `use royale_arena_backend::game::game_rule_engine::{GameRuleEngine, ItemType};`：

```rust
/// 测试：永久增益道具配置解析与物品创建
#[test]
fn test_permanent_buff_config_parsing() {
    let rules_json = r#"{
        "map": {"places": ["loc"], "safe_places": []},
        "player": {"max_life": 100, "max_strength": 100, "daily_life_recovery": 0, "daily_strength_recovery": 40, "search_cooldown": 30, "max_backpack_items": 6, "unarmed_damage": 5},
        "action_costs": {"move": 5, "search": 5, "pick": 0, "attack": 0, "equip": 0, "use": 0, "throw": 0, "deliver": 10},
        "rest_mode": {"life_recovery": 25, "strength_recovery": 1000, "max_moves": 1},
        "death_item_disposition": "killer_takes_loot",
        "teammate_behavior": 0,
        "items_config": {
            "rarity_levels": [],
            "items": {
                "permanent_buffs": [
                    {"name": "[HP上限+20]养生丸", "properties": {"effect_type": "max_life", "effect_value": 20}},
                    {"name": "[MP上限+50]行军丹", "properties": {"effect_type": "max_strength", "effect_value": 50}},
                    {"name": "[背包+6]百宝袋", "properties": {"effect_type": "max_backpack", "effect_value": 6}}
                ]
            },
            "upgrade_recipes": {}
        }
    }"#;
    let rule_engine =
        GameRuleEngine::from_json(rules_json).expect("Failed to parse rules with permanent buffs");

    let buffs = &rule_engine.items_config.items.permanent_buffs;
    assert_eq!(buffs.len(), 3);
    assert_eq!(buffs[0].name, "[HP上限+20]养生丸");
    assert_eq!(buffs[0].properties.effect_type, "max_life");
    assert_eq!(buffs[0].properties.effect_value, 20);
    assert_eq!(buffs[1].properties.effect_type, "max_strength");
    assert_eq!(buffs[2].properties.effect_type, "max_backpack");

    let item = rule_engine
        .create_item_from_name("[背包+6]百宝袋")
        .expect("Failed to create permanent buff item");
    assert_eq!(item.name, "[背包+6]百宝袋");
    match &item.item_type {
        ItemType::PermanentBuff(props) => {
            assert_eq!(props.effect_type, "max_backpack");
            assert_eq!(props.effect_value, 6);
        }
        _ => panic!("物品应该是永久增益类型"),
    }
}

/// 测试：旧配置（无 permanent_buffs 数组）解析为空列表
#[test]
fn test_permanent_buffs_absent_defaults_empty() {
    let rules_json = r#"{
        "map": {"places": ["loc"], "safe_places": []},
        "player": {"max_life": 100, "max_strength": 100, "daily_life_recovery": 0, "daily_strength_recovery": 40, "search_cooldown": 30, "max_backpack_items": 6, "unarmed_damage": 5},
        "action_costs": {"move": 5, "search": 5, "pick": 0, "attack": 0, "equip": 0, "use": 0, "throw": 0, "deliver": 10},
        "rest_mode": {"life_recovery": 25, "strength_recovery": 1000, "max_moves": 1},
        "death_item_disposition": "killer_takes_loot",
        "teammate_behavior": 0,
        "items_config": {"rarity_levels": [], "items": {}, "upgrade_recipes": {}}
    }"#;
    let rule_engine = GameRuleEngine::from_json(rules_json).expect("Failed to parse legacy rules");
    assert!(rule_engine.items_config.items.permanent_buffs.is_empty());
}
```

- [ ] **Step 2: 运行测试确认编译失败**

Run: `cd backend && cargo test --test game_rule_engine_integration test_permanent_buff`
Expected: 编译错误（`permanent_buffs` 字段不存在 / `ItemType::PermanentBuff` 不存在）

- [ ] **Step 3: 实现规则引擎改动**

在 `backend/src/game/game_rule_engine.rs` 中做四处修改：

① `ItemType` 枚举（L93-100）`Currency(CurrencyProperties),` 之后追加变体：

```rust
    Currency(CurrencyProperties),
    PermanentBuff(PermanentBuffProperties),
```

② `ItemsByCategory`（L197-211）`currencies` 字段后追加：

```rust
    #[serde(default)]
    pub currencies: Vec<CurrencyConfig>,
    #[serde(default)]
    pub permanent_buffs: Vec<PermanentBuffConfig>,
```

③ 在 `ConsumableProperties`（L288-293）定义之后新增两个结构体：

```rust
/// 永久增益属性
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermanentBuffProperties {
    pub effect_type: String,
    pub effect_value: i32,
}

/// 永久增益配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermanentBuffConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub internal_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rarity: Option<String>,
    pub properties: PermanentBuffProperties,
}
```

④ `create_item_from_name`（L440-514）中"6. 搜索货币"块之后、`Err(...)` 之前追加：

```rust
        // 7. 搜索永久增益道具
        for buff in &self.items_config.items.permanent_buffs {
            if buff.name == item_name {
                return Ok(Item::new(
                    buff.name.clone(),
                    buff.internal_name.clone(),
                    buff.rarity.clone(),
                    ItemType::PermanentBuff(buff.properties.clone()),
                ));
            }
        }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd backend && cargo test --test game_rule_engine_integration test_permanent_buff`
Expected: 2 个测试 PASS。同时 `cargo build` 无其他编译错误（`handle_use_action` 的 match 有 `_ =>` 兜底，新变体不会破坏现有穷尽匹配）。

- [ ] **Step 5: 提交**

```bash
git add backend/src/game/game_rule_engine.rs backend/tests/game_rule_engine_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(items): add permanent_buff item category to rule engine"
```

---

### Task 2: 玩家字段与 cap 配置（含旧状态兼容回填）

**Files:**
- Modify: `backend/src/game/game_rule_engine.rs`（PlayerConfig）
- Modify: `backend/src/websocket/models.rs`（Player 字段、`Player::new`、GameState 反序列化回填）
- Test: `backend/tests/permanent_buff_integration.rs`（新建）

**Interfaces:**
- Consumes: Task 1 的规则引擎结构
- Produces:
  - `PlayerConfig` 新字段：`max_life_cap: i32`（默认 300）、`max_strength_cap: i32`（默认 300）、`max_backpack_items_cap: usize`（默认 12），均 `#[serde(default = "...")]`
  - `Player.max_backpack_items: usize`（`#[serde(default)]`，出生时从规则初始化）
  - 测试文件级辅助函数（Task 3/4 复用）：`get_test_rules_with_permanent_buffs() -> serde_json::Value`、`rules_with_caps(i32, i32, usize) -> serde_json::Value`、`add_test_place(&mut GameState, &str)`、`add_test_player(&mut GameState, &str, &str, &str)`、`give_item(&mut GameState, &str, &str) -> String`、`empty_action_params() -> ActionParams`、`set_search_result_to_last_place_item(&mut GameState, &str, &str)`

- [ ] **Step 1: 新建测试文件，写失败测试**

创建 `backend/tests/permanent_buff_integration.rs`：

```rust
//! 永久增益道具系统集成测试
//! 测试上限提升、clamp、溢出浪费、背包扩容、旧状态兼容

use royale_arena_backend::game::game_rule_engine::GameRuleEngine;
use royale_arena_backend::websocket::actions::player_action_scheduler::ActionParams;
use royale_arena_backend::websocket::models::{
    GameState, Place, Player, SearchResult, SearchResultType,
};
use serde_json::{Value, json};

/// 测试规则配置（含永久增益道具）
fn get_test_rules_with_permanent_buffs() -> Value {
    json!({
        "map": {"places": ["位置1", "位置2"], "safe_places": []},
        "player": {
            "max_life": 100,
            "max_strength": 100,
            "daily_life_recovery": 0,
            "daily_strength_recovery": 40,
            "search_cooldown": 4,
            "max_backpack_items": 6,
            "unarmed_damage": 10
        },
        "action_costs": {"move": 5, "search": 5, "pick": 0, "attack": 0, "equip": 0, "use": 0, "throw": 0, "deliver": 10},
        "rest_mode": {"life_recovery": 25, "strength_recovery": 1000, "max_moves": 1},
        "death_item_disposition": "killer_takes_loot",
        "teammate_behavior": 0,
        "items_config": {
            "rarity_levels": [],
            "items": {
                "weapons": [],
                "armors": [],
                "utilities": [],
                "consumables": [],
                "upgraders": [],
                "currencies": [],
                "permanent_buffs": [
                    {"name": "[HP上限+20]养生丸", "properties": {"effect_type": "max_life", "effect_value": 20}},
                    {"name": "[HP上限+50]壮骨丹", "properties": {"effect_type": "max_life", "effect_value": 50}},
                    {"name": "[HP上限+100]金钟罩", "properties": {"effect_type": "max_life", "effect_value": 100}},
                    {"name": "[MP上限+50]行军丹", "properties": {"effect_type": "max_strength", "effect_value": 50}},
                    {"name": "[背包+2]腰包", "properties": {"effect_type": "max_backpack", "effect_value": 2}},
                    {"name": "[背包+6]百宝袋", "properties": {"effect_type": "max_backpack", "effect_value": 6}},
                    {"name": "[异常]无效丹", "properties": {"effect_type": "unknown_x", "effect_value": 10}}
                ]
            },
            "upgrade_recipes": {}
        }
    })
}

/// 覆盖三个 cap 字段的规则变体
fn rules_with_caps(max_life_cap: i32, max_strength_cap: i32, backpack_cap: usize) -> Value {
    let mut rules = get_test_rules_with_permanent_buffs();
    rules["player"]["max_life_cap"] = json!(max_life_cap);
    rules["player"]["max_strength_cap"] = json!(max_strength_cap);
    rules["player"]["max_backpack_items_cap"] = json!(backpack_cap);
    rules
}

fn add_test_place(game_state: &mut GameState, place_name: &str) {
    game_state
        .places
        .entry(place_name.to_string())
        .or_insert_with(|| Place::new(place_name.to_string()));
}

fn add_test_player(
    game_state: &mut GameState,
    player_id: &str,
    player_name: &str,
    location: &str,
) {
    add_test_place(game_state, location);

    let mut player = Player::new(
        player_id.to_string(),
        player_name.to_string(),
        "password".to_string(),
        1,
        &game_state.rule_engine,
    );
    player.location = location.to_string();

    game_state.players.insert(player_id.to_string(), player);
    game_state
        .places
        .get_mut(location)
        .unwrap()
        .players
        .push(player_id.to_string());
}

/// 按名称给玩家背包塞入道具，返回道具 ID
fn give_item(game_state: &mut GameState, player_id: &str, item_name: &str) -> String {
    let item = game_state
        .rule_engine
        .create_item_from_name(item_name)
        .expect("item should exist in rules");
    let item_id = item.id.clone();
    game_state
        .players
        .get_mut(player_id)
        .unwrap()
        .inventory
        .push(item);
    item_id
}

fn empty_action_params() -> ActionParams {
    ActionParams {
        target_place: None,
        place_name: None,
        item_id: None,
        item_ids: None,
        slot_type: None,
        target_player_id: None,
        target_player_ids: None,
        target_item_name: None,
        message: None,
        shop_buy_items: None,
    }
}

/// 把玩家上一次搜索结果指向地点中最后一个物品（供拾取测试使用）
fn set_search_result_to_last_place_item(
    game_state: &mut GameState,
    player_id: &str,
    place_name: &str,
) {
    let place = game_state.places.get(place_name).unwrap();
    let target_id = place.items.last().unwrap().id.clone();
    let target_name = place.items.last().unwrap().name.clone();
    game_state
        .players
        .get_mut(player_id)
        .unwrap()
        .last_search_result = Some(SearchResult {
        target_type: SearchResultType::Item,
        target_id,
        target_name,
        is_visible: true,
    });
}

/// 测试：cap 字段缺省时取默认值 300/300/12
#[test]
fn test_cap_defaults_when_not_configured() {
    let rules_json =
        serde_json::to_string(&get_test_rules_with_permanent_buffs()).expect("serialize rules");
    let engine = GameRuleEngine::from_json(&rules_json).expect("parse rules");
    assert_eq!(engine.player_config.max_life_cap, 300);
    assert_eq!(engine.player_config.max_strength_cap, 300);
    assert_eq!(engine.player_config.max_backpack_items_cap, 12);
}

/// 测试：cap 字段显式配置时可解析
#[test]
fn test_caps_parse_from_config() {
    let rules_json = serde_json::to_string(&rules_with_caps(250, 260, 10)).expect("serialize");
    let engine = GameRuleEngine::from_json(&rules_json).expect("parse");
    assert_eq!(engine.player_config.max_life_cap, 250);
    assert_eq!(engine.player_config.max_strength_cap, 260);
    assert_eq!(engine.player_config.max_backpack_items_cap, 10);
}

/// 测试：新玩家背包容量从规则初始化
#[test]
fn test_player_new_initializes_backpack_from_rules() {
    let rules_json =
        serde_json::to_string(&get_test_rules_with_permanent_buffs()).expect("serialize");
    let engine = GameRuleEngine::from_json(&rules_json).expect("parse");
    let player = Player::new(
        "p1".to_string(),
        "玩家1".to_string(),
        "pw".to_string(),
        1,
        &engine,
    );
    assert_eq!(player.max_backpack_items, 6);
    assert_eq!(player.max_life, 100);
    assert_eq!(player.max_strength, 100);
}

/// 测试：旧玩家状态（无 max_backpack_items 字段）反序列化后回填规则值
#[test]
fn test_deserialize_old_player_state_backfills_backpack() {
    let state_json = json!({
        "game_id": "g1",
        "players": {
            "p1": {
                "id": "p1",
                "name": "玩家1",
                "password": "pw",
                "location": "位置1",
                "life": 80,
                "strength": 90,
                "max_life": 100,
                "max_strength": 100,
                "inventory": [],
                "equipped_weapon": null,
                "equipped_armor": null,
                "last_search_result": null,
                "is_alive": true,
                "is_bound": false,
                "rest_mode": true,
                "rest_moves_used": 0,
                "last_search_time": null,
                "team_id": 1,
                "bleed_damage": 0,
                "coins": 0.0
            }
        },
        "places": {},
        "weather": 1.0,
        "votes": {},
        "rules_config": get_test_rules_with_permanent_buffs(),
        "night_start_time": null,
        "night_end_time": null,
        "next_night_destroyed_places": [],
        "save_time": null
    });
    let state: GameState =
        serde_json::from_value(state_json).expect("deserialize legacy game state");
    assert_eq!(state.players["p1"].max_backpack_items, 6);
}
```

- [ ] **Step 2: 运行测试确认编译失败**

Run: `cd backend && cargo test --test permanent_buff_integration`
Expected: 编译错误（`max_life_cap` / `max_backpack_items` 字段不存在）

- [ ] **Step 3: 实现**

① `game_rule_engine.rs` 的 `PlayerConfig`（L121-131）改为：

```rust
/// 玩家配置结构体
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlayerConfig {
    pub max_life: i32,
    pub max_strength: i32,
    #[serde(default = "default_max_life_cap")]
    pub max_life_cap: i32,
    #[serde(default = "default_max_strength_cap")]
    pub max_strength_cap: i32,
    pub daily_life_recovery: i32,
    pub daily_strength_recovery: i32,
    pub search_cooldown: i64,
    pub max_backpack_items: usize,
    #[serde(default = "default_max_backpack_items_cap")]
    pub max_backpack_items_cap: usize,
    pub unarmed_damage: i32, // 挥拳伤害
}

fn default_max_life_cap() -> i32 {
    300
}
fn default_max_strength_cap() -> i32 {
    300
}
fn default_max_backpack_items_cap() -> usize {
    12
}
```

② `models.rs` 的 `Player` 结构体：`coins: f64,`（L198-200）之前、`bleed_inflictor` 之后追加：

```rust
    /// 背包容量上限（初始来自规则，可被永久增益道具提升）
    #[serde(default)]
    pub max_backpack_items: usize,
```

③ `Player::new`（L205-238）：在 `let max_strength = ...;` 后加一行，并在 `Self { ... }` 中 `max_strength,` 后加字段：

```rust
        let max_life = rule_engine.player_config.max_life;
        let max_strength = rule_engine.player_config.max_strength;
        let max_backpack_items = rule_engine.player_config.max_backpack_items;
```

```rust
            max_life,
            max_strength,
            max_backpack_items,
```

④ `models.rs` 的 `impl<'de> Deserialize<'de> for GameState`（L522-571）：在 `let rule_engine = ...` 之后、`Ok(GameState {` 之前插入回填逻辑，并把构造处的 `players: helper.players` 换成 `players`：

```rust
        // 从 rules_config 重新创建 rule_engine
        let rules_json =
            serde_json::to_string(&helper.rules_config).map_err(serde::de::Error::custom)?;
        let rule_engine =
            GameRuleEngine::from_json(&rules_json).map_err(serde::de::Error::custom)?;

        // 旧存档玩家没有 max_backpack_items 字段（serde default 为 0），回填为规则初始值
        let mut players = helper.players;
        let default_backpack = rule_engine.player_config.max_backpack_items;
        for player in players.values_mut() {
            if player.max_backpack_items == 0 {
                player.max_backpack_items = default_backpack;
            }
        }

        Ok(GameState {
            game_id: helper.game_id,
            players,
            // ...其余字段不变
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd backend && cargo test --test permanent_buff_integration`
Expected: 4 个测试 PASS

- [ ] **Step 5: 提交**

```bash
git add backend/src/game/game_rule_engine.rs backend/src/websocket/models.rs backend/tests/permanent_buff_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(player): per-player backpack capacity with configurable hard caps"
```

---

### Task 3: 使用逻辑 handle_permanent_buff_use

**Files:**
- Modify: `backend/src/websocket/actions/player_use_action.rs`
- Test: `backend/tests/permanent_buff_integration.rs`

**Interfaces:**
- Consumes: Task 1 的 `PermanentBuffProperties`、Task 2 的 `Player.max_backpack_items` 与 cap 字段、测试辅助函数
- Produces: `handle_use_action` 支持 `ItemType::PermanentBuff`；日志 data 字段 `max_life`+`max_life_delta` / `max_strength`+`max_strength_delta` / `max_backpack_items`+`max_backpack_items_delta`（均含 `strength`/`strength_delta`）

- [ ] **Step 1: 写失败测试**

在 `permanent_buff_integration.rs` 末尾追加：

```rust
/// 测试：使用血量上限道具——只涨上限，当前血量/体力不变，道具销毁
#[test]
fn test_use_max_life_buff_raises_cap_only() {
    let mut state = GameState::new("g1".to_string(), get_test_rules_with_permanent_buffs());
    add_test_player(&mut state, "p1", "玩家1", "位置1");
    state.players.get_mut("p1").unwrap().life = 80;

    let item_id = give_item(&mut state, "p1", "[HP上限+50]壮骨丹");
    let results = state
        .handle_use_action("p1", &item_id, &empty_action_params())
        .expect("use should succeed");

    let player = state.players.get("p1").unwrap();
    assert_eq!(player.max_life, 150);
    assert_eq!(player.life, 80, "当前血量不应变化");
    assert_eq!(player.strength, 100, "use 消耗默认为 0");
    assert!(player.inventory.is_empty(), "道具应被销毁");
    assert_eq!(results.results.len(), 1);
}

/// 测试：使用体力上限道具
#[test]
fn test_use_max_strength_buff() {
    let mut state = GameState::new("g1".to_string(), get_test_rules_with_permanent_buffs());
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    let item_id = give_item(&mut state, "p1", "[MP上限+50]行军丹");
    state
        .handle_use_action("p1", &item_id, &empty_action_params())
        .expect("use should succeed");

    let player = state.players.get("p1").unwrap();
    assert_eq!(player.max_strength, 150);
    assert_eq!(player.strength, 100, "当前体力不应变化");
    assert!(player.inventory.is_empty());
}

/// 测试：连续使用至硬上限——clamp 且溢出浪费
#[test]
fn test_use_max_life_buff_clamps_to_cap() {
    let mut state = GameState::new("g1".to_string(), rules_with_caps(150, 300, 12));
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    // 100 + 100 → clamp 150，溢出 50 浪费
    let id1 = give_item(&mut state, "p1", "[HP上限+100]金钟罩");
    state
        .handle_use_action("p1", &id1, &empty_action_params())
        .expect("use should succeed");
    assert_eq!(state.players["p1"].max_life, 150);

    // 已到 cap 再用 +50 → +0，道具照常消耗
    let id2 = give_item(&mut state, "p1", "[HP上限+50]壮骨丹");
    state
        .handle_use_action("p1", &id2, &empty_action_params())
        .expect("use should succeed");
    assert_eq!(state.players["p1"].max_life, 150);
    assert!(state.players["p1"].inventory.is_empty());
}

/// 测试：cap 低于规则基础值时永不降低上限（生效上限 = max(cap, 基础值)）
#[test]
fn test_cap_below_base_never_lowers_max() {
    let mut state = GameState::new("g1".to_string(), rules_with_caps(50, 300, 12));
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    let item_id = give_item(&mut state, "p1", "[HP上限+100]金钟罩");
    state
        .handle_use_action("p1", &item_id, &empty_action_params())
        .expect("use should succeed");
    assert_eq!(state.players["p1"].max_life, 100, "生效上限应为 max(50, 100) = 100");
}

/// 测试：未知 effect_type —— 报错提示、道具回插、不扣体力
#[test]
fn test_unknown_effect_type_reinserts_item() {
    let mut state = GameState::new("g1".to_string(), get_test_rules_with_permanent_buffs());
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    let item_id = give_item(&mut state, "p1", "[异常]无效丹");
    let results = state
        .handle_use_action("p1", &item_id, &empty_action_params())
        .expect("use action returns info result");

    let player = state.players.get("p1").unwrap();
    assert_eq!(player.inventory.len(), 1, "道具应原位插回");
    assert_eq!(player.inventory[0].id, item_id);
    assert_eq!(player.strength, 100);
    assert_eq!(results.results.len(), 1);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd backend && cargo test --test permanent_buff_integration test_use_`
Expected: 前两个测试 FAIL（走 `_ =>` 兜底臂返回"无法通过使用行动触发"，道具被回插）；后三个同样 FAIL

- [ ] **Step 3: 实现**

① 文件顶部 use 语句（L3-5）加入 `PermanentBuffProperties`：

```rust
use crate::game::game_rule_engine::{
    ConsumableProperties, CurrencyProperties, Item, ItemType, PermanentBuffProperties,
    UtilityProperties,
};
```

② `handle_use_action` 的 match（L95-123）中 `ItemType::Utility(properties)` 臂之后、`_ =>` 之前追加：

```rust
            ItemType::PermanentBuff(effect) => self.handle_permanent_buff_use(
                player_id,
                &player_name,
                &item.name,
                effect,
                strength_before,
                use_cost,
            ),
```

③ `handle_currency_use` 之前新增函数：

```rust
    fn handle_permanent_buff_use(
        &mut self,
        player_id: &str,
        player_name: &str,
        item_display_name: &str,
        effect: &PermanentBuffProperties,
        strength_before: i32,
        use_cost: i32,
    ) -> Result<ItemUseOutcome, String> {
        let (max_life_cap, base_max_life, max_strength_cap, base_max_strength, backpack_cap, base_backpack) = {
            let pc = &self.rule_engine.player_config;
            (
                pc.max_life_cap,
                pc.max_life,
                pc.max_strength_cap,
                pc.max_strength,
                pc.max_backpack_items_cap,
                pc.max_backpack_items,
            )
        };

        let (label, before, after) = match effect.effect_type.as_str() {
            "max_life" => {
                let cap = max_life_cap.max(base_max_life);
                let player = self.players.get_mut(player_id).unwrap();
                let before = player.max_life;
                player.max_life = (player.max_life + effect.effect_value).min(cap);
                ("生命上限", before, player.max_life)
            }
            "max_strength" => {
                let cap = max_strength_cap.max(base_max_strength);
                let player = self.players.get_mut(player_id).unwrap();
                let before = player.max_strength;
                player.max_strength = (player.max_strength + effect.effect_value).min(cap);
                ("体力上限", before, player.max_strength)
            }
            "max_backpack" => {
                let cap = backpack_cap.max(base_backpack);
                let player = self.players.get_mut(player_id).unwrap();
                let before = player.max_backpack_items;
                let boost = effect.effect_value.max(0) as usize;
                player.max_backpack_items = (player.max_backpack_items + boost).min(cap);
                ("背包容量", before as i32, player.max_backpack_items as i32)
            }
            _ => return Err(format!("永久增益道具 {} 没有定义效果", item_display_name)),
        };

        let delta = after - before;
        let strength_after = self.predict_strength_after_use(player_id, use_cost);
        let strength_delta = strength_after - strength_before;

        let log_message = format!(
            "{} 使用了 {}，{}: {} ({})，体力: {} ({})",
            player_name,
            item_display_name,
            label,
            after,
            format_delta(delta),
            strength_after,
            format_delta(strength_delta)
        );

        let data = match effect.effect_type.as_str() {
            "max_life" => json!({
                "max_life": after,
                "max_life_delta": delta,
                "strength": strength_after,
                "strength_delta": strength_delta,
            }),
            "max_strength" => json!({
                "max_strength": after,
                "max_strength_delta": delta,
                "strength": strength_after,
                "strength_delta": strength_delta,
            }),
            _ => json!({
                "max_backpack_items": after,
                "max_backpack_items_delta": delta,
                "strength": strength_after,
                "strength_delta": strength_delta,
            }),
        };

        let result =
            ActionResult::new_system_message(data, vec![player_id.to_string()], log_message, true);

        Ok(ItemUseOutcome::new(vec![result]).with_reinsert(false))
    }
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd backend && cargo test --test permanent_buff_integration`
Expected: 全部 9 个测试 PASS

- [ ] **Step 5: 提交**

```bash
git add backend/src/websocket/actions/player_use_action.rs backend/tests/permanent_buff_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(items): permanent buff use action with cap clamp"
```

---

### Task 4: 背包上限引用点切换（全局规则 → 玩家字段）

**Files:**
- Modify: `backend/src/websocket/actions/player_common_actions.rs`（3 处）
- Modify: `backend/src/websocket/actions/player_action_scheduler.rs`（1 处）
- Modify: `backend/src/websocket/actions/game_state_common.rs`（1 处）
- Test: `backend/tests/permanent_buff_integration.rs`

**Interfaces:**
- Consumes: Task 2 的 `Player.max_backpack_items`、Task 3 的使用逻辑
- Produces: 所有背包容量判断以 `player.max_backpack_items` 为准

- [ ] **Step 1: 写失败测试**

在 `permanent_buff_integration.rs` 末尾追加：

```rust
/// 测试：背包扩容后可继续拾取（拾取容量检查使用玩家字段而非全局规则）
#[test]
fn test_backpack_expansion_allows_more_picks() {
    let mut state = GameState::new("g1".to_string(), get_test_rules_with_permanent_buffs());
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    // 背包先放一个扩容道具（占 1 格，基础容量 6）
    let bag_id = give_item(&mut state, "p1", "[背包+6]百宝袋");
    assert_eq!(state.players["p1"].get_total_item_count(), 1);

    // 地点放 8 个物品
    for _ in 0..8 {
        let item = state.rule_engine.create_item_from_name("[HP上限+20]养生丸").unwrap();
        state.places.get_mut("位置1").unwrap().items.push(item);
    }

    // 连续拾取 5 次：容量 6 已占 1，第 6 次提示背包已满
    for _ in 0..5 {
        set_search_result_to_last_place_item(&mut state, "p1", "位置1");
        state.handle_pick_action("p1").expect("pick should succeed");
    }
    assert_eq!(state.players["p1"].get_total_item_count(), 6);

    set_search_result_to_last_place_item(&mut state, "p1", "位置1");
    state.handle_pick_action("p1").expect("pick returns info");
    assert_eq!(state.players["p1"].get_total_item_count(), 6, "容量 6 时应拒绝拾取");

    // 使用百宝袋：容量 12，道具销毁后背包 5 件
    state
        .handle_use_action("p1", &bag_id, &empty_action_params())
        .expect("use bag");
    assert_eq!(state.players["p1"].max_backpack_items, 12);
    assert_eq!(state.players["p1"].get_total_item_count(), 5);

    // 扩容后可继续拾取（若容量检查仍读全局规则 6，此处会失败）
    set_search_result_to_last_place_item(&mut state, "p1", "位置1");
    state.handle_pick_action("p1").expect("pick after expansion");
    assert_eq!(state.players["p1"].get_total_item_count(), 6);
}

/// 测试：商店可按名称上架永久增益道具（现有"拒绝武器/防具"逻辑自动放行）
#[test]
fn test_shop_list_permanent_buff_by_name() {
    let mut state = GameState::new("g1".to_string(), get_test_rules_with_permanent_buffs());
    state
        .handle_shop_list_item("[HP上限+20]养生丸".to_string(), 10, 2)
        .expect("list should succeed");
    assert_eq!(state.shop.len(), 1);
    assert_eq!(state.shop[0].item_name, "[HP上限+20]养生丸");
    assert_eq!(state.shop[0].price, 10);
    assert_eq!(state.shop[0].quantity, 2);
}
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cd backend && cargo test --test permanent_buff_integration test_backpack_expansion`
Expected: `test_backpack_expansion_allows_more_picks` FAIL——扩容后拾取被"背包已满"拦截（容量仍读全局规则值 6）；`test_shop_list_permanent_buff_by_name` 此时即 PASS（商店逻辑本任务不改，仅验证规格行为）

- [ ] **Step 3: 切换 5 处引用**

① `player_common_actions.rs` L194-199（`handle_pick_action` 容量检查）：

```rust
        // 使用规则引擎检查背包容量（使用总物品数量）
        {
            let player = self.players.get(player_id).unwrap();

            if player.get_total_item_count() >= player.max_backpack_items {
```
（删除原来的 `let max_backpack_items = self.rule_engine.player_config.max_backpack_items as usize;` 行与比较中的局部变量）

② `player_common_actions.rs` L777（商店购买背包空间检查，`player` 引用在 L764 已取得）：

```rust
        // 检查背包空间
        let max_inventory_size = player.max_backpack_items;
```

③ `player_common_actions.rs` L953 附近（队友转让接收方容量检查）：

```rust
        // 6. 接收方背包未满
        let target = self.players.get(target_player_id);
        let max = target.map(|p| p.max_backpack_items).unwrap_or(0);
        let target_count = target.map(|p| p.get_total_item_count()).unwrap_or(0);
```

④ `player_action_scheduler.rs` L507（`check_inventory_space_from_ref`，函数参数已有 `player: &Player`）：

```rust
        // 获取背包最大容量
        let max_inventory_size = player.max_backpack_items;
```

⑤ `game_state_common.rs` L161-164（击杀者收缴战利品，位于 `if let Some(killer) = self.players.get_mut(loot_player_id)` 块内）：

```rust
                                let max_backpack = killer.max_backpack_items;
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cd backend && cargo test`
Expected: permanent_buff_integration 全部 PASS，且现有测试套件无回归（现有测试规则里玩家容量都等于规则值，行为不变）

- [ ] **Step 5: 提交**

```bash
git add backend/src/websocket/actions/player_common_actions.rs backend/src/websocket/actions/player_action_scheduler.rs backend/src/websocket/actions/game_state_common.rs backend/tests/permanent_buff_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "refactor(inventory): backpack capacity checks read per-player field"
```

---

### Task 5: 三份规则配置文件更新

**Files:**
- Modify: `director_rules_config.json`
- Modify: `full-feature-rules-template.json`
- Modify: `frontend/src/constants/defaultRulesConfig.ts`

**Interfaces:**
- Consumes: Task 1/2 的配置 schema
- Produces: 三份配置均含 3 个 cap 字段与 9 个永久增益道具

- [ ] **Step 1: 更新 `director_rules_config.json`**

`"player"` 对象内（`"max_backpack_items"` 与 `"unarmed_damage"` 之间或之后）加入：

```json
    "max_life_cap": 300,
    "max_strength_cap": 300,
    "max_backpack_items_cap": 12,
```

`"items_config"."items"` 内 `"currencies"` 数组之后加入：

```json
      "permanent_buffs": [
        {"name": "[HP上限+20]养生丸", "properties": {"effect_type": "max_life", "effect_value": 20}},
        {"name": "[HP上限+50]壮骨丹", "properties": {"effect_type": "max_life", "effect_value": 50}},
        {"name": "[HP上限+100]金钟罩", "properties": {"effect_type": "max_life", "effect_value": 100}},
        {"name": "[MP上限+20]干粮", "properties": {"effect_type": "max_strength", "effect_value": 20}},
        {"name": "[MP上限+50]行军丹", "properties": {"effect_type": "max_strength", "effect_value": 50}},
        {"name": "[MP上限+100]龙力丸", "properties": {"effect_type": "max_strength", "effect_value": 100}},
        {"name": "[背包+2]腰包", "properties": {"effect_type": "max_backpack", "effect_value": 2}},
        {"name": "[背包+4]行囊", "properties": {"effect_type": "max_backpack", "effect_value": 4}},
        {"name": "[背包+6]百宝袋", "properties": {"effect_type": "max_backpack", "effect_value": 6}}
      ],
```

- [ ] **Step 2: 同样内容应用到 `full-feature-rules-template.json`**（同样的 player 字段与 permanent_buffs 数组；注意保持 JSON 逗号合法）

- [ ] **Step 3: 同样内容应用到 `frontend/src/constants/defaultRulesConfig.ts`**——这是 TS 导出的对象字面量（L29-37 是 `"player"` 段，L364 起是 `"currencies"`），加入相同的三个字段与 `permanent_buffs` 数组（键名带引号，与文件现有风格一致）

- [ ] **Step 4: 验证**

Run: `node -e "JSON.parse(require('fs').readFileSync('director_rules_config.json','utf8')); JSON.parse(require('fs').readFileSync('full-feature-rules-template.json','utf8')); console.log('json ok')"` 和 `cd backend && cargo test`
Expected: `json ok`，后端测试全 PASS

- [ ] **Step 5: 提交**

```bash
git add director_rules_config.json full-feature-rules-template.json frontend/src/constants/defaultRulesConfig.ts
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "chore(config): add caps and 9 permanent buff items to rule configs"
```

---

### Task 6: 前端解析与校验

**Files:**
- Modify: `frontend/src/utils/itemConfigUtils.ts`
- Modify: `frontend/src/utils/itemParser.ts`
- Modify: `frontend/src/utils/gameRuleParser.ts`
- Modify: `frontend/src/types/gameStateTypes.ts`

**Interfaces:**
- Consumes: Task 5 的配置 schema
- Produces: `ItemsByCategoryConfig.permanentBuffs: PermanentBuffConfig[]`；`ParsedItemInfo.permanentBuffs: string[]`；`ItemCategory` 联合类型含 `'permanent_buff'`；`Player` 接口含 `max_backpack_items: number`

- [ ] **Step 1: `itemConfigUtils.ts`**

① `ConsumableConfig` 接口（L58-63）后新增：

```ts
export interface PermanentBuffProperties {
  effectType: string
  effectValue: number
}

export interface PermanentBuffConfig {
  name: string
  internalName?: string
  rarity?: string
  properties: PermanentBuffProperties
}
```

② `ItemsByCategoryConfig`（L87-94）加字段：`permanentBuffs: PermanentBuffConfig[]`

③ `createEmptyItemsConfig`（L107-118）的 items 里加 `permanentBuffs: []`

④ L165 解构改为 `const { weapons, armors, utilities, consumables, currencies, upgraders, permanent_buffs } = itemsSection`

⑤ currencies 解析块（L308-328）之后新增：

```ts
    if (Array.isArray(permanent_buffs)) {
      parsed.items.permanentBuffs = permanent_buffs.map((buff: any) => {
        const buffProperties = buff?.properties ?? {}
        const properties: PermanentBuffProperties = {
          effectType: typeof buffProperties.effect_type === 'string' ? buffProperties.effect_type : '',
          effectValue: toNumberWithDefault(buffProperties.effect_value, 0)
        }

        const rarity = typeof buff?.rarity === 'string' && buff.rarity.length > 0
          ? buff.rarity
          : undefined

        return {
          name: typeof buff?.name === 'string' ? buff.name : '',
          internalName: typeof buff?.internal_name === 'string' ? buff.internal_name : undefined,
          rarity,
          properties
        }
      })
    } else if (permanent_buffs !== undefined) {
      issues.push('items_config.items.permanent_buffs 应为数组')
    }
```

⑥ `findDuplicateItemNames`（L369-415）的 currencies 循环后新增：

```ts
  for (const buff of itemsConfig.items.permanentBuffs) {
    trackName(buff.name)
  }
```

- [ ] **Step 2: `itemParser.ts`**

① `ParsedItemInfo`（L26-41）加字段 `permanentBuffs: string[]`（放在 `upgraders` 后）

② 构造器 `hasAnyItem`（L73-80）加 `config.items.permanentBuffs.length > 0 ||`

③ `parseAllItems`（L98-154）：upgraders 收集块后加：

```ts
    // 永久增益道具
    const permanentBuffs = this.itemConfig.items.permanentBuffs.map(item => item.name)
    allItems.push(...permanentBuffs)
```

return 对象中加 `permanentBuffs,`

- [ ] **Step 3: `gameRuleParser.ts`（校验白名单——不加会拒绝新配置）**

① 类别未知字段检查（L419-426）数组加入 `'permanent_buffs'`

② currencies 校验块之后新增（缩进/风格与现有块一致，该文件用 tab）：

```ts
					if (categories.permanent_buffs !== undefined && !Array.isArray(categories.permanent_buffs)) {
						errors.push('items_config.items.permanent_buffs 必须是数组')
					} else if (Array.isArray(categories.permanent_buffs)) {
						categories.permanent_buffs.forEach((buff: any, index: number) => {
							if (!buff || typeof buff !== 'object') {
								errors.push(`永久增益[${index}]配置必须是对象`)
								return
							}
							const buffUnexpected = this.findUnexpectedKeys(buff as Record<string, unknown>, [
								'name',
								'internal_name',
								'rarity',
								'properties'
							])
							if (buffUnexpected.length > 0) {
								errors.push(`items_config.items.permanent_buffs[${index}] 包含未知字段: ${buffUnexpected.join(', ')}`)
							}
							if (!buff.name || typeof buff.name !== 'string' || buff.name.trim().length === 0) {
								errors.push(`永久增益[${index}]缺少名称`)
							}
							if (!buff.properties || typeof buff.properties !== 'object') {
								errors.push(`永久增益[${index}]缺少属性配置`)
							} else {
								const properties = buff.properties
								const buffPropUnexpected = this.findUnexpectedKeys(properties, [
									'effect_type',
									'effect_value'
								])
								if (buffPropUnexpected.length > 0) {
									errors.push(`items_config.items.permanent_buffs[${index}].properties 包含未知字段: ${buffPropUnexpected.join(', ')}`)
								}
								if (typeof properties.effect_type !== 'string' || !['max_life', 'max_strength', 'max_backpack'].includes(properties.effect_type)) {
									errors.push(`永久增益[${index}]效果类型必须为 max_life/max_strength/max_backpack`)
								}
								if (typeof properties.effect_value !== 'number' || !Number.isFinite(properties.effect_value) || properties.effect_value <= 0) {
									errors.push(`永久增益[${index}]效果值必须是正数`)
								}
							}
						})
					}
```

③ player 配置未知字段检查（L254-262）数组加入 `'max_life_cap'`、`'max_strength_cap'`、`'max_backpack_items_cap'`

④ `playerFields` 必填循环（L266-285）之后新增可选字段校验：

```ts
				const optionalPlayerFields: Array<[string, string]> = [
					['max_life_cap', '生命上限硬上限'],
					['max_strength_cap', '体力上限硬上限'],
					['max_backpack_items_cap', '背包容量硬上限']
				]

				for (const [field, label] of optionalPlayerFields) {
					if (config.player[field] === undefined) {
						continue
					}
					const value = config.player[field]
					if (typeof value !== 'number' || !Number.isFinite(value)) {
						errors.push(`${label}必须是数字`)
					}
				}
```

- [ ] **Step 4: `gameStateTypes.ts`**

① L27 `ItemCategory` 联合类型加入 `'permanent_buff'`：

```ts
export type ItemCategory = 'weapon' | 'armor' | 'consumable' | 'utility' | 'upgrader' | 'currency' | 'permanent_buff';
```

② `Player` 接口（L52-53）`max_strength: number;` 后加：

```ts
  max_backpack_items: number;
```

- [ ] **Step 5: 验证**

Run: `cd frontend && pnpm build`
Expected: vue-tsc 无类型错误，vite 构建成功

- [ ] **Step 6: 提交**

```bash
git add frontend/src/utils/itemConfigUtils.ts frontend/src/utils/itemParser.ts frontend/src/utils/gameRuleParser.ts frontend/src/types/gameStateTypes.ts
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(frontend): parse and validate permanent_buff category"
```

---

### Task 7: 前端 UI（空投 / 商店 / 规则预览）

**Files:**
- Modify: `frontend/src/views/director/components/BatchAirdropDialog.vue`
- Modify: `frontend/src/views/director/components/ShopManagement.vue`
- Modify: `frontend/src/components/GameRulesPreview.vue`

**Interfaces:**
- Consumes: Task 6 的 `parsedItems.permanentBuffs` / `parsedRules.itemsConfig.items.permanentBuffs`

- [ ] **Step 1: `BatchAirdropDialog.vue`**

① "升级器"分类块（L102-114）之后新增同构分类块：

```vue
          <!-- 永久增益 -->
          <div class="item-category">
            <h5>永久增益</h5>
            <div v-for="item in parsedItems?.permanentBuffs || []" :key="item" class="item-option">
              <el-form-item :label="item">
                <el-input-number
                  v-model="specificSelections[item]"
                  :min="0"
                  placeholder="数量"
                  style="width: 100%"
                />
              </el-form-item>
            </div>
          </div>
```

② L423 随机物品合并列表加入新类别：

```ts
  [...parsedItems.value.utilities, ...parsedItems.value.consumables, ...parsedItems.value.currencies, ...parsedItems.value.upgraders, ...parsedItems.value.permanentBuffs]
```

- [ ] **Step 2: `ShopManagement.vue`**

L155-159 分组逻辑中"货币"分组之后新增：

```ts
  if (p.permanentBuffs.length > 0) {
    groups.push({ label: '永久增益', items: p.permanentBuffs })
  }
```

- [ ] **Step 3: `GameRulesPreview.vue`**

"消耗品" tab（L259-282）之后新增（效果类型做中文映射）：

```vue
              <el-tab-pane label="永久增益" name="permanent_buffs">
                <div class="table-wrapper">
                  <el-table :data="parsedRules.itemsConfig.items.permanentBuffs" style="width: 100%">
                    <el-table-column prop="name" label="名称" />
                    <el-table-column label="效果类型">
                      <template #default="scope">
                        {{ { max_life: '生命上限', max_strength: '体力上限', max_backpack: '背包容量' }[scope.row.properties.effectType] || scope.row.properties.effectType }}
                      </template>
                    </el-table-column>
                    <el-table-column label="效果值">
                      <template #default="scope">
                        {{ scope.row.properties.effectValue }}
                      </template>
                    </el-table-column>
                  </el-table>
                </div>
              </el-tab-pane>
```

- [ ] **Step 4: 验证**

Run: `cd frontend && pnpm build`
Expected: 构建成功

- [ ] **Step 5: 提交**

```bash
git add frontend/src/views/director/components/BatchAirdropDialog.vue frontend/src/views/director/components/ShopManagement.vue frontend/src/components/GameRulesPreview.vue
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(frontend): permanent buff category in airdrop, shop and rules preview"
```

---

### Task 8: 文档更新

**Files:**
- Modify: `frontend/public/docs/game-rules-explain.md`
- Modify: `docs/api/data-models.md`
- Modify: `docs/api/game-rules-config.md`
- Modify: `docs/api/ws/player-actions.md`

**Interfaces:**
- Consumes: 前面所有任务最终行为

- [ ] **Step 1: `frontend/public/docs/game-rules-explain.md`**

在"消耗品"章节之后新增"永久增益"章节，内容要点：

```markdown
## 永久增益道具（permanent_buffs）

使用后**永久提升**对应属性的上限，道具消耗。配置结构与消耗品一致：

| 字段 | 说明 |
|------|------|
| name | 道具名称 |
| rarity | 可选稀有度 |
| properties.effect_type | `max_life`（生命上限）/ `max_strength`（体力上限）/ `max_backpack`（背包容量） |
| properties.effect_value | 提升数值 |

规则：
- 只提升上限，不恢复当前血量/体力
- 溢出浪费：达到硬上限后再使用，超出部分无效，道具照常消耗
- 硬上限在 player 配置：`max_life_cap`（默认 300）、`max_strength_cap`（默认 300）、`max_backpack_items_cap`（默认 12）
- 生效上限 = max(硬上限, 规则基础值)，道具永远不会降低上限
- 使用体力消耗同消耗品（action_costs.use，默认 0）
```

- [ ] **Step 2: `docs/api/data-models.md`**

Player 字段表（max_life/max_strength 所在处，约 L145-146）补充两行：

```markdown
| max_backpack_items | int | 背包容量上限（初始来自规则，可被永久增益道具提升） |
```

并在 PlayerConfig 说明处补充 `max_life_cap` / `max_strength_cap` / `max_backpack_items_cap`（默认 300/300/12）。

- [ ] **Step 3: `docs/api/game-rules-config.md`**

在消耗品配置说明之后补充 `permanent_buffs` 数组与 player 三个 cap 字段的配置示例（照抄 Task 5 的 JSON 片段）。

- [ ] **Step 4: `docs/api/ws/player-actions.md`**

use 动作说明中补充：对 `permanent_buff` 类型道具，返回 data 含 `max_life`+`max_life_delta`（或 `max_strength`+`max_strength_delta`、`max_backpack_items`+`max_backpack_items_delta`）及 `strength`/`strength_delta`；未知 effect_type 返回 Info 错误且道具退回背包。

- [ ] **Step 5: 提交**

```bash
git add frontend/public/docs/game-rules-explain.md docs/api/data-models.md docs/api/game-rules-config.md docs/api/ws/player-actions.md
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "docs: permanent buff category documentation"
```

---

### Task 9: 全量验证与收尾

**Files:** 无新改动（验证 + 可能的修复）

- [ ] **Step 1: 后端全量测试**

Run: `cd backend && cargo test`
Expected: 全部 PASS（含现有 game_rule_engine_integration、currency_and_movement、shop_rarity、sell_system、teammate_system 等无回归）

- [ ] **Step 2: 前端构建**

Run: `cd frontend && pnpm build`
Expected: 构建成功

- [ ] **Step 3: 冒烟验证规则解析链路**

Run: `cd backend && cargo test --test permanent_buff_integration`
Expected: 11 个测试 PASS

- [ ] **Step 4: 检查工作区干净**

Run: `git status --porcelain`
Expected: 仅剩无关的未跟踪文件（如 royale-arena-src.zip），本特性改动全部已提交

- [ ] **Step 5: 若以上全部通过，向用户汇报完成情况**（后端手动冒烟：启动前后端在浏览器实际使用一次新道具可作为可选补充，向用户说明）

---

## Self-Review 记录

- 规格覆盖：独立类别（T1）、cap 配置+玩家字段+旧状态兼容（T2）、使用逻辑+clamp+溢出+错误处理（T3）、背包 5 处引用（T4）、三份配置（T5）、前端解析/校验/类型（T6）、空投/商店/预览 UI（T7）、4 份文档（T8）、全量验证（T9）——规格各节均有对应任务
- 规格测试清单逐条核对：①三类使用（T3）②clamp/溢出（T3）③背包扩容拾取（T4）④未知 effect_type（T3）⑤旧规则配置兼容（T1 空 `permanent_buffs` + T2 cap 默认值）⑥旧玩家状态反序列化（T2）⑦商店按名称上架（T4 补 `test_shop_list_permanent_buff_by_name`）⑧规则解析断言（T1）
- 商店上架逻辑：规格确认无需改动（现有"拒绝武器/防具"逻辑自动放行），仅需行为验证测试
- 类型一致性：`PermanentBuffProperties.effect_type: String` / `effect_value: i32` 与 Rust 使用处、TS `effectType: string` / `effectValue: number`、serde JSON `effect_type`/`effect_value` 对应一致；测试辅助函数在 Task 2 定义、Task 3/4 按同名复用
