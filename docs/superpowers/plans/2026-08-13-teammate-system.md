# Teammate System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement a teammate mode for Royale Arena that lets the director enable team-aware rules (damage immunity, search filter, item transfer) between players sharing the same `team_id > 0`, while leaving solo players (team_id=0) completely unaffected.

**Architecture:** Bitmask-based config (`teammate_behavior: i32`) stored in `rules_config` JSON and parsed into `TeammateBehavior.mode`. Helpers on `TeammateBehavior` + `GameState.are_teammates` provide clean checks. Enforcement points are added in attack/splash/mine/search actions. New `transfer_item` player action + new director `set_teammate_behavior` action. Frontend gets a director config card and an actor transfer button + dialog.

**Tech Stack:**
- Backend: Rust (axum, yawc, sqlx, serde)
- Frontend: Vue 3 + TypeScript + Vite, Element Plus, Pinia
- Tests: Rust `#[test]` integration tests in `backend/tests/`

## Global Constraints

- **mode range**: `0..=15` inclusive (4-bit mask). Bit values: 1=damage immune, 2=search filter, 8=transfer enabled. Bit 4 reserved, NOT exposed in UI.
- **Team membership**: Two players are teammates iff `team_id_a == team_id_b AND team_id_a > 0`. `team_id == 0` (or `None`) means solo, never a teammate of anyone (including self).
- **Solo player guarantee**: When `mode == 0` OR the acting player has `team_id == 0`, all behavior must be byte-for-byte identical to current code.
- **Config sync**: When director sets mode, both `game_state.rules_config["teammate_behavior"]` (JsonValue) AND `game_state.rule_engine.teammate_behavior.mode` (i32) must be updated atomically.
- **Transfer cost**: Sender pays nothing. Receiver pays 5 strength. Fails cleanly if receiver has `< 5` strength, dead, or backpack full.
- **Backwards compat**: Don't break existing saves. Don't rename existing fields.
- **Existing uncommitted changes**: `backend/src/websocket/actions/player_common_actions.rs` and `backend/tests/currency_and_movement_integration.rs` have unrelated user modifications. Do NOT revert them.
- **Rust borrow discipline**: When a handler needs to mutate two different players, take `get_mut` sequentially — never hold two mutable borrows simultaneously.
- **Test commands**: Run backend tests with `cd backend && cargo test --test <test_file_name> -- --nocapture`. Run frontend lint with `cd frontend && pnpm lint`. Run frontend build with `cd frontend && pnpm build`.
- **Frontend naming**: Follow existing — `ComputedRef<T>`, Element Plus components, `useGameStateStore` for state.

---

## File Structure

**Backend (modify):**
- `backend/src/game/game_rule_engine.rs` — `TeammateBehavior` constants + helper methods
- `backend/src/websocket/models.rs` — `GameState::are_teammates`
- `backend/src/websocket/broadcaster.rs` — Expose `team_id` in player list JSON
- `backend/src/websocket/actions/director_action_scheduler.rs` — Register `set_teammate_behavior`
- `backend/src/websocket/actions/director_common_actions.rs` — `handle_set_teammate_behavior`
- `backend/src/websocket/actions/player_action_scheduler.rs` — Register `transfer_item`
- `backend/src/websocket/actions/player_common_actions.rs` — `handle_transfer_item_action` + search filter
- `backend/src/websocket/actions/player_attack_action.rs` — Main target + AOE immunity
- `backend/src/websocket/actions/player_use_action.rs` — Remote mine immunity

**Backend (create tests):**
- `backend/tests/teammate_system_integration.rs` — All integration tests for the feature

**Frontend (modify):**
- `frontend/src/types/gameStateTypes.ts` — Add `team_id?` to `ActorPlayer`
- `frontend/src/stores/gameState.ts` — Add `setTeammateBehavior(mode)`
- `frontend/src/views/director/management/InGameManagement.vue` — Mount `TeammateModeCard`
- `frontend/src/views/actor/components/InventoryPanel.vue` — Add transfer button + dialog mount

**Frontend (create):**
- `frontend/src/views/director/components/TeammateModeCard.vue` — Director config UI
- `frontend/src/views/actor/components/TransferTeammateDialog.vue` — Teammate picker

---

## Task 1: Backend TeammateBehavior Helpers + GameState::are_teammates

**Files:**
- Modify: `backend/src/game/game_rule_engine.rs` (lines ~110, ~158-162)
- Modify: `backend/src/websocket/models.rs` (add method on `impl GameState`)
- Test: `backend/src/game/game_rule_engine.rs` (mod tests, append) + `backend/src/websocket/models.rs` (mod tests, append if exists, else create)

**Interfaces:**
- Consumes: existing `TeammateBehavior { mode: i32 }` struct
- Produces:
  - `TeammateBehavior::BIT_DAMAGE_IMMUNE: i32 = 1`
  - `TeammateBehavior::BIT_SEARCH_FILTER: i32 = 2`
  - `TeammateBehavior::BIT_TRANSFER: i32 = 8`
  - `TeammateBehavior::is_damage_immune(&self) -> bool`
  - `TeammateBehavior::is_search_filtered(&self) -> bool`
  - `TeammateBehavior::is_transfer_enabled(&self) -> bool`
  - `GameState::are_teammates(&self, a_id: &str, b_id: &str) -> bool`

- [ ] **Step 1: Write failing test for TeammateBehavior helpers**

Append to `backend/src/game/game_rule_engine.rs` `mod tests` (find the existing one or add new):

```rust
#[test]
fn teammate_behavior_bit_helpers() {
    use crate::game::game_rule_engine::TeammateBehavior;

    let b = TeammateBehavior { mode: 0 };
    assert!(!b.is_damage_immune());
    assert!(!b.is_search_filtered());
    assert!(!b.is_transfer_enabled());

    let b = TeammateBehavior { mode: 1 };
    assert!(b.is_damage_immune());
    assert!(!b.is_search_filtered());
    assert!(!b.is_transfer_enabled());

    let b = TeammateBehavior { mode: 2 };
    assert!(!b.is_damage_immune());
    assert!(b.is_search_filtered());
    assert!(!b.is_transfer_enabled());

    let b = TeammateBehavior { mode: 8 };
    assert!(!b.is_damage_immune());
    assert!(!b.is_search_filtered());
    assert!(b.is_transfer_enabled());

    let b = TeammateBehavior { mode: 11 }; // 1 + 2 + 8
    assert!(b.is_damage_immune());
    assert!(b.is_search_filtered());
    assert!(b.is_transfer_enabled());

    let b = TeammateBehavior { mode: 9 }; // 1 + 8
    assert!(b.is_damage_immune());
    assert!(!b.is_search_filtered());
    assert!(b.is_transfer_enabled());

    // Bit 4 (value 4) is NOT exposed — helper methods ignore it
    let b = TeammateBehavior { mode: 4 };
    assert!(!b.is_damage_immune());
    assert!(!b.is_search_filtered());
    assert!(!b.is_transfer_enabled());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd backend && cargo test --lib teammate_behavior_bit_helpers -- --nocapture`
Expected: FAIL — methods `is_damage_immune` etc. don't exist on `TeammateBehavior`.

- [ ] **Step 3: Add constants + helpers to TeammateBehavior**

