# 玩家上限显示与导演配置 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地规格 `docs/superpowers/specs/2026-08-27-player-caps-display-and-director-config-design.md` 的四项需求：导演端玩家上限配置 WS 动作三件套、演员端 当前/上限 显示、规则解析 caps 显示、导演端规则解析弹窗 bug 修复。

**Architecture:** 后端在现有导演动作调度链（`DirectorActionScheduler::dispatch` → `GameState::handle_*`）上新增 `max_life` / `max_strength` / `max_backpack` 三个动作，clamp 语义与永久增益道具（`handle_permanent_buff_use`）完全一致；前端导演表格沿用可编辑列（focus 锁序 + blur 提交）模式，演员端与规则解析为纯展示改动；bug 修复为删除一行导致组件卸载的 `emit('refresh')`。

**Tech Stack:** Rust (axum + serde) 后端，Vue 3 + TypeScript + Element Plus + Pinia 前端，pnpm 包管理。

## Global Constraints

- **提交身份（每次 commit 必须带）：** `git -c user.name="Huazero66" -c user.email="317786271+Huazero66@users.noreply.github.com" commit -m "..."`
- **clamp 语义（与永久增益道具完全一致，不得偏离）：** `base` = 规则基础值（`player_config.max_life` / `max_strength` / `max_backpack_items`）；`cap = max(cap配置, base)`（`max_life_cap` / `max_strength_cap` / `max_backpack_items_cap`，serde 缺省默认 300/300/12）；新上限 = `输入值.clamp(base, cap)`，越界自动收敛不报错；`max_life`/`max_strength` 降低时当前值 > 新上限则压到新上限；`max_backpack_items` 降低不丢弃已有物品（usize 经 i32 中转）
- **WS 动作名与参数字段名：** 动作 `"max_life"`/`"max_strength"`/`"max_backpack"`，参数 `player_id` + `max_life`/`max_strength`/`max_backpack_items`
- **前端 `Player` 类型（`frontend/src/types/gameStateTypes.ts`）不改** —— `max_life`/`max_strength`/`max_backpack_items` 字段已存在
- **前端验证命令：** `pnpm build`（= `vue-tsc -b && vite build`，在 `frontend/` 下执行）
- **后端测试命令：** `cargo test --test director_max_caps_integration`（纯 GameState 测试，无需数据库；在 `backend/` 下执行）
- **新 WS 动作纯增量**，不改动任何现有动作分支行为

---

### Task 1: 后端 WS 动作三件套与 clamp 处理器

**Files:**
- Modify: `backend/src/websocket/actions/director_action_scheduler.rs`（`DirectorActionParams` 结构体 ：11-59；`dispatch` match 的 `"coins"` 分支之后 ：168）
- Modify: `backend/src/websocket/actions/director_common_actions.rs`（`handle_set_player_coins` 函数结束后追加三个新 handler，该函数从 :412 开始）
- Create: `backend/tests/director_max_caps_integration.rs`

**Interfaces:**
- Consumes: `GameState.rule_engine.player_config` 的 `max_life: i32` / `max_life_cap: i32` / `max_strength: i32` / `max_strength_cap: i32` / `max_backpack_items: usize` / `max_backpack_items_cap: usize` 字段；`models::Player` 的 `max_life: i32` / `max_strength: i32` / `max_backpack_items: usize` 字段；`ActionResult::new_info_message` / `new_system_message` / `.as_results()`
- Produces: `GameState::handle_set_player_max_life(&mut self, player_id: &str, max_life: i32) -> Result<ActionResults, String>`、`handle_set_player_max_strength`（同签名）、`handle_set_player_max_backpack`（同签名，第二参数为 `i32`）；WS 动作 `"max_life"` / `"max_strength"` / `"max_backpack"`（Task 2 前端 store 依赖这三个动作名）

- [ ] **Step 1: 写失败测试（新文件 `backend/tests/director_max_caps_integration.rs`）**

