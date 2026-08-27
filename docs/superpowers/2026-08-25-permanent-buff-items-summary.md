# 永久增益道具（第 7 类道具）修改总结

> 工作日期：2026-08-25 ｜ 分支：`worktree-permanent-buff-items`（已合并 main，快进至 `2d2e459`）
> 规格：`docs/superpowers/specs/2026-08-25-permanent-buff-items-design.md` ｜ 计划：`docs/superpowers/plans/2026-08-25-permanent-buff-items.md`

## 一、功能语义

新增第 7 类道具 `permanent_buff`（永久增益），使用后永久改变一项上限，道具销毁，默认不扣体力（`action_costs.use` 可配置）。

- **效果类型**：`max_life`（生命上限）/ `max_strength`（体力上限）/ `max_backpack`（背包容量）
- **正数提升**：封顶 `max(硬上限, 规则基础值)`；溢出浪费（到顶后道具照常消耗、数值无效）
  - 硬上限字段：`max_life_cap` / `max_strength_cap` / `max_backpack_items_cap`（默认 300/300/12，serde 默认值，旧配置兼容）
- **负数削弱**：降低上限，**下限为规则基础值**（最多降回开局状态，不会更低）
  - 降低生命/体力上限时，当前值若超过新上限则压到新上限
  - 背包容量降低不丢已有物品，只影响后续拾取/购买/接收
- **背包容量改为玩家字段** `Player.max_backpack_items`（原为全局规则读取，共 5 处引用点切换：拾取、商店购买、队友转移、行动调度、击杀拾取战利品）；旧存档反序列化自动回填规则值
- 商店自动放行新类别（现有逻辑仅拒绝武器/防具）
- 不设稀有度（与消耗品一致，`rarity` 保留为可选字段）

## 二、后端改动（Rust）

| 文件 | 内容 |
|---|---|
| `backend/src/game/game_rule_engine.rs` | `PermanentBuffProperties` / `PermanentBuffConfig` 类型；`ItemType::PermanentBuff` 变体；`ItemsByCategory.permanent_buffs` 配置数组（`#[serde(default)]`）；`PlayerConfig` 三个 cap 字段（带 serde 默认） |
| `backend/src/websocket/actions/player_use_action.rs` | `handle_permanent_buff_use`：三效果分支、正负双向 clamp（`clamp(基础值, 生效上限)`）、当前值压缩、未知 effect_type 报错并回插道具 |
| `backend/src/websocket/models.rs` | `Player.max_backpack_items` 字段（`#[serde(default)]`）；GameState 自定义反序列化中的旧存档回填钩子 |
| `backend/src/websocket/actions/player_common_actions.rs`、`player_action_scheduler.rs`、`game_state_common.rs` | 背包容量 5 处引用从 `rule_engine.player_config.max_backpack_items` 切换到 `player.max_backpack_items` |
| `backend/tests/permanent_buff_integration.rs`（新建） | 13 个集成测试：配置解析、cap 默认值、旧存档回填、三种效果、上限 clamp、溢出浪费、负值下限与当前值压缩、拾取扩容、商店上架、未知效果回插 |
| `backend/tests/game_rule_engine_integration.rs` | 2 个解析测试（含旧配置无 `permanent_buffs` 键的兼容用例） |

## 三、前端改动（Vue 3 + TypeScript）

- **解析与校验**：`itemConfigUtils.ts`（接口 + 解析 + 重名检测）、`itemParser.ts`（`permanentBuffs` 列表、`hasAnyItem`）、`gameRuleParser.ts`（类别白名单、`effect_type` 白名单、`effect_value` 非零校验——负数表示降低上限）、`gameStateTypes.ts`（`ItemCategory` 联合类型 + 玩家新字段）、`itemType.ts`（类别标签映射）
- **导演端 UI**：批量空投弹窗新分类块（`BatchAirdropDialog.vue`）、商店管理分组（`ShopManagement.vue`）、规则预览新 tab（`GameRulesPreview.vue`）
- **玩家端 UI**（实测发现遗漏后补上）：背包面板使用键白名单 + 分组排序 + 属性行展示（`InventoryPanel.vue`、`itemDisplay.ts`，负值显示如 `-50`）

## 四、配置与文档

- **跟踪配置**：`full-feature-rules-template.json`、`frontend/src/constants/defaultRulesConfig.ts` 加入 caps 与 9 个默认道具
  - 正面道具：[HP上限+20]养生丸 / [HP上限+50]壮骨丹 / [HP上限+100]金钟罩 / [MP上限+20]干粮 / [MP上限+50]行军丹 / [MP上限+100]龙力丸 / [背包+2]腰包 / [背包+4]行囊 / [背包+6]百宝袋
- **本地导演配置**（`director_rules_config.json`，gitignore 不入库）：共 18 个道具 = 9 正 + 9 负
  - 削弱道具：[HP上限-20]蚀骨散 / [HP上限-50]衰弱咒 / [HP上限-100]枯萎蛊 / [MP上限-20]迷魂香 / [MP上限-50]软筋散 / [MP上限-100]缚魂咒 / [背包-2]破损袋 / [背包-4]漏底包 / [背包-6]破麻袋
- **文档**：`docs/api/game-rules-config.md`、`docs/api/data-models.md`、`docs/api/ws/player-actions.md`、`frontend/public/docs/game-rules-explain.md`

## 五、质量过程

superpowers 全流程：头脑风暴（8 个决策点确认）→ 规格文档 → 实现计划（9 任务）→ 子代理逐任务实现 + 规格与质量双维审查（9 任务全部通过）→ 全分支终审（结论：可合并）→ 本地实测补漏 2 处（玩家背包 UI、负值支持）。

- 后端测试：**139 个全绿**（基线 124 + 新增 15）
- 前端：`vue-tsc` 类型检查 + `vite build` 干净
- 终审遗留 10 条 Minor 建议（测试断言收紧、无用参数清理等），均为可随行级别，不影响功能

## 六、提交清单（11 个，均已进 main）

```
2d2e459 feat(items): permanent buff items support negative effect values to lower caps
0365292 fix(frontend): enable use button and display for permanent buff items in player inventory
ff51bd2 docs+fix: clarify permanent buff docs and guard negative effect values
63c3024 docs: permanent buff category documentation
31c9431 feat(frontend): permanent buff category in airdrop, shop and rules preview
c308279 feat(frontend): parse and validate permanent_buff category
b72747b chore(config): add caps and 9 permanent buff items to rule configs
c1578b0 refactor(inventory): backpack capacity checks read per-player field
0088d92 feat(items): permanent buff use action with cap clamp
79bc94e feat(player): per-player backpack capacity with configurable hard caps
98732c0 feat(items): add permanent_buff item category to rule engine
```

## 七、备注

- 2026-08-25 测试中"无法开始游戏"为数据库中 2026-05 的旧规则模板缺 `safe_places` 字段所致（数据问题，非代码缺陷），更换模板后解决
- 既有缺陷备忘：无效规则配置在"开始游戏"时才 panic（500），建游戏时缺少前置校验，后续可改进