In `backend/src/game/game_rule_engine.rs`, replace the existing `TeammateBehavior` definition:

```rust
/// 队友行为配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeammateBehavior {
    pub mode: i32,
}

impl TeammateBehavior {
    pub const BIT_DAMAGE_IMMUNE: i32 = 1;
    pub const BIT_SEARCH_FILTER: i32 = 2;
    pub const BIT_TRANSFER: i32 = 8;

    pub fn is_damage_immune(&self) -> bool {
        self.mode & Self::BIT_DAMAGE_IMMUNE != 0
    }
    pub fn is_search_filtered(&self) -> bool {
        self.mode & Self::BIT_SEARCH_FILTER != 0
    }
    pub fn is_transfer_enabled(&self) -> bool {
        self.mode & Self::BIT_TRANSFER != 0
    }
}
```

Also remove the `#[allow(dead_code)]` attribute on the `teammate_behavior` field of `GameRuleEngine` (around line 110-111):

```rust
// Before:
#[allow(dead_code)]
pub teammate_behavior: TeammateBehavior, // TODO: 实现队友行为规则

// After:
pub teammate_behavior: TeammateBehavior,
```

- [ ] **Step 4: Run helper test to verify pass**

Run: `cd backend && cargo test --lib teammate_behavior_bit_helpers -- --nocapture`
Expected: PASS

- [ ] **Step 5: Write failing test for GameState::are_teammates**

In `backend/src/websocket/models.rs`, find or create a `#[cfg(test)] mod tests` at the bottom. Append:

```rust
#[cfg(test)]
mod are_teammates_tests {
    use super::*;
    use crate::game::game_rule_engine::GameRuleEngine;

    fn build_state_with_players(pairs: &[(&str, u32)]) -> GameState {
        let rules_json = r#"{
            "map": {"places": ["loc"], "safe_places": []},
            "player": {"max_life": 100, "max_strength": 100, "daily_life_recovery": 0, "daily_strength_recovery": 40, "search_cooldown": 30, "max_backpack_items": 6, "unarmed_damage": 5},
            "action_costs": {"move": 5, "search": 5, "pick": 0, "attack": 0, "equip": 0, "use": 0, "throw": 0, "deliver": 10},
            "rest_mode": {"life_recovery": 25, "strength_recovery": 1000, "max_moves": 1},
            "death_item_disposition": "killer_takes_loot",
            "teammate_behavior": 0,
            "items_config": {"rarity_levels": [], "items": {}, "upgrade_recipes": {}}
        }"#;
        let engine = GameRuleEngine::from_json(rules_json).unwrap();
        let mut state = GameState::new("game1".to_string(), serde_json::json!({}));
        state.rule_engine = engine;
        for (id, team) in pairs {
            let mut p = Player::new(
                id.to_string(),
                format!("name_{}", id),
                "pw".to_string(),
                *team,
                &state.rule_engine,
            );
            p.location = "loc".to_string();
            state.players.insert(id.to_string(), p);
        }
        state
    }

    #[test]
    fn same_positive_team_id_are_teammates() {
        let s = build_state_with_players(&[("a", 1), ("b", 1)]);
        assert!(s.are_teammates("a", "b"));
        assert!(s.are_teammates("b", "a")); // symmetric
    }

    #[test]
    fn different_team_ids_not_teammates() {
        let s = build_state_with_players(&[("a", 1), ("b", 2)]);
        assert!(!s.are_teammates("a", "b"));
    }

    #[test]
    fn zero_team_id_never_teammate_even_if_equal() {
        let s = build_state_with_players(&[("a", 0), ("b", 0)]);
        assert!(!s.are_teammates("a", "b"));
    }

    #[test]
    fn self_is_not_own_teammate() {
        let s = build_state_with_players(&[("a", 1)]);
        assert!(!s.are_teammates("a", "a"));
    }

    #[test]
    fn unknown_player_id_not_teammate() {
        let s = build_state_with_players(&[("a", 1)]);
        assert!(!s.are_teammates("a", "ghost"));
        assert!(!s.are_teammates("ghost", "a"));
    }
}
```

- [ ] **Step 6: Run test to verify it fails**

Run: `cd backend && cargo test --lib are_teammates -- --nocapture`
Expected: FAIL — method `are_teammates` doesn't exist on `GameState`.

- [ ] **Step 7: Implement are_teammates on GameState**

In `backend/src/websocket/models.rs`, find `impl GameState {` and add this method:

```rust
/// 判断两个玩家是否为同队队友
/// 规则：team_id 必须相等且 > 0；自己不算自己的队友；任一玩家不存在返回 false
pub fn are_teammates(&self, a_id: &str, b_id: &str) -> bool {
    if a_id == b_id {
        return false;
    }
    let (a, b) = match (self.players.get(a_id), self.players.get(b_id)) {
        (Some(a), Some(b)) => (a, b),
        _ => return false,
    };
    match (a.team_id, b.team_id) {
        (Some(ta), Some(tb)) => ta > 0 && ta == tb,
        _ => false,
    }
}
```

- [ ] **Step 8: Run all teammate tests to verify pass**

Run: `cd backend && cargo test --lib are_teammates teammate_behavior_bit_helpers -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 9: Run full test suite to verify no regressions**

Run: `cd backend && cargo test -- --nocapture`
Expected: All pre-existing tests still pass.

- [ ] **Step 10: Commit**

```bash
cd backend && git add src/game/game_rule_engine.rs src/websocket/models.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): add TeammateBehavior helpers and GameState::are_teammates"
```

---

## Task 2: Director Action `set_teammate_behavior`

**Files:**
- Modify: `backend/src/websocket/actions/director_action_scheduler.rs` (add field + dispatch case)
- Modify: `backend/src/websocket/actions/director_common_actions.rs` (add handler)
- Test: `backend/tests/teammate_system_integration.rs` (create new test file)

**Interfaces:**
- Consumes: Task 1 helpers (`is_damage_immune` etc.)
- Produces:
  - Director action type: `"set_teammate_behavior"`
  - Required param: `teammate_behavior: i32` (in `DirectorActionParams`)
  - Behavior: updates both `rules_config` JSON and `rule_engine.teammate_behavior.mode`, broadcasts SystemNotice to all players

- [ ] **Step 1: Create failing test file**

Create `backend/tests/teammate_system_integration.rs`:

```rust
//! 队友系统功能集成测试

use royale_arena_backend::game::models::MessageType;
use royale_arena_backend::websocket::actions::director_action_scheduler::{
    DirectorActionParams, DirectorActionScheduler,
};
use royale_arena_backend::websocket::models::GameState;
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
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test --test teammate_system_integration -- --nocapture`
Expected: FAIL — `DirectorActionParams` doesn't have `teammate_behavior` field; dispatch doesn't know `"set_teammate_behavior"`.

- [ ] **Step 3: Add teammate_behavior field to DirectorActionParams**

In `backend/src/websocket/actions/director_action_scheduler.rs`, append to the `DirectorActionParams` struct (after `quantity`):

```rust
    /// 队友行为位掩码（0..=15）
    pub teammate_behavior: Option<i32>,