```rust
//! 导演设置玩家上限（max_life / max_strength / max_backpack）集成测试
//! 覆盖 clamp 收敛、当前值压限、背包降容不丢物品、拾取拦截、缺参报错

use royale_arena_backend::websocket::actions::director_action_scheduler::{
    DirectorActionParams, DirectorActionScheduler,
};
use royale_arena_backend::websocket::models::{
    GameState, Place, Player, SearchResult, SearchResultType,
};
use serde_json::{Value, json};

/// 测试规则（基础：max_life 100 / max_strength 100 / max_backpack_items 6，cap 缺省 300/300/12）
fn get_test_rules() -> Value {
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
                    {"name": "[HP上限+20]养生丸", "properties": {"effect_type": "max_life", "effect_value": 20}}
                ]
            },
            "upgrade_recipes": {}
        }
    })
}

/// 覆盖三个 cap 字段的规则变体
fn rules_with_caps(max_life_cap: i32, max_strength_cap: i32, backpack_cap: usize) -> Value {
    let mut rules = get_test_rules();
    rules["player"]["max_life_cap"] = json!(max_life_cap);
    rules["player"]["max_strength_cap"] = json!(max_strength_cap);
    rules["player"]["max_backpack_items_cap"] = json!(backpack_cap);
    rules
}

fn add_test_player(
    game_state: &mut GameState,
    player_id: &str,
    player_name: &str,
    location: &str,
) {
    game_state
        .places
        .entry(location.to_string())
        .or_insert_with(|| Place::new(location.to_string()));

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

/// 测试：max_life 超过硬上限收敛到 cap，低于基础值收敛到 base
#[test]
fn test_max_life_clamps_between_base_and_cap() {
    let mut state = GameState::new("g1".to_string(), rules_with_caps(150, 300, 12));
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    state
        .handle_set_player_max_life("p1", 250)
        .expect("set max life");
    assert_eq!(state.players["p1"].max_life, 150, "超上限应收敛到 cap 150");

    state
        .handle_set_player_max_life("p1", 50)
        .expect("set max life");
    assert_eq!(state.players["p1"].max_life, 100, "低于基础值应收敛到 base 100");
}

/// 测试：cap 配置低于 base 时，生效上限为 max(cap, base)
#[test]
fn test_max_life_effective_cap_at_least_base() {
    let mut state = GameState::new("g1".to_string(), rules_with_caps(50, 300, 12));
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    state
        .handle_set_player_max_life("p1", 80)
        .expect("set max life");
    assert_eq!(
        state.players["p1"].max_life, 100,
        "生效上限应为 max(50, 100) = 100"
    );
}

/// 测试：降低 max_life 后当前生命压到新上限
#[test]
fn test_lower_max_life_presses_current_life() {
    let mut state = GameState::new("g1".to_string(), rules_with_caps(300, 300, 12));
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    state
        .handle_set_player_max_life("p1", 200)
        .expect("raise max life");
    state.players.get_mut("p1").unwrap().life = 180;

    state
        .handle_set_player_max_life("p1", 150)
        .expect("lower max life");
    assert_eq!(state.players["p1"].max_life, 150);
    assert_eq!(state.players["p1"].life, 150, "当前生命应压到新上限");
}

/// 测试：降低 max_strength 后当前体力压到新上限（与 max_life 同构）
#[test]
fn test_lower_max_strength_presses_current_strength() {
    let mut state = GameState::new("g1".to_string(), rules_with_caps(300, 200, 12));
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    state
        .handle_set_player_max_strength("p1", 180)
        .expect("raise max strength");
    state.players.get_mut("p1").unwrap().strength = 160;

    state
        .handle_set_player_max_strength("p1", 120)
        .expect("lower max strength");
    assert_eq!(state.players["p1"].max_strength, 120);
    assert_eq!(state.players["p1"].strength, 120, "当前体力应压到新上限");
}

/// 测试：降低 max_backpack_items 不丢物品，后续拾取按新上限拦截
#[test]
fn test_lower_max_backpack_keeps_items_and_blocks_pickup() {
    let mut state = GameState::new("g1".to_string(), get_test_rules());
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    // 升到 10，塞入 8 件物品
    state
        .handle_set_player_max_backpack("p1", 10)
        .expect("raise backpack cap");
    for _ in 0..8 {
        let item = state
            .rule_engine
            .create_item_from_name("[HP上限+20]养生丸")
            .unwrap();
        state.players.get_mut("p1").unwrap().inventory.push(item);
    }
    assert_eq!(state.players["p1"].get_total_item_count(), 8);

    // 降回基础值 6：物品不丢弃
    state
        .handle_set_player_max_backpack("p1", 6)
        .expect("lower backpack cap");
    assert_eq!(state.players["p1"].max_backpack_items, 6);
    assert_eq!(
        state.players["p1"].get_total_item_count(),
        8,
        "降低上限不应丢弃已有物品"
    );

    // 地点放物品，拾取应被拦截
    let item = state
        .rule_engine
        .create_item_from_name("[HP上限+20]养生丸")
        .unwrap();
    state.places.get_mut("位置1").unwrap().items.push(item);
    set_search_result_to_last_place_item(&mut state, "p1", "位置1");
    state.handle_pick_action("p1").expect("pick returns info result");
    assert_eq!(
        state.players["p1"].get_total_item_count(),
        8,
        "超上限拾取应被拦截"
    );
}

/// 测试：值未变化（含被 clamp 收敛回原值）返回 info 消息
#[test]
fn test_set_same_max_returns_info() {
    let mut state = GameState::new("g1".to_string(), get_test_rules());
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    let results = state
        .handle_set_player_max_life("p1", 100)
        .expect("info result");
    assert_eq!(results.results.len(), 1);
    assert_eq!(state.players["p1"].max_life, 100);
}

/// 测试：玩家不存在报错
#[test]
fn test_set_max_life_player_not_found() {
    let mut state = GameState::new("g1".to_string(), get_test_rules());
    let err = state
        .handle_set_player_max_life("nope", 150)
        .unwrap_err();
    assert_eq!(err, "Player not found");
}

/// 测试：调度器缺参报错（三个动作 + 缺 player_id）
#[test]
fn test_scheduler_missing_params() {
    let mut state = GameState::new("g1".to_string(), get_test_rules());
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    let params = DirectorActionParams::from_json(&json!({"player_id": "p1"})).unwrap();
    let err = DirectorActionScheduler::dispatch(&mut state, "max_life", params).unwrap_err();
    assert_eq!(err, "Missing max_life parameter");

    let params = DirectorActionParams::from_json(&json!({"player_id": "p1"})).unwrap();
    let err = DirectorActionScheduler::dispatch(&mut state, "max_strength", params).unwrap_err();
    assert_eq!(err, "Missing max_strength parameter");

    let params = DirectorActionParams::from_json(&json!({"player_id": "p1"})).unwrap();
    let err = DirectorActionScheduler::dispatch(&mut state, "max_backpack", params).unwrap_err();
    assert_eq!(err, "Missing max_backpack_items parameter");

    let params = DirectorActionParams::from_json(&json!({"max_life": 150})).unwrap();
    let err = DirectorActionScheduler::dispatch(&mut state, "max_life", params).unwrap_err();
    assert_eq!(err, "Missing player_id parameter");
}

/// 测试：调度器端到端 —— max_life 动作经 dispatch 生效并复用 clamp
#[test]
fn test_scheduler_max_life_applies() {
    let mut state = GameState::new("g1".to_string(), rules_with_caps(150, 300, 12));
    add_test_player(&mut state, "p1", "玩家1", "位置1");

    let params =
        DirectorActionParams::from_json(&json!({"player_id": "p1", "max_life": 250})).unwrap();
    DirectorActionScheduler::dispatch(&mut state, "max_life", params).expect("dispatch ok");
    assert_eq!(
        state.players["p1"].max_life, 150,
        "dispatch 应复用 clamp 语义"
    );
}
```

