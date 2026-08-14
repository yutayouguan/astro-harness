# Agent 运行时学习闭环设计

**日期:** 2026-07-17  
**状态:** 已实现（P1 运行时）  
**范围:** Skills patch / curate、skill usage、learning 配置、prompt + 回合 nudge  
**外部参考（仅设计借鉴，不引入其品牌命名）：** 自我改进 Agent 的「create → maintain → evolve」闭环；evolve（离线遗传）为本规格 Phase 2。

## 命名约束

- **代码、模块、类型、文件、用户可见文案、路径中不得出现 `hermes` / `Hermes` 字样。**
- 推荐命名：`learning`、`LearningConfig`、`skill usage`、`curate`、`pending_learning_nudge`、`manage_action=patch`。

## 目标

1. Agent 能在对话中 **创建 / 精准 patch / 删除** Agent 工作区 Skills  
2. **Curator**：基于加载时间给出闲置建议，**不自动删除**  
3. **Nudge**：复杂任务后一回合注入短提示；固定 guidance 说明何时用 skills / memory  
4. 与现有 Memory review / DecisionLog 衔接，不重复造第二套记忆  

## 明确不做（P1）

- Done 后 auxiliary 自动写 Skill  
- 离线 GEPA / DSPy 遗传优化仓  
- 自动 delete / disable skill  
- 拆分 `file_ops` / 新增独立 toolset id  
- 结构化 EntityMemory / Honcho 式用户建模  

## 决策摘要

| 项 | 选择 |
|----|------|
| Skill 写入范围 | 仅 `agent_skills_dir`（Agent `workspace/skills/`） |
| 局部修改 | `manage_action=patch`，唯一 `old_string` |
| 自动提炼 | 否；靠工具 + nudge |
| Curator | `action=curate` 只建议 |
| 配置段 | `config.yaml` → `learning:` |
| Usage 路径 | `~/.astro/learning/skill-usage.json` |

## 架构

```text
config.yaml (learning.*)
        │
        ▼
 AgentLoop ── begin_user_turn ──► pending_learning_nudge
        │                              │
        │                              ▼
        │                    build_system_prompt (guidance + dynamic)
        ▼
 skills 工具 ── load ──► record skill-usage.json
             ── curate ──► 闲置建议
             ── manage patch/create/update/delete
```

## 配置

见 [`docs/learning-loop.md`](../../learning-loop.md)。默认：`nudge_enabled=true`，`complex_task_tool_threshold=5`，`unused_skill_days=30`。

## 安全

- `skill_id`：禁止路径分隔与 `..`；仅 `[A-Za-z0-9._-]`  
- delete/patch/update：canonical 路径必须落在 Agent skills 根下  
- Curator 输出仅为建议文案  

## Phase 2（离线进化，Rust 内置极简版已落地）

离线进化：轨迹 → 变体评测 → 门禁 → 人工审。已落地为 Rust 内置极简引擎（单轮反思 + 应用内审批），非完整 GEPA 遗传搜索。

落地位置：
- crate [`evolution`](../../../evolution)：`candidate`（候选类型）、`reflect`（提示词 + `parse_candidates`）、`gates`（体积/patch 结构门禁）、`proposal`（提案队列 + 审批应用到 agent skills）
- Tauri [`evolution_run_commands.rs`](../../../apps/desktop/src-tauri/src/evolution_run_commands.rs)：`run_evolution` / `list_evolution_proposals` / `approve_evolution_proposal` / `reject_evolution_proposal`；`reflection`/`judge` 目标经 [`auxiliary_resolver::resolve_evolution_targets`](../../../apps/desktop/src-tauri/src/auxiliary_resolver.rs) 解析
- UI：`EvolutionModelsPanel` 增「运行进化」+ 提案 diff 审批
- 数据：读 `learning/decisions.jsonl` + 已启用技能索引；提案存 `learning/evolution/proposals/{id}.json`；批准写入 `agent skills` 目录

仍为后续：完整 GEPA/Pareto 遗传搜索、`judge` 打分、`run_tests` 实跑、git 分支/PR 自动化、会话逐字 transcript 富化。

### 模型角色（不复用 `auxiliary.*`）