```

- [ ] **Step 4: Add dispatch case**

In the same file's `dispatch` function, before the `_ => Err(...)` line, add:

```rust
            "set_teammate_behavior" => {
                let mode = action_params
                    .teammate_behavior
                    .ok_or_else(|| "Missing teammate_behavior parameter".to_string())?;
                if !(0..=15).contains(&mode) {
                    return Err(format!(
                        "teammate_behavior must be in 0..=15, got {}",
                        mode
                    ));
                }
                game_state.handle_set_teammate_behavior(mode)
            }
```

- [ ] **Step 5: Implement the handler**

In `backend/src/websocket/actions/director_common_actions.rs`, add this method to the `impl GameState` block (find an existing handler as a placement reference, e.g., `handle_shop_delist_item`):

```rust
    /// 导演更新队友行为配置
    pub fn handle_set_teammate_behavior(&mut self, mode: i32) -> Result<ActionResults, String> {
        // 1. 写回 rule_engine（让本轮后续 action 立刻生效）
        self.rule_engine.teammate_behavior.mode = mode;

        // 2. 同步到 rules_config JSON（让前端 state_update 看到新值、让存档保留）
        if let Some(obj) = self.rules_config.as_object_mut() {
            obj.insert(
                "teammate_behavior".to_string(),
                serde_json::json!(mode),
            );
        }

        // 3. 全员广播 SystemNotice
        let all_player_ids: Vec<String> = self.players.keys().cloned().collect();
        Ok(ActionResult::new_system_message(
            serde_json::json!({ "teammate_behavior": mode }),
            all_player_ids,
            format!("导演已更新队友行为规则（位掩码={}）", mode),
            true,
        )
        .as_results())
    }
```

- [ ] **Step 6: Run the new tests**

Run: `cd backend && cargo test --test teammate_system_integration -- --nocapture`
Expected: All 3 tests PASS.

- [ ] **Step 7: Run full test suite to verify no regressions**

Run: `cd backend && cargo test -- --nocapture`
Expected: All pre-existing tests still pass.

- [ ] **Step 8: Commit**

```bash
cd backend && git add src/websocket/actions/director_action_scheduler.rs src/websocket/actions/director_common_actions.rs tests/teammate_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): add director set_teammate_behavior action"
```

---

## Task 3: Attack Main Target Immunity

**Files:**
- Modify: `backend/src/websocket/actions/player_attack_action.rs` (insert check after target_alive validation, ~line 80)
- Test: `backend/tests/teammate_system_integration.rs` (append)

**Interfaces:**
- Consumes: Task 1 `is_damage_immune` + `are_teammates`
- Produces: When attacker and target are teammates AND bit 1 set, returns Info message without applying damage or consuming resources.

- [ ] **Step 1: Write failing test**

Append to `backend/tests/teammate_system_integration.rs`:

```rust
use royale_arena_backend::websocket::actions::player_action_scheduler::ActionParams;
use royale_arena_backend::websocket::models::{Place, Player, SearchResultType, SearchResult};

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
        target_id: "target".to_string(), // will override per test
        target_name: "target".to_string(),
        is_visible: true,
    });
    state.places.entry(loc.to_string()).or_insert_with(|| Place::new(loc.to_string()));
    state.players.insert(id.to_string(), p);
    state.places.get_mut(loc).unwrap().players.push(id.to_string());
}

fn set_last_search_to(state: &mut GameState, searcher: &str, target: &str) {
    let target_name = state.players.get(target).map(|p| p.name.clone()).unwrap_or_default();
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
    let has_system = results.results.iter().any(|r| r.message_type == MessageType::SystemNotice);
    assert!(has_system, "attack on solo should produce SystemNotice, not Info");
    assert!(state.players["target"].life < 100);
}

#[test]
fn attack_teammate_with_mode_off_works_normally() {
    let mut state = build_empty_game_state(0); // mode off
    add_player_at(&mut state, "attacker", 1, "loc");
    add_player_at(&mut state, "target", 1, "loc");
    set_last_search_to(&mut state, "attacker", "target");

    state.handle_attack_action("attacker").expect("attack ok");
    assert!(state.players["target"].life < 100, "damage should apply when mode is off");
}
```

**Note:** Verify `SearchResult` struct field names by grepping `pub struct SearchResult` in `backend/src/websocket/models.rs` before writing the test. Adjust field initialization to match.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test --test teammate_system_integration attack_teammate attack_solo_player -- --nocapture`
Expected: First test FAILs (teammate takes damage). The other two should PASS already (correct baseline behavior).

- [ ] **Step 3: Implement immunity check in attack action**

In `backend/src/websocket/actions/player_attack_action.rs`, find the block:

```rust
        // 验证目标玩家是否已死亡
        if !target_player_alive {
            let action_result = ActionResult::new_info_message(
                serde_json::json!({}),
                vec![player_id.to_string()],
                failed_message,
                false,
            );
            return Ok(action_result.as_results());
        }
```

Immediately after that block (before `// 根据是否装备武器计算伤害及附加效果`), insert:

```rust
        // 队友伤害免疫（位 1）：同队队友之间不能造成伤害
        if self.rule_engine.teammate_behavior.is_damage_immune()
            && self.are_teammates(player_id, &target_player_id)
        {
            let action_result = ActionResult::new_info_message(
                serde_json::json!({}),
                vec![player_id.to_string()],
                "队友伤害免疫已开启，无法攻击队友".to_string(),
                false,
            );
            return Ok(action_result.as_results());
        }
```

- [ ] **Step 4: Run attack immunity tests**

Run: `cd backend && cargo test --test teammate_system_integration attack_teammate attack_solo_player attack_teammate_with_mode_off -- --nocapture`
Expected: All 3 tests PASS.

- [ ] **Step 5: Run full test suite**

Run: `cd backend && cargo test -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 6: Commit**

```bash
cd backend && git add src/websocket/actions/player_attack_action.rs tests/teammate_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): block attack on teammates when damage immune bit set"
```

---

## Task 4: AOE Splash + Remote Mine Immunity

**Files:**
- Modify: `backend/src/websocket/actions/player_attack_action.rs` (`aoe_targets` filter_map)
- Modify: `backend/src/websocket/actions/player_use_action.rs` (remote mine occupant_ids)
- Test: `backend/tests/teammate_system_integration.rs` (append)

**Interfaces:**
- Consumes: Task 1 helpers
- Produces: Teammates are filtered out of splash/mine candidate lists; non-teammates still take damage.

- [ ] **Step 1: Write failing tests**

Append to `backend/tests/teammate_system_integration.rs`:

```rust
use royale_arena_backend::game::game_rule_engine::{
    ItemType, UtilityProperties, WeaponProperties,
};

fn equip_legendary_weapon(state: &mut GameState, player_id: &str) {
    // 橙武带 aoe_damage=40, bleed_damage=10
    let weapon = royale_arena_backend::websocket::models::Item {
        id: "w1".to_string(),
        name: "[橙]自然之力.晓".to_string(),
        item_type: ItemType::Weapon(WeaponProperties {
            damage: 50,
            uses: Some(5),
            votes: 0,
            aoe_damage: Some(40),
            bleed_damage: Some(10),
        }),
    };
    state.players.get_mut(player_id).unwrap().equipped_weapon = Some(weapon);
}

