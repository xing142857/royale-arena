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
        id.to_string(), name.to_string(), "pw".to_string(), 0, &engine,
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