离线进化对模型的依赖比运行时闭环**更重且分角色**，不能简单套用现有「便宜辅助模型」：

| 角色 | 职责 | 模型倾向 |
|------|------|----------|
| **reflection（反思/变异）** | 读执行 trace，诊断「为什么失败」，提出针对性改写 | **强推理模型**，进化质量的关键 |
| **judge（评测）** | 对候选变体判分 / 对比；能自动判分时可省略 | 中等模型或规则；仅在 LLM-as-judge 时用 |
| **target（目标运行）** | 被优化的 Skill/prompt 实际运行 | **生产主模型**（须与线上一致，评测才有意义） |

与现有 `auxiliary.*`（`background_review` / `dreaming` / `compaction` 等）的区别：

- `auxiliary` = **在线、便宜、低风险旁路**，默认 `auto`（跟随会话主模型）。  
- `evolution` = **离线、批量、可接受慢与贵**（对标上游 ~$2–10 / run），reflection 若用便宜模型会拖低质量。  
- 产物是**候选 + PR**，绝不直接改线上。

因此 Phase 2 **单列** `evolution.*` 配置，不挤进 `auxiliary` 的五类；可复用 `AuxiliaryRoute` 的 `{provider, model}` 结构体，但语义与默认值独立。

### 配置（`config.yaml`，**已实现**：仅配置层，引擎未实现）

```yaml
evolution:
  enabled: false                # 离线进化总开关（默认关）
  reflection:
    provider: auto              # 建议显式指向强模型，不用 auto
    model: auto
  judge:
    provider: auto              # 留空 / auto 表示尽量走自动判分
    model: auto
  gates:
    run_tests: true             # 候选须通过测试
    max_skill_bytes: 15360      # Skill 体积上限（~15KB）
    require_pr: true            # 只开 PR，禁止直接落库
  auto:                         # 自动触发（Chat Done；默认关）
    enabled: false
    cooldown_secs: 3600         # 两次自动运行最小间隔
    min_new_decisions: 3        # 新增 DecisionLog 条数下限
    max_runs_per_day: 3         # UTC 日限额；仅单轮 reflect，绝不自动写入
```

落地位置：
- 配置读写：[`crates/agent-memory/src/config.rs`](../../../memory/src/config.rs)（`EvolutionConfig` / `EvolutionGates` / `EvolutionRouteKind`、`load_evolution_config`、`set_evolution_*`、`reset_all_evolution_routes`）
- Tauri 命令：[`apps/desktop/src-tauri/src/evolution_commands.rs`](../../../apps/desktop/src-tauri/src/evolution_commands.rs)
- UI：模型服务页「离线进化」子 Tab（[`EvolutionModelsPanel.tsx`](../../../apps/desktop/src/components/settings/EvolutionModelsPanel.tsx)），复用辅助模型的路由选择交互
- `provider=auto` 跟随会话主模型；显式值保存 UI Provider ID，与 `auxiliary` 一致但**配置段独立**

### 门禁（对齐上游）

1. 测试全过  
2. 体积上限（Skill ≤ ~15KB）  
3. 语义不漂移（保持原始意图）  
4. 人工 PR 审核，永不直接 commit  

### 自动触发（已实现，默认关）

Chat Done → Tauri `spawn_maybe_auto_evolution` → 护栏（总开关 + auto.enabled + 冷却 + 日限额 + 最低新决策）→ 单轮 `run_evolution_core(mode=auto)` → 仅入待审提案。状态水位 `learning/evolution/auto_state.json`；失败也记冷却防热重试。不跑遗传搜索 / DSPy，不自动 approve。

### 明确边界

- 独立 crate / 流水线，默认 `enabled=false`，不改变运行时默认行为。  
- 目标运行必须用生产主模型；reflection/judge 由 `evolution.*` 指定。  
- 自动触发仍须人工审批提案；命名同样禁止 `hermes` 字样。

## 验收

- [x] `docs/learning-loop.md` + 本规格  
- [x] `manage patch` 单测（0/1/多命中）  
- [x] `load` 写 usage；`curate` 标闲置  
- [x] `learning` 配置加载；复杂回合后下一轮 guidance/dynamic 含 nudge  
- [x] 无用户可见 `hermes` 字样  

**状态:** 已实现（P1 运行时）  