#[test]
fn attack_splash_skips_teammates() {
    let mut state = build_empty_game_state(1); // bit 1 on
    add_player_at(&mut state, "attacker", 1, "loc");
    add_player_at(&mut state, "main_target", 2, "loc");   // 不同队，主目标
    add_player_at(&mut state, "teammate", 1, "loc");      // 同队，应被溅射过滤
    equip_legendary_weapon(&mut state, "attacker");
    set_last_search_to(&mut state, "attacker", "main_target");

    let main_life_before = state.players["main_target"].life;
    let team_life_before = state.players["teammate"].life;

    let results = state.handle_attack_action("attacker").expect("attack ok");

    // main_target 应受伤（武器 + 溅射）
    assert!(state.players["main_target"].life < main_life_before);
    // teammate 生命值不变
    assert_eq!(state.players["teammate"].life, team_life_before);

    // 攻击者收到的消息里 aoe_hits 不含 teammate
    let attacker_msg = results.results.iter()
        .find(|r| r.broadcast_players.iter().any(|p| p == "attacker"))
        .map(|r| r.log_message.as_str())
        .unwrap_or("");
    assert!(!attacker_msg.contains("name_teammate"), "teammate name should not appear in attacker log");
}

#[test]
fn remote_mine_skips_teammates() {
    let mut state = build_empty_game_state(1);
    add_player_at(&mut state, "miner", 1, "loc");
    add_player_at(&mut state, "enemy", 2, "loc");
    add_player_at(&mut state, "friend", 1, "loc");

    // 给 miner 装备遥控地雷
    let mine = royale_arena_backend::websocket::models::Item {
        id: "m1".to_string(),
        name: "[炸]遥控地雷".to_string(),
        item_type: ItemType::Utility(UtilityProperties {
            category: "utility_trap".to_string(),
            damage: Some(30),
            uses: Some(1),
            votes: Some(0),
            uses_night: None,
            targets: None,
        }),
    };
    state.players.get_mut("miner").unwrap().inventory.push(mine);

    let enemy_before = state.players["enemy"].life;
    let friend_before = state.players["friend"].life;

    let params = ActionParams {
        target_place: None, place_name: None, item_id: Some("m1".to_string()),
        slot_type: None, target_player_id: None, target_player_ids: None,
        target_item_name: None, message: None, shop_buy_items: None,
    };
    // 直接走 handle_use_action（绕过 scheduler 校验，简化测试）
    state.handle_use_action("miner", "m1", &params).expect("use ok");

    assert!(state.players["enemy"].life < enemy_before, "enemy takes mine damage");
    assert_eq!(state.players["friend"].life, friend_before, "friend unharmed");
}
```

**Note:** Before writing, verify `Item` struct fields and `UtilityProperties` fields by grepping `pub struct Item` and `pub struct UtilityProperties` in `backend/src/game/game_rule_engine.rs`. Adjust field initialization to match. Drop optional fields that don't exist.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test --test teammate_system_integration attack_splash_skips_teammates remote_mine_skips_teammates -- --nocapture`
Expected: FAIL — teammates take damage currently.

- [ ] **Step 3: Filter teammates in AOE splash**

In `backend/src/websocket/actions/player_attack_action.rs`, find the `aoe_targets` filter_map block (around line 110-136). Add the teammate check inside `filter_map`:

```rust
        let aoe_targets: Vec<String> = if weapon_aoe_damage.is_some() {
            if let Some(place) = self.places.get(&player_location) {
                place
                    .players
                    .iter()
                    .filter_map(|other_id| {
                        if other_id.as_str() == player_id || other_id == &target_player_id {
                            return None;
                        }
                        // 队友溅射免疫（位 1）
                        if self.rule_engine.teammate_behavior.is_damage_immune()
                            && self.are_teammates(player_id, other_id)
                        {
                            return None;
                        }
                        let is_alive = self
                            .players
                            .get(other_id)
                            .map(|player| player.is_alive)
                            .unwrap_or(false);
                        if is_alive {
                            Some(other_id.clone())
                        } else {
                            None
                        }
                    })
                    .collect()
            } else {
                Vec::new()
            }
        } else {
            Vec::new()
        };
```

- [ ] **Step 4: Filter teammates in remote mine**

In `backend/src/websocket/actions/player_use_action.rs`, find `handle_utility_trap_remote_mine`. Replace the line that builds `occupant_ids`:

```rust
        // Before (current code):
        // let occupant_ids = self
        //     .places
        //     .get(player_location)
        //     .map(|place| place.players.clone())
        //     .unwrap_or_default();

        // After:
        let occupant_ids: Vec<String> = self
            .places
            .get(player_location)
            .map(|place| place.players.clone())
            .unwrap_or_default()
            .into_iter()
            .filter(|target_id| {
                if target_id == player_id {
                    return false;
                }
                // 队友免疫（位 1）
                if self.rule_engine.teammate_behavior.is_damage_immune()
                    && self.are_teammates(player_id, target_id)
                {
                    return false;
                }
                true
            })
            .collect();
```

- [ ] **Step 5: Run the new tests**

Run: `cd backend && cargo test --test teammate_system_integration attack_splash_skips_teammates remote_mine_skips_teammates -- --nocapture`
Expected: Both tests PASS.

- [ ] **Step 6: Run full test suite**

Run: `cd backend && cargo test -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 7: Commit**

```bash
cd backend && git add src/websocket/actions/player_attack_action.rs src/websocket/actions/player_use_action.rs tests/teammate_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): filter teammates out of splash and remote mine damage"
```

---

## Task 5: Search Filter

**Files:**
- Modify: `backend/src/websocket/actions/game_state_common.rs` (`collect_search_targets`, line ~360-386)
- Test: `backend/tests/teammate_system_integration.rs` (append)

**Interfaces:**
- Consumes: Task 1 `is_search_filtered` + `are_teammates`
- Produces: When bit 2 set, teammates are excluded from search candidates.

- [ ] **Step 1: Write failing test**

Append to `backend/tests/teammate_system_integration.rs`:

```rust
#[test]
fn search_filters_out_teammates_when_bit2_set() {
    let mut state = build_empty_game_state(2); // bit 2 on
    add_player_at(&mut state, "searcher", 1, "loc");
    add_player_at(&mut state, "enemy", 2, "loc");
    add_player_at(&mut state, "friend", 1, "loc");

    let targets = state.collect_search_targets("searcher");

    let target_ids: Vec<String> = targets.iter().filter_map(|t| match t {
        royale_arena_backend::websocket::models::SearchTarget::Player(id) => Some(id.clone()),
        _ => None,
    }).collect();

    assert!(target_ids.iter().any(|id| id == "enemy"), "enemy should be searchable");
    assert!(!target_ids.iter().any(|id| id == "friend"), "teammate should be filtered");
}

#[test]
fn search_keeps_teammates_when_bit2_off() {
    let mut state = build_empty_game_state(0); // mode off
    add_player_at(&mut state, "searcher", 1, "loc");
    add_player_at(&mut state, "friend", 1, "loc");

    let targets = state.collect_search_targets("searcher");
    let has_friend = targets.iter().any(|t| matches!(t, royale_arena_backend::websocket::models::SearchTarget::Player(id) if id == "friend"));
    assert!(has_friend, "teammate searchable when mode off");
}