- [ ] **Step 2: 运行测试确认编译失败**

Run: `cd backend && cargo test --test director_max_caps_integration`
Expected: 编译错误，`handle_set_player_max_life` / `handle_set_player_max_strength` / `handle_set_player_max_backpack` 方法不存在（no method found）

- [ ] **Step 3: `DirectorActionParams` 加三个字段**

在 `backend/src/websocket/actions/director_action_scheduler.rs` 的 `pub coins: Option<f64>,    // 玩家货币`（:27）之后插入：

```rust
    pub max_life: Option<i32>,           // 玩家生命上限
    pub max_strength: Option<i32>,       // 玩家体力上限
    pub max_backpack_items: Option<i32>, // 玩家背包上限
```

- [ ] **Step 4: `dispatch` 加三个动作分支**

在同文件 `"coins"` 分支（:160-168）之后插入：

```rust
            "max_life" => {
                let player_id = action_params
                    .player_id
                    .ok_or_else(|| "Missing player_id parameter".to_string())?;
                let max_life = action_params
                    .max_life
                    .ok_or_else(|| "Missing max_life parameter".to_string())?;
                game_state.handle_set_player_max_life(&player_id, max_life)
            }

            "max_strength" => {
                let player_id = action_params
                    .player_id
                    .ok_or_else(|| "Missing player_id parameter".to_string())?;
                let max_strength = action_params
                    .max_strength
                    .ok_or_else(|| "Missing max_strength parameter".to_string())?;
                game_state.handle_set_player_max_strength(&player_id, max_strength)
            }

            "max_backpack" => {
                let player_id = action_params
                    .player_id
                    .ok_or_else(|| "Missing player_id parameter".to_string())?;
                let max_backpack_items = action_params
                    .max_backpack_items
                    .ok_or_else(|| "Missing max_backpack_items parameter".to_string())?;
                game_state.handle_set_player_max_backpack(&player_id, max_backpack_items)
            }
```

- [ ] **Step 5: 在 `director_common_actions.rs` 实现三个 handler**

在 `handle_set_player_coins` 函数结束的 `}` 之后追加（clamp 语义照抄 `player_use_action.rs` 的 `handle_permanent_buff_use`）：

