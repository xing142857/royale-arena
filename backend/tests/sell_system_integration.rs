//! 售出系统集成测试

use chrono::{Duration, Utc};
use royale_arena_backend::game::game_rule_engine::GameRuleEngine;
use royale_arena_backend::websocket::models::GameState;
use serde_json::Value;

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

pub fn sell_test_rules(_mode: i32) -> Value {
    serde_json::from_str(SELL_RULES).unwrap()
}

fn build_sell_game_state() -> GameState {
    GameState::new("sell_game".to_string(), sell_test_rules(0))
}

fn sell_add_player(state: &mut GameState, id: &str, name: &str) {
    let engine = GameRuleEngine::from_json(&state.rules_config.to_string()).unwrap();
    let player = royale_arena_backend::websocket::models::Player::new(
        id.to_string(),
        name.to_string(),
        "pw".to_string(),
        0,
        &engine,
    );
    state.players.insert(id.to_string(), player);
}

#[allow(dead_code)] // 后续售出任务复用
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
    assert!(
        (p.coins - 1.0).abs() < 1e-9,
        "两件 0.5 应累计为 1.0，实际 {}",
        p.coins
    );
}

#[test]
fn game_state_deserializes_without_sell_prices() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    let serialized = serde_json::to_string(&state).unwrap();
    // 模拟旧存档：手工删掉 sell_prices 字段后仍能加载
    let legacy = serialized
        .replace("\"sell_prices\":[]", "")
        .replace(",}", "}");
    let restored: GameState =
        serde_json::from_str(&legacy).expect("旧存档（无 sell_prices）必须能反序列化");
    assert!(restored.sell_prices.is_empty());
}

use royale_arena_backend::game::models::MessageType;
use royale_arena_backend::websocket::actions::director_action_scheduler::{
    DirectorActionParams, DirectorActionScheduler,
};

fn director_dispatch(
    state: &mut GameState,
    params: serde_json::Value,
) -> Result<royale_arena_backend::websocket::models::ActionResults, String> {
    let action_type = params["action_type"].as_str().unwrap_or("").to_string();
    let ap = DirectorActionParams::from_json(&params)?;
    DirectorActionScheduler::dispatch(state, &action_type, ap)
}

#[test]
fn sell_set_price_adds_entry() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    let results = director_dispatch(
        &mut state,
        serde_json::json!({"action_type": "sell_set_price", "sell_rarity": "common", "sell_price": 2.5}),
    )
    .expect("dispatch ok");
    assert_eq!(results.results[0].message_type, MessageType::SystemNotice);
    assert_eq!(state.sell_prices.len(), 1);
    assert_eq!(state.sell_prices[0].rarity, "common");
    assert!((state.sell_prices[0].price - 2.5).abs() < 1e-9);
}

#[test]
fn sell_set_price_updates_existing_entry() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    director_dispatch(
        &mut state,
        serde_json::json!({"action_type":"sell_set_price","sell_rarity":"rare","sell_price":1.0}),
    )
    .unwrap();
    director_dispatch(
        &mut state,
        serde_json::json!({"action_type":"sell_set_price","sell_rarity":"rare","sell_price":3.0}),
    )
    .unwrap();
    assert_eq!(state.sell_prices.len(), 1, "同一稀有度只有一条");
    assert!((state.sell_prices[0].price - 3.0).abs() < 1e-9, "改价生效");
}

#[test]
fn sell_set_price_rejects_bad_rarity_and_price() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    for (rarity, price) in [
        ("purple", 1.0),
        ("common", 0.0),
        ("common", -0.5),
        ("common", 0.3),
    ] {
        let results = director_dispatch(
            &mut state,
            serde_json::json!({"action_type":"sell_set_price","sell_rarity":rarity,"sell_price":price}),
        )
        .expect("ok");
        assert_eq!(
            results.results[0].message_type,
            MessageType::Info,
            "{}@{} 应被拒绝",
            rarity,
            price
        );
    }
    assert!(state.sell_prices.is_empty(), "拒绝时不产生条目");
}

#[test]
fn sell_remove_price_works() {
    let mut state = build_sell_game_state();
    sell_add_player(&mut state, "p1", "玩家一");
    director_dispatch(
        &mut state,
        serde_json::json!({"action_type":"sell_set_price","sell_rarity":"epic","sell_price":4.0}),
    )
    .unwrap();
    let results = director_dispatch(
        &mut state,
        serde_json::json!({"action_type":"sell_remove_price","sell_rarity":"epic"}),
    )
    .unwrap();
    assert_eq!(results.results[0].message_type, MessageType::SystemNotice);
    assert!(state.sell_prices.is_empty());
    // 删除不存在的稀有度 → Info
    let results = director_dispatch(
        &mut state,
        serde_json::json!({"action_type":"sell_remove_price","sell_rarity":"epic"}),
    )
    .unwrap();
    assert_eq!(results.results[0].message_type, MessageType::Info);
}