#[test]
fn search_keeps_teammates_for_solo_searcher_even_when_bit2_on() {
    let mut state = build_empty_game_state(2);
    add_player_at(&mut state, "solo", 0, "loc"); // 散人
    add_player_at(&mut state, "anyone", 1, "loc");

    let targets = state.collect_search_targets("solo");
    // solo has no teammates, so anyone should be searchable
    let has_anyone = targets.iter().any(|t| matches!(t, royale_arena_backend::websocket::models::SearchTarget::Player(id) if id == "anyone"));
    assert!(has_anyone);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test --test teammate_system_integration search_filters search_keeps_teammates_when_bit2_off search_keeps_teammates_for_solo -- --nocapture`
Expected: First test FAILs (teammate appears in candidates).

- [ ] **Step 3: Add filter to collect_search_targets**

In `backend/src/websocket/actions/game_state_common.rs`, find `pub fn collect_search_targets`. Modify the inner loop where players are added:

```rust
        if let Some(place) = self.places.get(player_location) {
            // 添加其他玩家到搜索目标
            for other_player_id in &place.players {
                if other_player_id != player_id {
                    if let Some(other_player) = self.players.get(other_player_id) {
                        if !other_player.is_alive {
                            continue;
                        }
                        // 队友搜索过滤（位 2）
                        if self.rule_engine.teammate_behavior.is_search_filtered()
                            && self.are_teammates(player_id, other_player_id)
                        {
                            continue;
                        }
                        targets.push(SearchTarget::Player(other_player_id.clone()));
                    }
                }
            }
            // 添加物品到搜索目标（保持原样）
            for item in &place.items {
                targets.push(SearchTarget::Item(item.id.clone()));
            }
        }
```

- [ ] **Step 4: Run search tests**

Run: `cd backend && cargo test --test teammate_system_integration search_filters search_keeps_teammates_when_bit2_off search_keeps_teammates_for_solo -- --nocapture`
Expected: All 3 PASS.

- [ ] **Step 5: Run full test suite**

Run: `cd backend && cargo test -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 6: Commit**

```bash
cd backend && git add src/websocket/actions/game_state_common.rs tests/teammate_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): filter teammates from search candidates when bit 2 set"
```

---

## Task 6: Transfer Item Action

**Files:**
- Modify: `backend/src/websocket/actions/player_action_scheduler.rs` (register `transfer_item`)
- Modify: `backend/src/websocket/actions/player_common_actions.rs` (add handler)
- Test: `backend/tests/teammate_system_integration.rs` (append)

**Interfaces:**
- Consumes: Task 1 `is_transfer_enabled` + `are_teammates`; existing `ActionParams.item_id` + `ActionParams.target_player_id`
- Produces: New player action `"transfer_item"` with handler `handle_transfer_item_action`. Sender: no cost. Receiver: -5 strength. Validates bit 8 enabled, same team, target alive, item in inventory, receiver strength ≥ 5, receiver backpack not full.

- [ ] **Step 1: Write failing tests**

Append to `backend/tests/teammate_system_integration.rs`:

```rust
use royale_arena_backend::game::game_rule_engine::ItemType;

fn put_item_in_inventory(state: &mut GameState, player_id: &str, item_id: &str, item_name: &str) {
    let item = royale_arena_backend::websocket::models::Item {
        id: item_id.to_string(),
        name: item_name.to_string(),
        item_type: ItemType::Upgrader,
    };
    state.players.get_mut(player_id).unwrap().inventory.push(item);
}

fn transfer_params(item_id: &str, target_id: &str) -> ActionParams {
    ActionParams {
        target_place: None, place_name: None,
        item_id: Some(item_id.to_string()),
        slot_type: None,
        target_player_id: Some(target_id.to_string()),
        target_player_ids: None, target_item_name: None, message: None, shop_buy_items: None,
    }
}

#[test]
fn transfer_item_happy_path() {
    let mut state = build_empty_game_state(8); // bit 8 on
    add_player_at(&mut state, "sender", 1, "loc");
    add_player_at(&mut state, "receiver", 1, "loc");
    put_item_in_inventory(&mut state, "sender", "i1", "[橙]自然之力.晓");

    let receiver_strength_before = state.players["receiver"].strength;
    let sender_count_before = state.players["sender"].inventory.len();

    state.handle_transfer_item_action("sender", "i1", "receiver").expect("transfer ok");

    assert!(state.players["sender"].inventory.iter().all(|i| i.id != "i1"), "item removed from sender");
    assert!(state.players["receiver"].inventory.iter().any(|i| i.id == "i1"), "item added to receiver");
    assert_eq!(state.players["sender"].inventory.len(), sender_count_before - 1);
    assert_eq!(state.players["receiver"].strength, receiver_strength_before - 5);
}

#[test]
fn transfer_item_rejected_when_bit8_off() {
    let mut state = build_empty_game_state(1); // bit 1 only
    add_player_at(&mut state, "sender", 1, "loc");
    add_player_at(&mut state, "receiver", 1, "loc");
    put_item_in_inventory(&mut state, "sender", "i1", "X");

    let results = state.handle_transfer_item_action("sender", "i1", "receiver").expect("ok");
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(state.players["sender"].inventory.iter().any(|i| i.id == "i1"), "item stays");
}

#[test]
fn transfer_item_rejected_when_target_low_strength() {
    let mut state = build_empty_game_state(8);
    add_player_at(&mut state, "sender", 1, "loc");
    add_player_at(&mut state, "receiver", 1, "loc");
    state.players.get_mut("receiver").unwrap().strength = 3;
    put_item_in_inventory(&mut state, "sender", "i1", "X");

    let results = state.handle_transfer_item_action("sender", "i1", "receiver").expect("ok");
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(state.players["sender"].inventory.iter().any(|i| i.id == "i1"));
    assert_eq!(state.players["receiver"].strength, 3);
}

#[test]
fn transfer_item_rejected_when_target_dead() {
    let mut state = build_empty_game_state(8);
    add_player_at(&mut state, "sender", 1, "loc");
    add_player_at(&mut state, "receiver", 1, "loc");
    state.players.get_mut("receiver").unwrap().is_alive = false;
    put_item_in_inventory(&mut state, "sender", "i1", "X");

    let results = state.handle_transfer_item_action("sender", "i1", "receiver").expect("ok");
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(state.players["sender"].inventory.iter().any(|i| i.id == "i1"));
}

#[test]
fn transfer_item_rejected_when_target_backpack_full() {
    let mut rules = teammate_test_rules(8);
    rules["player"]["max_backpack_items"] = json!(1);
    let mut state = GameState::new("g1".to_string(), rules);
    // manually place players
    let mut s = Player::new("sender".to_string(), "ns".to_string(), "p".to_string(), 1, &state.rule_engine);
    s.location = "loc".to_string();
    state.players.insert("sender".to_string(), s);
    let mut r = Player::new("receiver".to_string(), "nr".to_string(), "p".to_string(), 1, &state.rule_engine);
    r.location = "loc".to_string();
    r.inventory.push(royale_arena_backend::websocket::models::Item {
        id: "blocker".to_string(), name: "B".to_string(),
        item_type: ItemType::Upgrader,
    });
    state.players.insert("receiver".to_string(), r);
    state.places.insert("loc".to_string(), Place::new("loc".to_string()));
    put_item_in_inventory(&mut state, "sender", "i1", "X");

    let results = state.handle_transfer_item_action("sender", "i1", "receiver").expect("ok");
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(state.players["sender"].inventory.iter().any(|i| i.id == "i1"));
}

#[test]
fn transfer_item_rejected_for_solo_player() {
    let mut state = build_empty_game_state(8);
    add_player_at(&mut state, "solo", 0, "loc"); // 散人
    add_player_at(&mut state, "anyone", 1, "loc");
    put_item_in_inventory(&mut state, "solo", "i1", "X");

    let results = state.handle_transfer_item_action("solo", "i1", "anyone").expect("ok");
    assert_eq!(results.results[0].message_type, MessageType::Info);
    assert!(state.players["solo"].inventory.iter().any(|i| i.id == "i1"));
}

#[test]
fn transfer_item_rejected_when_item_not_in_inventory() {
    let mut state = build_empty_game_state(8);
    add_player_at(&mut state, "sender", 1, "loc");
    add_player_at(&mut state, "receiver", 1, "loc");
    // No item put in inventory

    let results = state.handle_transfer_item_action("sender", "ghost_item", "receiver").expect("ok");
    assert_eq!(results.results[0].message_type, MessageType::Info);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cd backend && cargo test --test teammate_system_integration transfer_item -- --nocapture`
Expected: FAIL — `handle_transfer_item_action` doesn't exist.

- [ ] **Step 3: Register dispatch in player_action_scheduler**

In `backend/src/websocket/actions/player_action_scheduler.rs`, find the `"deliver"` dispatch case. After it, add:

```rust
            "transfer_item" => {
                let item_id = action_params
                    .item_id
                    .clone()
                    .ok_or("Missing item_id parameter".to_string())?;
                let target_player_id = action_params
                    .target_player_id
                    .clone()
                    .ok_or("Missing target_player_id parameter".to_string())?;
                return game_state.handle_transfer_item_action(
                    player_id,
                    &item_id,
                    &target_player_id,
                );
            }
```

- [ ] **Step 4: Implement the handler**

In `backend/src/websocket/actions/player_common_actions.rs`, add this method to `impl GameState`:

```rust
    /// 处理道具转移行动（队友模式位 8）
    /// 发起方免费，接收方体力 -5
    pub fn handle_transfer_item_action(
        &mut self,
        sender_id: &str,
        item_id: &str,
        target_player_id: &str,
    ) -> Result<ActionResults, String> {
        let info_message = |message: String| -> ActionResults {
            ActionResult::new_info_message(
                serde_json::json!({}),
                vec![sender_id.to_string()],
                message,
                false,
            )
            .as_results()
        };

        // 1. 位 8 必须开
        if !self.rule_engine.teammate_behavior.is_transfer_enabled() {
            return Ok(info_message("队友物品转移未开启".to_string()));
        }
        // 2. 必须是同队
        if !self.are_teammates(sender_id, target_player_id) {
            return Ok(info_message("目标不是你的队友".to_string()));
        }
        // 3. 接收方必须存活
        let target_alive = self
            .players
            .get(target_player_id)
            .map(|p| p.is_alive)
            .unwrap_or(false);
        if !target_alive {
            return Ok(info_message("对方已阵亡，无法接收".to_string()));
        }
        // 4. 物品必须在背包
        let item = {
            let sender = self.players.get(sender_id).ok_or("Sender not found")?;
            sender.inventory.iter().find(|i| i.id == item_id).cloned()
        };
        let item = match item {
            Some(it) => it,
            None => return Ok(info_message("物品不在背包中".to_string())),
        };
        // 5. 接收方体力 ≥ 5
        let target_strength = self
            .players
            .get(target_player_id)
            .map(|p| p.strength)
            .unwrap_or(0);
        if target_strength < 5 {
            return Ok(info_message("对方体力不足，无法接收".to_string()));
        }
        // 6. 接收方背包未满
        let max = self.rule_engine.player_config.max_backpack_items as usize;
        let target_count = self
            .players
            .get(target_player_id)
            .map(|p| p.get_total_item_count())
            .unwrap_or(0);
        if target_count >= max {
            return Ok(info_message("对方背包已满，无法接收".to_string()));
        }

        // 应用：sender 移除、target 加入并扣体力
        let sender_name = self.players.get(sender_id).unwrap().name.clone();
        let target_name = self.players.get(target_player_id).unwrap().name.clone();
        let item_name = item.name.clone();

        self.players
            .get_mut(sender_id)
            .unwrap()
            .inventory
            .retain(|i| i.id != item_id);
        let target = self.players.get_mut(target_player_id).unwrap();
        target.inventory.push(item);
        target.strength -= 5;
        let target_strength_after = target.strength;
        let target_life = target.life;

        let sender_msg = format!("你将 {} 转移给了 {}", item_name, target_name);
        let target_msg = format!("队友 {} 将 {} 转移给你，体力 -5", sender_name, item_name);

        let sender_data = serde_json::json!({
            "item_name": item_name,
            "target": target_name,
        });
        let target_data = serde_json::json!({
            "item_name": item_name,
            "sender": sender_name,
            "strength": target_strength_after,
            "life": target_life,
        });

        Ok(ActionResults {
            results: vec![
                ActionResult::new_system_message(
                    sender_data,
                    vec![sender_id.to_string()],
                    sender_msg,
                    true,
                ),
                ActionResult::new_system_message(
                    target_data,
                    vec![target_player_id.to_string()],
                    target_msg,
                    true,
                ),
            ],
        })
    }
```

- [ ] **Step 5: Run transfer tests**

Run: `cd backend && cargo test --test teammate_system_integration transfer_item -- --nocapture`
Expected: All 7 transfer tests PASS.

- [ ] **Step 6: Run full test suite**

Run: `cd backend && cargo test -- --nocapture`
Expected: All tests PASS.

- [ ] **Step 7: Commit**

```bash
cd backend && git add src/websocket/actions/player_action_scheduler.rs src/websocket/actions/player_common_actions.rs tests/teammate_system_integration.rs
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): add transfer_item player action"
```

---

## Task 7: Expose team_id in Player List JSON

**Files:**
- Modify: `backend/src/websocket/broadcaster.rs` (`to_player_client_json_for_other_players`)
- Modify: `frontend/src/types/gameStateTypes.ts` (add `team_id?` to `ActorPlayer`)
- Test: `backend/tests/teammate_system_integration.rs` (append)

**Interfaces:**
- Consumes: existing `Player.team_id` field
- Produces: Player list JSON for actor view now includes `"team_id": number | null`. TS `ActorPlayer` has `team_id?: number`.

- [ ] **Step 1: Write failing backend test**

Append to `backend/tests/teammate_system_integration.rs`:

```rust
#[test]
fn player_list_json_includes_team_id() {
    let mut state = build_empty_game_state(0);
    add_player_at(&mut state, "p1", 5, "loc");
    let player = state.players.get("p1").unwrap();
    let json = royale_arena_backend::websocket::broadcaster::Player::to_player_client_json_for_other_players(player);
    assert!(json.get("team_id").is_some(), "team_id must be present in player list JSON");
    assert_eq!(json["team_id"].as_i64(), Some(5));
}
```

Note: if `Player::to_player_client_json_for_other_players` is not exposed as a free function, adjust the test to construct a Player directly. The function is an impl method on Player, callable as `player.to_player_client_json_for_other_players()` — adjust accordingly:

```rust
#[test]
fn player_list_json_includes_team_id() {
    use royale_arena_backend::game::game_rule_engine::GameRuleEngine;
    let rules_json = teammate_test_rules(0).to_string();
    let engine = GameRuleEngine::from_json(&rules_json).unwrap();
    let player = Player::new("p1".to_string(), "n".to_string(), "p".to_string(), 7, &engine);
    let json = player.to_player_client_json_for_other_players();
    assert!(json.get("team_id").is_some());
    assert_eq!(json["team_id"].as_i64(), Some(7));
}
```

Use whichever version compiles — both invoke the same method.

- [ ] **Step 2: Run test to verify it fails**

Run: `cd backend && cargo test --test teammate_system_integration player_list_json_includes_team_id -- --nocapture`
Expected: FAIL — `team_id` not in JSON.

- [ ] **Step 3: Update to_player_client_json_for_other_players**

In `backend/src/websocket/broadcaster.rs`, find the method (around line 130-135) and replace:

```rust
    pub fn to_player_client_json_for_other_players(&self) -> JsonValue {
        json!({
            "id": self.id,
            "name": self.name,
            "team_id": self.team_id,
        })
    }
```

- [ ] **Step 4: Run backend test**

Run: `cd backend && cargo test --test teammate_system_integration player_list_json_includes_team_id -- --nocapture`
Expected: PASS.

- [ ] **Step 5: Update frontend ActorPlayer type**

In `frontend/src/types/gameStateTypes.ts`, find the `ActorPlayer` type/interface and add `team_id?: number`:

```ts
export interface ActorPlayer {
  id: string
  name: string
  team_id?: number
}
```

(Verify the actual interface name by grepping `ActorPlayer` in the file first.)

- [ ] **Step 6: Verify frontend builds**

Run: `cd frontend && pnpm build`
Expected: Build succeeds.

- [ ] **Step 7: Commit**

```bash
cd backend && git add src/websocket/broadcaster.rs tests/teammate_system_integration.rs
cd ../frontend && git add src/types/gameStateTypes.ts
cd ..
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): expose team_id in actor player list JSON"
```

---

## Task 8: Frontend — Director TeammateModeCard

**Files:**
- Modify: `frontend/src/stores/gameState.ts` (add `setTeammateBehavior`)
- Create: `frontend/src/views/director/components/TeammateModeCard.vue`
- Modify: `frontend/src/views/director/management/InGameManagement.vue` (mount card)

**Interfaces:**
- Consumes: `store.globalState.rules_config.teammate_behavior` (number, may be undefined → default 0)
- Produces: Calls `store.setTeammateBehavior(mode: number)` on toggle/checkbox change.

- [ ] **Step 1: Add setTeammateBehavior to gameState store**

In `frontend/src/stores/gameState.ts`, find the existing `// 商店上架物品` block (around line 316). Before it (or near other director actions), add:

```ts
  // 设置队友行为位掩码（0..=15）
  const setTeammateBehavior = (mode: number) => {
    sendDirectorAction('set_teammate_behavior', { teammate_behavior: mode })
  }
```

And in the `return { ... }` block at the bottom of the store, add `setTeammateBehavior,` next to `shopBuy,`.

- [ ] **Step 2: Create TeammateModeCard.vue**

Create `frontend/src/views/director/components/TeammateModeCard.vue`:

```vue
<template>
  <el-card class="teammate-mode-card">
    <template #header>
      <div class="card-header">
        <h4>队友模式</h4>
        <el-switch v-model="masterOn" active-text="总开关" />
      </div>
    </template>

    <div class="bits" :class="{ disabled: !masterOn }">
      <el-checkbox v-model="bit1" :disabled="!masterOn">禁止队友伤害 (位 1)</el-checkbox>
      <el-checkbox v-model="bit2" :disabled="!masterOn">禁止搜索到队友 (位 2)</el-checkbox>
      <el-checkbox v-model="bit8" :disabled="!masterOn">允许转移物品 (位 8)</el-checkbox>
    </div>

    <div class="hint" v-if="!masterOn">未开启时，所有玩家行为与散人一致。</div>
  </el-card>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useGameStateStore } from '@/stores/gameState'

const store = useGameStateStore()

const rulesConfig = computed(() => store.globalState?.rules_config ?? {})
const mode = computed(() => {
  const v = (rulesConfig.value as any).teammate_behavior
  return typeof v === 'number' ? v : 0
})

const lastNonZero = ref(1)
watch(mode, (v) => {
  if (v !== 0) lastNonZero.value = v
}, { immediate: true })

const masterOn = computed<boolean>({
  get: () => mode.value !== 0,
  set: (v) => store.setTeammateBehavior(v ? lastNonZero.value : 0),
})

const makeBit = (bit: number) =>
  computed<boolean>({
    get: () => (mode.value & bit) !== 0,
    set: (v) => store.setTeammateBehavior(v ? mode.value | bit : mode.value & ~bit),
  })

const bit1 = makeBit(1)
const bit2 = makeBit(2)
const bit8 = makeBit(8)
</script>

<style scoped>
.teammate-mode-card {
  margin-bottom: 16px;
}
.card-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
}
.card-header h4 {
  margin: 0;
}
.bits {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.bits.disabled {
  opacity: 0.5;
}
.hint {
  margin-top: 8px;
  color: #909399;
  font-size: 12px;
}
</style>
```

- [ ] **Step 3: Mount the card in InGameManagement**

In `frontend/src/views/director/management/InGameManagement.vue`, find the `<ShopManagement />` block (around line 102-105). Wrap it together with the new card in a fragment:

```vue
        <!-- 队友模式 + 商店管理 -->
        <div class="full-width-section">
          <TeammateModeCard />
        </div>

        <!-- 商店管理 -->
        <div class="full-width-section">
          <ShopManagement />
        </div>
```

In the `<script setup>` section, add to the imports (near the `ShopManagement` import):

```ts
import TeammateModeCard from '../components/TeammateModeCard.vue'
```

- [ ] **Step 4: Run frontend lint**

Run: `cd frontend && pnpm lint`
Expected: No new errors.

- [ ] **Step 5: Run frontend build**

Run: `cd frontend && pnpm build`
Expected: Build succeeds.

- [ ] **Step 6: Manual smoke test (frontend dev server)**

Run: `cd frontend && pnpm dev`

Open the director page for a game:
1. Verify "队友模式" card appears above "商店管理"
2. Toggle master switch ON → all 3 checkboxes enabled
3. Check bit 1 → log shows director broadcast
4. Toggle master OFF → mode returns to 0, checkboxes disabled
5. Toggle master ON again → restores last non-zero mode

- [ ] **Step 7: Commit**

```bash
cd frontend && git add src/stores/gameState.ts src/views/director/components/TeammateModeCard.vue src/views/director/management/InGameManagement.vue
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): add TeammateModeCard for director config"
```

---

## Task 9: Frontend — Actor Transfer Button + Dialog

**Files:**
- Modify: `frontend/src/views/actor/components/InventoryPanel.vue` (add button + dialog mount)
- Create: `frontend/src/views/actor/components/TransferTeammateDialog.vue`

**Interfaces:**
- Consumes: `store.globalState.rules_config.teammate_behavior`, `store.actorPlayer.team_id`, `store.actorPlayerList`
- Produces: Calls `gameStateStore.sendPlayerAction('transfer_item', { item_id, target_player_id })`.

- [ ] **Step 1: Create TransferTeammateDialog.vue**

Create `frontend/src/views/actor/components/TransferTeammateDialog.vue`:

```vue
<template>
  <el-dialog
    :model-value="modelValue"
    @update:model-value="$emit('update:modelValue', $event)"
    :title="`转移 ${itemName} 给队友`"
    width="420px"
  >
    <div v-if="teammates.length === 0" class="empty-state">
      暂无队友可转移
    </div>
    <el-radio-group v-else v-model="selected" class="radio-list">
      <el-radio v-for="t in teammates" :key="t.id" :value="t.id">
        {{ t.name }}
      </el-radio>
    </el-radio-group>
    <div v-if="teammates.length > 0" class="hint">对方将扣除 5 点体力</div>
    <template #footer>
      <el-button @click="$emit('update:modelValue', false)">取消</el-button>
      <el-button
        type="primary"
        :disabled="!selected || teammates.length === 0"
        @click="handleConfirm"
      >
        确认转移
      </el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useGameStateStore } from '@/stores/gameState'
import type { ActorPlayer } from '@/types/gameStateTypes'

const props = defineProps<{
  modelValue: boolean
  itemName: string
  itemId: string
}>()

const emit = defineEmits<{
  'update:modelValue': [boolean]
  transferred: []
}>()

const store = useGameStateStore()
const selected = ref<string>('')

const myTeamId = computed(() => store.actorPlayer?.team_id ?? 0)
const teammates = computed<ActorPlayer[]>(() =>
  store.actorPlayerList.filter(
    (p) => p.team_id && p.team_id > 0 && p.team_id === myTeamId.value
  )
)

watch(
  () => props.modelValue,
  (open) => {
    if (open) selected.value = ''
  }
)

const handleConfirm = () => {
  if (!selected.value) return
  store.sendPlayerAction('transfer_item', {
    item_id: props.itemId,
    target_player_id: selected.value,
  })
  emit('update:modelValue', false)
  emit('transferred')
}
</script>

<style scoped>
.radio-list {
  display: flex;
  flex-direction: column;
  gap: 8px;
}
.empty-state {
  color: #909399;
  text-align: center;
  padding: 16px;
}
.hint {
  margin-top: 8px;
  color: #e6a23c;
  font-size: 12px;
}
</style>
```

- [ ] **Step 2: Add transfer button to InventoryPanel.vue**

In `frontend/src/views/actor/components/InventoryPanel.vue`, find the `<template>` block where `<el-button>` items are rendered per inventory item (around line 150-160, the "丢弃" button). Insert a new button **before** the 丢弃 button:

```vue
          <!-- 转移按钮（队友模式位 8 开启时显示） -->
          <el-button
            v-if="transferAvailable"
            type="warning"
            size="small"
            @click="openTransferDialog(item)"
            :loading="loadingItems.includes(item.id)"
          >
            转移
          </el-button>

          <!-- 丢弃按钮 -->
          <el-button
            type="danger"
            size="small"
            @click="discardItem(item.id)"
            :loading="loadingItems.includes(item.id)"
          >
            丢弃
          </el-button>
```

In the `<script setup>` block, add:

```ts
import TransferTeammateDialog from './TransferTeammateDialog.vue'

const teammateMode = computed(() => {
  const v = (store.globalState as any)?.rules_config?.teammate_behavior
  return typeof v === 'number' ? v : 0
})
const transferAvailable = computed(() => {
  const masterOn = teammateMode.value !== 0
  const transferBitOn = (teammateMode.value & 8) !== 0
  return masterOn && transferBitOn
})

const transferDialogVisible = ref(false)
const transferTargetItem = ref<{ id: string; name: string } | null>(null)

const openTransferDialog = (item: { id: string; name: string }) => {
  transferTargetItem.value = { id: item.id, name: item.name }
  transferDialogVisible.value = true
}
```

Note: `store` must already be accessible — verify by searching for `useGameStateStore` in the file. If not present, import it.

At the bottom of the template (after the existing dialogs like `ItemSelectionDialog`), add:

```vue
    <TransferTeammateDialog
      v-if="transferTargetItem"
      v-model="transferDialogVisible"
      :item-name="transferTargetItem.name"
      :item-id="transferTargetItem.id"
    />
```

- [ ] **Step 3: Run frontend lint**

Run: `cd frontend && pnpm lint`
Expected: No new errors.

- [ ] **Step 4: Run frontend build**

Run: `cd frontend && pnpm build`
Expected: Build succeeds.

- [ ] **Step 5: Manual smoke test (full dev stack)**

Start both backend and frontend (`cargo run` + `pnpm dev`). Use two browser windows (one director, two actors in same team).

1. Director: open teammate mode + check bit 8 → save
2. Actor A (team=1) sees "转移" button on each inventory item
3. Actor A clicks "转移" → dialog opens, lists Actor B (also team=1)
4. Actor A selects B and confirms
5. Both A and B receive SystemNotice log entries
6. B's strength decreases by 5, item moves from A's inventory to B's
7. Director turns off teammate mode → button disappears

Edge cases to verify:
- Solo player (team_id=0) clicks "转移" → dialog opens with "暂无队友可转移"
- Director turns off just bit 8 (mode still > 0) → button disappears

- [ ] **Step 6: Commit**

```bash
cd frontend && git add src/views/actor/components/InventoryPanel.vue src/views/actor/components/TransferTeammateDialog.vue
git -c user.name="Wang Li" -c user.email="itx351@126.com" commit -m "feat(teammate): add transfer button + teammate picker dialog for actors"
```

---

## Self-Review

**Spec coverage:**
- §1 Config storage + helpers → Task 1, 2
- §2 (1) Attack main target immunity → Task 3
- §2 (2) AOE splash immunity → Task 4
- §2 (3) Remote mine immunity → Task 4
- §2 (4) Search filter → Task 5
- §3 Transfer action → Task 6
- §4.1 Expose team_id → Task 7
- §4.2 Director config card → Task 8
- §4.3 Actor transfer button → Task 9
- §4.4 Transfer dialog → Task 9

**Placeholder scan:** No TBD / "implement later" / vague references. All code blocks are concrete. Where Rust struct field names needed verification (e.g., `SearchResult`, `Item`, `UtilityProperties`), the plan instructs the implementer to grep first and adjust — this is a verification step, not a placeholder.

**Type consistency:**
- `TeammateBehavior::is_damage_immune` / `is_search_filtered` / `is_transfer_enabled` — defined in Task 1, used consistently in Tasks 3-6
- `GameState::are_teammates(&str, &str) -> bool` — defined in Task 1, used in Tasks 3-6
- `handle_transfer_item_action(sender, item_id, target)` — defined Task 6, dispatched in Task 6
- `handle_set_teammate_behavior(mode)` — defined Task 2, dispatched in Task 2
- Frontend `setTeammateBehavior(mode: number)` — defined Task 8, used Task 8
- Frontend `ActorPlayer.team_id?: number` — defined Task 7, used Task 9

**Scope check:** Single plan, 9 tasks, each independently testable. No sub-project decomposition needed.