```rust
    /// 设置玩家生命上限（clamp 到 [base, cap]，当前生命高于新上限时压到新上限）
    pub fn handle_set_player_max_life(
        &mut self,
        player_id: &str,
        max_life: i32,
    ) -> Result<ActionResults, String> {
        let (base_max_life, max_life_cap) = {
            let pc = &self.rule_engine.player_config;
            (pc.max_life, pc.max_life_cap)
        };
        let cap = max_life_cap.max(base_max_life);
        let new_max_life = max_life.clamp(base_max_life, cap);

        let (player_name, final_max_life, final_life) = {
            let player = self.players.get_mut(player_id).ok_or("Player not found")?;

            if player.max_life == new_max_life {
                let data = serde_json::json!({
                    "player_id": player_id,
                    "max_life": player.max_life,
                    "message": "生命上限未发生变化"
                });
                let log_message = format!(
                    "导演尝试设置 {} 生命上限为 {}，但未发生变化（或被上限规则收敛回原值）",
                    player.name, max_life
                );
                return Ok(
                    ActionResult::new_info_message(data, vec![], log_message, true).as_results(),
                );
            }

            let player_name = player.name.clone();
            player.max_life = new_max_life;
            if player.life > player.max_life {
                player.life = player.max_life;
            }
            (player_name, player.max_life, player.life)
        };

        let data = serde_json::json!({
            "player_id": player_id,
            "max_life": final_max_life,
            "life": final_life
        });

        let action_result = ActionResult::new_system_message(
            data,
            vec![player_id.to_string()],
            format!(
                "导演设置 {} 生命上限为 {}（当前生命 {}）",
                player_name, final_max_life, final_life
            ),
            true,
        );

        Ok(action_result.as_results())
    }

    /// 设置玩家体力上限（clamp 到 [base, cap]，当前体力高于新上限时压到新上限）
    pub fn handle_set_player_max_strength(
        &mut self,
        player_id: &str,
        max_strength: i32,
    ) -> Result<ActionResults, String> {
        let (base_max_strength, max_strength_cap) = {
            let pc = &self.rule_engine.player_config;
            (pc.max_strength, pc.max_strength_cap)
        };
        let cap = max_strength_cap.max(base_max_strength);
        let new_max_strength = max_strength.clamp(base_max_strength, cap);

        let (player_name, final_max_strength, final_strength) = {
            let player = self.players.get_mut(player_id).ok_or("Player not found")?;

            if player.max_strength == new_max_strength {
                let data = serde_json::json!({
                    "player_id": player_id,
                    "max_strength": player.max_strength,
                    "message": "体力上限未发生变化"
                });
                let log_message = format!(
                    "导演尝试设置 {} 体力上限为 {}，但未发生变化（或被上限规则收敛回原值）",
                    player.name, max_strength
                );
                return Ok(
                    ActionResult::new_info_message(data, vec![], log_message, true).as_results(),
                );
            }

            let player_name = player.name.clone();
            player.max_strength = new_max_strength;
            if player.strength > player.max_strength {
                player.strength = player.max_strength;
            }
            (player_name, player.max_strength, player.strength)
        };

        let data = serde_json::json!({
            "player_id": player_id,
            "max_strength": final_max_strength,
            "strength": final_strength
        });

        let action_result = ActionResult::new_system_message(
            data,
            vec![player_id.to_string()],
            format!(
                "导演设置 {} 体力上限为 {}（当前体力 {}）",
                player_name, final_max_strength, final_strength
            ),
            true,
        );

        Ok(action_result.as_results())
    }

    /// 设置玩家背包上限（clamp 到 [base, cap]，降低上限不丢弃已有物品）
    pub fn handle_set_player_max_backpack(
        &mut self,
        player_id: &str,
        max_backpack_items: i32,
    ) -> Result<ActionResults, String> {
        let (base_backpack, backpack_cap) = {
            let pc = &self.rule_engine.player_config;
            (pc.max_backpack_items, pc.max_backpack_items_cap)
        };
        let base = base_backpack as i32;
        let cap = (backpack_cap as i32).max(base);
        let new_max = max_backpack_items.clamp(base, cap) as usize;

        let (player_name, final_max) = {
            let player = self.players.get_mut(player_id).ok_or("Player not found")?;

            if player.max_backpack_items == new_max {
                let data = serde_json::json!({
                    "player_id": player_id,
                    "max_backpack_items": player.max_backpack_items,
                    "message": "背包上限未发生变化"
                });
                let log_message = format!(
                    "导演尝试设置 {} 背包上限为 {}，但未发生变化（或被上限规则收敛回原值）",
                    player.name, max_backpack_items
                );
                return Ok(
                    ActionResult::new_info_message(data, vec![], log_message, true).as_results(),
                );
            }

            let player_name = player.name.clone();
            player.max_backpack_items = new_max;
            (player_name, new_max)
        };

        // 降低上限不丢弃已有物品：inventory 不裁剪，仅拦截后续拾取

        let data = serde_json::json!({
            "player_id": player_id,
            "max_backpack_items": final_max
        });

        let action_result = ActionResult::new_system_message(
            data,
            vec![player_id.to_string()],
            format!("导演设置 {} 背包上限为 {}", player_name, final_max),
            true,
        );

        Ok(action_result.as_results())
    }
```

- [ ] **Step 6: 运行测试确认全部通过**

Run: `cd backend && cargo test --test director_max_caps_integration`
Expected: 9 个测试全部 PASS

- [ ] **Step 7: 跑相邻回归测试**

Run: `cd backend && cargo test --test permanent_buff_integration`
Expected: 全部 PASS（未改动道具 clamp 路径，确认无回归）

