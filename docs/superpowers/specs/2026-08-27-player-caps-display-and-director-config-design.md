# 玩家上限显示与导演配置设计

> 日期：2026-08-27 ｜ 状态：已确认
> 背景：永久增益道具（2026-08-25）落地后，上限成为运行时动态值，但各端 UI 仍只显示规则初始值或孤立数值，导演也无法直接调整玩家上限。

## 目标

1. 导演端"玩家状态管理"显示并允许配置每位玩家的生命/体力/背包上限
2. 演员端生命、体力显示为 `当前/上限`（如 `80/120`），背包管理显示 `总物品数: 1/8`
3. 规则解析显示初始上限与可累加最大上限（硬上限）
4. 修复导演端游戏开始后"浏览当前规则解析"打不开的缺陷

## 一、导演端玩家状态管理：上限显示与配置

### UI（`frontend/src/views/director/components/PlayerStatusCard.vue`）

表格在现有"生命""体力"列后新增三列：`生命上限`、`体力上限`、`背包上限`，完全沿用现有可编辑列模式：

- 单元格为 `el-input`，显示 `player.max_life` / `player.max_strength` / `player.max_backpack_items`
- 失焦提交（`handleMaxLifeBlur` 等），`parseInt` 且值变化才提交，与 `handleLifeBlur` 同构
- 表头排序与现有列一致（`sortKey` 支持 `max_life` / `max_strength` / `max_backpack_items`）

### 前端 store（`frontend/src/stores/gameState.ts`）

新增三个方法，对齐现有 `setPlayerLife` 模式：

```ts
const setPlayerMaxLife = (playerId: string, maxLife: number) =>
  sendDirectorAction('max_life', { player_id: playerId, max_life: maxLife })
const setPlayerMaxStrength = (playerId: string, maxStrength: number) =>
  sendDirectorAction('max_strength', { player_id: playerId, max_strength: maxStrength })
const setPlayerMaxBackpack = (playerId: string, maxBackpackItems: number) =>
  sendDirectorAction('max_backpack', { player_id: playerId, max_backpack_items: maxBackpackItems })
```

### 后端（`backend/src/websocket/actions/director_action_scheduler.rs` + `models.rs`）

- `DirectorActionParams` 新增可选字段：`max_life: Option<i32>`、`max_strength: Option<i32>`、`max_backpack_items: Option<i32>`
- 调度器新增三个动作分支：`"max_life"`、`"max_strength"`、`"max_backpack"`，缺参报错模式与 `"life"` 分支一致
- 转发到 `game_state.handle_set_player_max_life / handle_set_player_max_strength / handle_set_player_max_backpack`（与 `handle_set_player_life` 同在 `backend/src/websocket/actions/director_common_actions.rs`）

### 生效语义（与永久增益道具完全一致，用户已确认）

记 `base` 为规则基础值（`player_config.max_life` / `max_strength` / `max_backpack_items`），`cap = max(cap配置, base)`（`max_life_cap` / `max_strength_cap` / `max_backpack_items_cap`，缺省 300/300/12）：

- 新上限 = `输入值.clamp(base, cap)`，越界自动收敛，不报错
- `max_life` / `max_strength`：若当前值 > 新上限，当前值压到新上限
- `max_backpack_items`：降低不丢弃已有物品（usize 经 i32 中转，同道具实现）
- 动作通过现有广播机制同步给导演与演员客户端

## 二、演员端 当前/上限 显示（纯前端）

- `frontend/src/views/actor/components/CompactActionPanel.vue` 状态栏：
  - `生命: {{ player.life }}/{{ player.max_life }}`
  - `体力: {{ player.strength }}/{{ player.max_strength }}`
- `frontend/src/views/actor/states/InGameState.vue` 背包管理头部标签：
  - `总物品数: {{ totalItemCount }}/{{ player.max_backpack_items }}`
  - 计数口径不变（背包物品 + 已装备武器/防具，与后端 `get_total_item_count` 一致）
- 前端 `Player` 类型已含三个字段（`gameStateTypes.ts:45`），后端 WS 状态已序列化，无类型改动

## 三、规则解析显示初始上限与可累加最大上限

### 解析器（`frontend/src/utils/gameRuleParser.ts`）

`ParsedRules.player` 新增：`maxLifeCap`、`maxStrengthCap`、`maxBackpackItemsCap`，取自 `config.player.max_life_cap` / `max_strength_cap` / `max_backpack_items_cap`，**缺省默认 300 / 300 / 12**（与后端 serde 默认一致，旧配置兼容）。

### 展示（`frontend/src/components/GameRulesPreview.vue` 玩家配置区）

| 现显示 | 改为 |
|---|---|
| 最大生命值：100 | 生命上限（初始）：100（可累加至 300） |
| 最大体力值：100 | 体力上限（初始）：100（可累加至 300） |
| 背包最大物品数：6 | 背包上限（初始）：6（可累加至 12） |

导演端与演员端预览共用此组件，同时生效。

## 四、修复导演端规则解析打不开

**根因**（代码级确认）：`DirectorHeader.vue` 的 `openRulesPreview` 先 `emit('refresh')` 再置 `showRulesPreview = true`；`DirectorMain.fetchGameDetail` 第一行 `loading.value = true`，模板 `v-if="loading"` 将整个内容区（含 DirectorHeader 及其弹窗）卸载重挂，弹窗的局部 ref 随组件销毁，重挂后回到 `false`，弹窗从未显示。

**修复**：`openRulesPreview` 改为与演员页（`ActorHeader.vue:41`）一致——直接 `showRulesPreview.value = true`，去掉 `emit('refresh')`。位置、样式不变。

## 测试策略

- **后端集成测试**（并入现有 `backend/tests/director_integration.rs`，clamp 测试可参照 `permanent_buff_integration.rs` 的 helpers）：
  - `max_life` 超过硬上限 → 收敛到 cap；低于基础值 → 收敛到 base
  - 降低 `max_life` 后当前生命压到新上限；`max_strength` 同构
  - 降低 `max_backpack_items` 不丢物品，后续拾取按新上限拦截
  - 缺参报错
- **前端**：`vue-tsc` 类型检查 + `vite build`；实测清单：导演表格三列编辑生效、演员 `80/120` 与 `1/8` 显示、规则解析新文案、开局后导演端规则解析可打开

## 兼容性

- 新 WS 动作纯增量，旧客户端不受影响
- `max_*_cap` 缺省默认与后端一致，旧规则配置无需迁移
- 前端 `Player` 类型无改动
