//! 队友系统功能集成测试

use royale_arena_backend::game::models::MessageType;
use royale_arena_backend::websocket::actions::director_action_scheduler::{
    DirectorActionParams, DirectorActionScheduler,
};
use royale_arena_backend::websocket::models::{
    GameState, Place, Player, SearchResult, SearchResultType,
};
use serde_json::{Value, json};

fn teammate_test_rules(mode: i32) -> Value {
    json!({
        "map": {"places": ["loc"], "safe_places": []},
        "player": {"max_life": 100, "max_strength": 100, "daily_life_recovery": 0, "daily_strength_recovery": 40, "search_cooldown": 30, "max_backpack_items": 6, "unarmed_damage": 5},
        "action_costs": {"move": 5, "search": 5, "pick": 0, "attack": 0, "equip": 0, "use": 0, "throw": 0, "deliver": 10},
        "rest_mode": {"life_recovery": 25, "strength_recovery": 1000, "max_moves": 1},
        "death_item_disposition": "killer_takes_loot",
        "teammate_behavior": mode,
        "items_config": {"rarity_levels": [], "items": {}, "upgrade_recipes": {}}
    })
}

fn build_empty_game_state(mode: i32) -> GameState {
    GameState::new("game1".to_string(), teammate_test_rules(mode))
}

fn extract_mode_from_rules(state: &GameState) -> i32 {
    state.rules_config
        .get("teammate_behavior")
        .and_then(|v| v.as_i64())
        .unwrap_or(0) as i32
}

#[test]
fn set_teammate_behavior_updates_rule_engine_and_rules_config() {
    let mut state = build_empty_game_state(0);
    let params = DirectorActionParams {
        teammate_behavior: Some(11),
        timestamp: None, place_name: None, is_destroyed: None, places: None,
        weather: None, player_id: None, life: None, strength: None, coins: None,
        target_place: None, action_type: None, rest_enabled: None,
        target_type: None, item_name: None, message: None,
        airdrops: None, deletions: None, clear_all: None,
        shop_listing_id: None, price: None, quantity: None,
    };
    let results = DirectorActionScheduler::dispatch(&mut state, "set_teammate_behavior", params)
        .expect("dispatch should succeed");

    assert_eq!(state.rule_engine.teammate_behavior.mode, 11);
    assert_eq!(extract_mode_from_rules(&state), 11);

    // Should produce one SystemNotice broadcasting to (empty) player list
    assert_eq!(results.results.len(), 1);
    let r = &results.results[0];
    assert_eq!(r.message_type, MessageType::SystemNotice);
    assert!(r.broadcast_to_director);
}

#[test]
fn set_teammate_behavior_zero_disables_mode() {
    let mut state = build_empty_game_state(11);
    let params = DirectorActionParams {
        teammate_behavior: Some(0),
        timestamp: None, place_name: None, is_destroyed: None, places: None,
        weather: None, player_id: None, life: None, strength: None, coins: None,
        target_place: None, action_type: None, rest_enabled: None,
        target_type: None, item_name: None, message: None,
        airdrops: None, deletions: None, clear_all: None,
        shop_listing_id: None, price: None, quantity: None,
    };
    DirectorActionScheduler::dispatch(&mut state, "set_teammate_behavior", params)
        .expect("dispatch should succeed");
    assert_eq!(state.rule_engine.teammate_behavior.mode, 0);
    assert!(!state.rule_engine.teammate_behavior.is_damage_immune());
}

#[test]
fn set_teammate_behavior_rejects_out_of_range() {
    let mut state = build_empty_game_state(0);
    for bad in [16_i32, -1, 100] {
        let params = DirectorActionParams {
            teammate_behavior: Some(bad),
            timestamp: None, place_name: None, is_destroyed: None, places: None,
            weather: None, player_id: None, life: None, strength: None, coins: None,
            target_place: None, action_type: None, rest_enabled: None,
            target_type: None, item_name: None, message: None,
            airdrops: None, deletions: None, clear_all: None,
            shop_listing_id: None, price: None, quantity: None,
        };
        let result = DirectorActionScheduler::dispatch(&mut state, "set_teammate_behavior", params);
        assert!(result.is_err(), "mode={} should be rejected", bad);
    }
}

// ============================================================================
// Task 3: Attack main-target immunity helpers
// ============================================================================

fn add_player_at(state: &mut GameState, id: &str, team: u32, loc: &str) {
    let mut p = Player::new(
        id.to_string(),
        format!("name_{}", id),
        "pw".to_string(),
        team,
        &state.rule_engine,
    );
    p.location = loc.to_string();
    p.last_search_result = Some(SearchResult {
        target_type: SearchResultType::Player,
        target_id: "target".to_string(),
        target_name: "target".to_string(),
        is_visible: true,
    });
    state
        .places
        .entry(loc.to_string())
        .or_insert_with(|| Place::new(loc.to_string()));
    state.players.insert(id.to_string(), p);
    state
        .places
        .get_mut(loc)
        .unwrap()
        .players
        .push(id.to_string());
}

fn set_last_search_to(state: &mut GameState, searcher: &str, target: &str) {
    let target_name = state
        .players
        .get(target)
        .map(|p| p.name.clone())
        .unwrap_or_default();
    let player = state.players.get_mut(searcher).unwrap();
    player.last_search_result = Some(SearchResult {
        target_type: SearchResultType::Player,
        target_id: target.to_string(),
        target_name,
        is_visible: true,
    });
}

#[test]
fn attack_teammate_with_damage_immune_returns_info() {
    let mut state = build_empty_game_state(1); // bit 1 on
    add_player_at(&mut state, "attacker", 1, "loc");
    add_player_at(&mut state, "target", 1, "loc");
    set_last_search_to(&mut state, "attacker", "target");

    let attacker_strength_before = state.players["attacker"].strength;
    let target_life_before = state.players["target"].life;

    let results = state.handle_attack_action("attacker").expect("attack ok");

    // Should return Info message
    assert_eq!(results.results.len(), 1);
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(!results.results[0].broadcast_to_director);

    // Nothing consumed, no damage applied
    assert_eq!(state.players["attacker"].strength, attacker_strength_before);
    assert_eq!(state.players["target"].life, target_life_before);
    // Search result should NOT be cleared (attack didn't happen)
    assert!(state.players["attacker"].last_search_result.is_some());
}

#[test]
fn attack_solo_player_works_even_with_mode_on() {
    let mut state = build_empty_game_state(1); // bit 1 on
    add_player_at(&mut state, "attacker", 1, "loc");
    add_player_at(&mut state, "target", 0, "loc"); // solo, team_id=0
    set_last_search_to(&mut state, "attacker", "target");

    let results = state.handle_attack_action("attacker").expect("attack ok");
    // Solo target should be damaged normally — not Info
    let has_system = results
        .results
        .iter()
        .any(|r| r.message_type == MessageType::SystemNotice);
    assert!(has_system, "attack on solo should produce SystemNotice, not Info");
    assert!(state.players["target"].life < 100);
}

#[test]
fn attack_teammate_with_mode_off_works_normally() {
    let mut state = build_empty_game_state(0); // mode off
    add_player_at(&mut state, "attacker", 1, "loc");
    add_player_at(&mut state, "target", 1, "loc");
    set_last_search_to(&mut state, "attacker", "target");

    state
        .handle_attack_action("attacker")
        .expect("attack ok");
    assert!(
        state.players["target"].life < 100,
        "damage should apply when mode is off"
    );
}