- [ ] **Step 8: 提交**

```bash
git add backend/src/websocket/actions/director_action_scheduler.rs backend/src/websocket/actions/director_common_actions.rs backend/tests/director_max_caps_integration.rs
git -c user.name="Huazero66" -c user.email="317786271+Huazero66@users.noreply.github.com" commit -m "feat: 导演端玩家上限配置 WS 动作三件套（clamp 语义与永久增益道具一致）"
```

---

### Task 2: 前端 store 方法与导演表格三个可编辑上限列

**Files:**
- Modify: `frontend/src/stores/gameState.ts`（`setPlayerCoins` 之后 :258 加三个方法；store 的 return 导出列表中 `setPlayerLife,` 附近加三个导出）
- Modify: `frontend/src/views/director/components/PlayerStatusCard.vue`

**Interfaces:**
- Consumes: Task 1 的 WS 动作 `"max_life"` / `"max_strength"` / `"max_backpack"`（参数 `player_id` + `max_life` / `max_strength` / `max_backpack_items`）；现有 `sendDirectorAction(action, params)` helper
- Produces: store 方法 `setPlayerMaxLife(playerId: string, maxLife: number)` / `setPlayerMaxStrength(playerId: string, maxStrength: number)` / `setPlayerMaxBackpack(playerId: string, maxBackpackItems: number)`（本任务组件内使用；未来任何导演组件可用）

- [ ] **Step 1: store 加三个方法**

在 `frontend/src/stores/gameState.ts` 的 `setPlayerCoins`（:256-258）之后插入：

```ts
  // 设置玩家生命上限（绝对值）
  const setPlayerMaxLife = (playerId: string, maxLife: number) => {
    sendDirectorAction('max_life', { player_id: playerId, max_life: maxLife })
  }

  // 设置玩家体力上限（绝对值）
  const setPlayerMaxStrength = (playerId: string, maxStrength: number) => {
    sendDirectorAction('max_strength', { player_id: playerId, max_strength: maxStrength })
  }

  // 设置玩家背包上限（绝对值）
  const setPlayerMaxBackpack = (playerId: string, maxBackpackItems: number) => {
    sendDirectorAction('max_backpack', { player_id: playerId, max_backpack_items: maxBackpackItems })
  }
```

再在 store 的 return 对象里找到 `setPlayerLife,` 一行，紧随其后（与 `setPlayerStrength,` `setPlayerCoins,` 相邻处）加入：

```ts
    setPlayerMaxLife,
    setPlayerMaxStrength,
    setPlayerMaxBackpack,
```

- [ ] **Step 2: `PlayerStatusCard.vue` 扩展 SortKey 类型**

:358 现为：

```ts
type SortKey = 'name' | 'votes' | 'location' | 'life' | 'strength' | 'coins'
```

改为：

```ts
type SortKey =
  | 'name'
  | 'votes'
  | 'location'
  | 'life'
  | 'strength'
  | 'max_life'
  | 'max_strength'
  | 'max_backpack_items'
  | 'coins'
```

- [ ] **Step 3: 模板在"体力"列后插入三个可编辑列**

`PlayerStatusCard.vue` 的体力列（`label="体力"`，:156-181）结束标签 `</el-table-column>` 之后、货币列（`label="货币"`）之前插入：

```vue
          <el-table-column label="生命上限" min-width="70">
            <template #header>
              <div
                class="sortable-header"
                role="button"
                tabindex="0"
                @click="toggleSort('max_life')"
                @keydown.enter.prevent="toggleSort('max_life')"
                @keydown.space.prevent="toggleSort('max_life')"
              >
                生命上限
                <ArrowUp v-if="sortKey === 'max_life' && sortOrder === 'asc'" class="sort-icon" />
                <ArrowDown v-else-if="sortKey === 'max_life' && sortOrder === 'desc'" class="sort-icon" />
              </div>
            </template>
            <template #default="scope">
              <div class="status-value">
                <el-input 
                  v-model="scope.row.max_life"
                  @focus="() => handleEditableFieldFocus(scope.row, 'max_life')"
                  @blur="(event: FocusEvent) => handleMaxLifeBlur(scope.row, event)"
                  size="small"
                />
              </div>
            </template>
          </el-table-column>
          <el-table-column label="体力上限" min-width="70">
            <template #header>
              <div
                class="sortable-header"
                role="button"
                tabindex="0"
                @click="toggleSort('max_strength')"
                @keydown.enter.prevent="toggleSort('max_strength')"
                @keydown.space.prevent="toggleSort('max_strength')"
              >
                体力上限
                <ArrowUp v-if="sortKey === 'max_strength' && sortOrder === 'asc'" class="sort-icon" />
                <ArrowDown v-else-if="sortKey === 'max_strength' && sortOrder === 'desc'" class="sort-icon" />
              </div>
            </template>
            <template #default="scope">
              <div class="status-value">
                <el-input 
                  v-model="scope.row.max_strength"
                  @focus="() => handleEditableFieldFocus(scope.row, 'max_strength')"
                  @blur="(event: FocusEvent) => handleMaxStrengthBlur(scope.row, event)"
                  size="small"
                />
              </div>
            </template>
          </el-table-column>
          <el-table-column label="背包上限" min-width="70">
            <template #header>
              <div
                class="sortable-header"
                role="button"
                tabindex="0"
                @click="toggleSort('max_backpack_items')"
                @keydown.enter.prevent="toggleSort('max_backpack_items')"
                @keydown.space.prevent="toggleSort('max_backpack_items')"
              >
                背包上限
                <ArrowUp v-if="sortKey === 'max_backpack_items' && sortOrder === 'asc'" class="sort-icon" />
                <ArrowDown v-else-if="sortKey === 'max_backpack_items' && sortOrder === 'desc'" class="sort-icon" />
              </div>
            </template>
            <template #default="scope">
              <div class="status-value">
                <el-input 
                  v-model="scope.row.max_backpack_items"
                  @focus="() => handleEditableFieldFocus(scope.row, 'max_backpack_items')"
                  @blur="(event: FocusEvent) => handleMaxBackpackBlur(scope.row, event)"
                  size="small"
                />
              </div>
            </template>
          </el-table-column>
```

