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

// 以下辅助函数供 Task 3/4 的后续测试复用，当前任务暂未使用
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
#[allow(dead_code)]
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
        serde_json::to_string(&get_test_rules_with_permanent_buffs()).expect("serialize rules");
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