- [ ] **Step 4: `getSortValue` 加三个 case**

在 `getSortValue`（:400-416）的 `case 'strength': return player.strength` 之后插入：

```ts
    case 'max_life':
      return player.max_life
    case 'max_strength':
      return player.max_strength
    case 'max_backpack_items':
      return player.max_backpack_items
```

- [ ] **Step 5: `handleEditableFieldFocus` 加三个分支**

:508-526 现为 `if (key === 'life') ... else if (key === 'strength') ... else if (key === 'coins')`。在 `else if (key === 'strength')` 分支之后插入：

```ts
  } else if (key === 'max_life') {
    originalValue = player.max_life
  } else if (key === 'max_strength') {
    originalValue = player.max_strength
  } else if (key === 'max_backpack_items') {
    originalValue = player.max_backpack_items
```

- [ ] **Step 6: 加三个 blur handler 与 update 函数**

在 `handleStrengthBlur`（:543-551）之后插入：

```ts
const handleMaxLifeBlur = (player: Player, event: FocusEvent) => {
  const newValueStr = (event.target as HTMLInputElement).value
  const currentValue = editingState.value && editingState.value.playerId === player.id && editingState.value.key === 'max_life'
    ? editingState.value.originalValue
    : player.max_life

  updatePlayerMaxLife(player.id, currentValue, newValueStr)
  finishEditing()
}

const handleMaxStrengthBlur = (player: Player, event: FocusEvent) => {
  const newValueStr = (event.target as HTMLInputElement).value
  const currentValue = editingState.value && editingState.value.playerId === player.id && editingState.value.key === 'max_strength'
    ? editingState.value.originalValue
    : player.max_strength

  updatePlayerMaxStrength(player.id, currentValue, newValueStr)
  finishEditing()
}

const handleMaxBackpackBlur = (player: Player, event: FocusEvent) => {
  const newValueStr = (event.target as HTMLInputElement).value
  const currentValue = editingState.value && editingState.value.playerId === player.id && editingState.value.key === 'max_backpack_items'
    ? editingState.value.originalValue
    : player.max_backpack_items

  updatePlayerMaxBackpack(player.id, currentValue, newValueStr)
  finishEditing()
}
```

在 `updatePlayerStrength`（:586-592）之后插入：

```ts
// 更新玩家生命上限
const updatePlayerMaxLife = (playerId: string, currentValue: number, newValueStr: string) => {
  const newValue = parseInt(newValueStr, 10)
  // 只有当值发生变化时才提交修改
  if (!isNaN(newValue) && newValue !== currentValue) {
    store.setPlayerMaxLife(playerId, newValue)
  }
}

// 更新玩家体力上限
const updatePlayerMaxStrength = (playerId: string, currentValue: number, newValueStr: string) => {
  const newValue = parseInt(newValueStr, 10)
  // 只有当值发生变化时才提交修改
  if (!isNaN(newValue) && newValue !== currentValue) {
    store.setPlayerMaxStrength(playerId, newValue)
  }
}

// 更新玩家背包上限
const updatePlayerMaxBackpack = (playerId: string, currentValue: number, newValueStr: string) => {
  const newValue = parseInt(newValueStr, 10)
  // 只有当值发生变化时才提交修改
  if (!isNaN(newValue) && newValue !== currentValue) {
    store.setPlayerMaxBackpack(playerId, newValue)
  }
}
```

- [ ] **Step 7: 类型检查与构建**

Run: `cd frontend && pnpm build`
Expected: `vue-tsc -b` 无错误，`vite build` 成功

- [ ] **Step 8: 提交**

```bash
git add frontend/src/stores/gameState.ts frontend/src/views/director/components/PlayerStatusCard.vue
git -c user.name="Huazero66" -c user.email="317786271+Huazero66@users.noreply.github.com" commit -m "feat: 导演端玩家状态管理新增生命/体力/背包上限三个可编辑列"
```

---

### Task 3: 演员端 当前/上限 显示

**Files:**
- Modify: `frontend/src/views/actor/components/CompactActionPanel.vue`（状态栏 :8-14）
- Modify: `frontend/src/views/actor/states/InGameState.vue`（:43 总物品数标签）

**Interfaces:**
- Consumes: `Player` 类型的 `max_life` / `max_strength` / `max_backpack_items` 字段（已存在，无类型改动）；`InGameState` 现有 `totalItemCount` computed（计数口径不变）
- Produces: 无（纯展示）

- [ ] **Step 1: 状态栏改为 当前/上限**

`CompactActionPanel.vue` :9 与 :13 现为：

```vue
      <span :class="['status-value', 'life', lifeAnimationClass]">{{ player.life }}</span>
```

```vue
      <span class="status-value strength">{{ player.strength }}</span>
```

分别改为：

```vue
      <span :class="['status-value', 'life', lifeAnimationClass]">{{ player.life }}/{{ player.max_life }}</span>
```

```vue
      <span class="status-value strength">{{ player.strength }}/{{ player.max_strength }}</span>
```

- [ ] **Step 2: 背包总物品数改为 当前/上限**

`InGameState.vue` :43 现为：

```vue
            <el-tag v-if="player" type="info">总物品数: {{ totalItemCount }}</el-tag>
```

改为：

```vue
            <el-tag v-if="player" type="info">总物品数: {{ totalItemCount }}/{{ player.max_backpack_items }}</el-tag>
```

- [ ] **Step 3: 类型检查与构建**

Run: `cd frontend && pnpm build`
Expected: 构建成功

- [ ] **Step 4: 提交**

```bash
git add frontend/src/views/actor/components/CompactActionPanel.vue frontend/src/views/actor/states/InGameState.vue
git -c user.name="Huazero66" -c user.email="317786271+Huazero66@users.noreply.github.com" commit -m "feat: 演员端生命/体力/背包显示当前值与上限"
```

---

### Task 4: 规则解析显示初始上限与可累加最大上限

**Files:**
- Modify: `frontend/src/utils/gameRuleParser.ts`（`ParsedGameRules.player` 接口 :17-25；`parse()` 返回对象 :112-120）
- Modify: `frontend/src/components/GameRulesPreview.vue`（玩家配置区 :108-114）

**Interfaces:**
- Consumes: 规则 JSON 的 `player.max_life_cap` / `max_strength_cap` / `max_backpack_items_cap`（可选字段，缺省 300/300/12，与后端 serde 默认一致）；`parse()` 内 `config` 为 `any` 类型，直接取字段无需改原始类型
- Produces: `ParsedGameRules.player` 新增 `maxLifeCap: number` / `maxStrengthCap: number` / `maxBackpackItemsCap: number`（`GameRulesPreview.vue` 与任何后续消费者使用）

- [ ] **Step 1: `ParsedGameRules.player` 加三个字段**

`gameRuleParser.ts` :17-25 现为：

```ts
	player: {
		maxLife: number
		maxStrength: number
		dailyLifeRecovery: number
		dailyStrengthRecovery: number
		searchCooldown: number
		maxBackpackItems: number
		unarmedDamage: number
	}
```

改为（注意该文件用 Tab 缩进）：

```ts
	player: {
		maxLife: number
		maxLifeCap: number
		maxStrength: number
		maxStrengthCap: number
		dailyLifeRecovery: number
		dailyStrengthRecovery: number
		searchCooldown: number
		maxBackpackItems: number
		maxBackpackItemsCap: number
		unarmedDamage: number
	}
```

- [ ] **Step 2: `parse()` 填充三个字段**

:112-120 的 `player: { ... }` 返回块改为：

```ts
			player: {
				maxLife: config.player.max_life,
				maxLifeCap: config.player.max_life_cap ?? 300,
				maxStrength: config.player.max_strength,
				maxStrengthCap: config.player.max_strength_cap ?? 300,
				dailyLifeRecovery: config.player.daily_life_recovery,
				dailyStrengthRecovery: config.player.daily_strength_recovery,
				searchCooldown: config.player.search_cooldown,
				maxBackpackItems: config.player.max_backpack_items,
				maxBackpackItemsCap: config.player.max_backpack_items_cap ?? 12,
				unarmedDamage: config.player.unarmed_damage
			},
```

- [ ] **Step 3: `GameRulesPreview.vue` 更新三行文案**

:108-114 现为：

```vue
                <p><strong>最大生命值：</strong>{{ parsedRules.player.maxLife }}</p>
                <p><strong>最大体力值：</strong>{{ parsedRules.player.maxStrength }}</p>
                <p><strong>每日生命恢复：</strong>{{ parsedRules.player.dailyLifeRecovery }}</p>
                <p><strong>每日体力恢复：</strong>{{ parsedRules.player.dailyStrengthRecovery }}</p>
                <p><strong>搜索冷却时间：</strong>{{ parsedRules.player.searchCooldown }}秒</p>
                <p><strong>背包最大物品数：</strong>{{ parsedRules.player.maxBackpackItems }}</p>
                <p><strong>挥拳伤害：</strong>{{ parsedRules.player.unarmedDamage }}</p>
```

改为（中间三行不动）：

```vue
                <p><strong>生命上限（初始）：</strong>{{ parsedRules.player.maxLife }}（可累加至 {{ parsedRules.player.maxLifeCap }}）</p>
                <p><strong>体力上限（初始）：</strong>{{ parsedRules.player.maxStrength }}（可累加至 {{ parsedRules.player.maxStrengthCap }}）</p>
                <p><strong>每日生命恢复：</strong>{{ parsedRules.player.dailyLifeRecovery }}</p>
                <p><strong>每日体力恢复：</strong>{{ parsedRules.player.dailyStrengthRecovery }}</p>
                <p><strong>搜索冷却时间：</strong>{{ parsedRules.player.searchCooldown }}秒</p>
                <p><strong>背包上限（初始）：</strong>{{ parsedRules.player.maxBackpackItems }}（可累加至 {{ parsedRules.player.maxBackpackItemsCap }}）</p>
                <p><strong>挥拳伤害：</strong>{{ parsedRules.player.unarmedDamage }}</p>
```

导演端与演员端预览共用此组件，同时生效。

- [ ] **Step 4: 类型检查与构建**

Run: `cd frontend && pnpm build`
Expected: 构建成功（若 `ParsedGameRules` 存在其他构造点遗漏新字段，`vue-tsc` 会报错——按报错位置补齐同名字段）

- [ ] **Step 5: 提交**

```bash
git add frontend/src/utils/gameRuleParser.ts frontend/src/components/GameRulesPreview.vue
git -c user.name="Huazero66" -c user.email="317786271+Huazero66@users.noreply.github.com" commit -m "feat: 规则解析显示初始上限与可累加最大上限"
```

---

### Task 5: 修复导演端游戏开始后规则解析打不开

**Files:**
- Modify: `frontend/src/views/director/components/DirectorHeader.vue`（:178-186）

**Interfaces:**
- Consumes: 无
- Produces: 无（行为修复）

**根因**（已代码级确认）：`openRulesPreview` 先 `emit('refresh')` 再置 `showRulesPreview = true`；`DirectorMain.fetchGameDetail` 第一行 `loading.value = true`，模板 `v-if="loading"` 将整个内容区（含 DirectorHeader 及其弹窗）卸载重挂，弹窗的局部 ref 随组件销毁，重挂后回到 `false`，弹窗从未显示。演员页 `ActorHeader.vue:41` 直接 `showRulesPreview.value = true`，故能打开。

- [ ] **Step 1: 删除触发组件卸载的 `emit('refresh')`**

`DirectorHeader.vue` :183-186 现为：

```ts
const openRulesPreview = () => {
  emit('refresh')
  showRulesPreview.value = true
}
```

改为：

```ts
const openRulesPreview = () => {
  showRulesPreview.value = true
}
```

同时清理现已无用的 emit 声明——`:178-181` 现为：

```ts
const emit = defineEmits<{
  (e: 'status-updated'): void
  (e: 'refresh'): void
}>()
```

改为（`:184` 是 `'refresh'` 的唯一 emit 调用点；`'status-updated'` 在 :255/:329 仍使用，保留）：

```ts
const emit = defineEmits<{
  (e: 'status-updated'): void
}>()
```

- [ ] **Step 2: 类型检查与构建**

Run: `cd frontend && pnpm build`
Expected: 构建成功

- [ ] **Step 3: 提交**

```bash
git add frontend/src/views/director/components/DirectorHeader.vue
git -c user.name="Huazero66" -c user.email="317786271+Huazero66@users.noreply.github.com" commit -m "fix: 导演端游戏开始后规则解析弹窗无法打开（emit refresh 导致组件卸载）"
```

---

## 手动验证清单（全部任务完成后，与用户一起在本地部署环境实测）

1. 导演表格三列（生命上限/体力上限/背包上限）可编辑、提交后生效、表头可排序
2. 越界输入被 clamp：低于规则基础值收敛到基础值、超过 cap 收敛到 cap
3. 演员端状态栏显示 `80/120` 格式；背包管理头部显示 `总物品数: 1/8`
4. 规则解析（导演端与演员端）显示 `生命上限（初始）：100（可累加至 300）` 三行新文案；旧规则配置（无 cap 字段）显示默认 300/300/12
5. 游戏开始后导演端折叠区"浏览当前规则解析"可正常打开弹窗
