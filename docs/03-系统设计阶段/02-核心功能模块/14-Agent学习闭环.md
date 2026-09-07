# Astro Agent 学习闭环

Astro 将「可复用工作流」固化为 **Skills（程序性记忆）**，将「长期偏好/事实」写入 **Memory**，并在对话中用 **nudge** 提醒 Agent 何时沉淀经验。完整学习闭环分三层：

| 层 | 状态 | 说明 |
|----|------|------|
| **运行时闭环** | 本期已落地 | Agent 用 `skills` / `memory` 建改；回合后复杂任务 nudge；`curate` 修剪建议 |
| **记忆 review** | 已有 | 回合后 `auxiliary.background_review` 精炼 MEMORY/USER（见 [`memory.md`](./memory.md)） |
| **离线进化** | 已落地（Rust GEPA-lite + 可选 DSPy） | 读轨迹→反思/搜索产候选→门禁/打分→应用内待审批；完整外部 GEPA 为可选扩展 |

设计规格：[`docs/superpowers/specs/2026-07-17-agent-learning-loop-design.md`](./superpowers/specs/2026-07-17-agent-learning-loop-design.md)。

---

## 运行时流程

```text
复杂任务成功 / 纠错找到通路
        │
        ▼
  learning nudge（下一轮 system）
        │
   ┌────┴────┐
   ▼         ▼
 skills    memory
 create/   add/replace
 patch
        │
        ▼
 skills curate（建议 disable/delete，不自动删）
```

### 何时沉淀 Skill

- 本轮工具调用次数 ≥ `learning.complex_task_tool_threshold`（默认 5）且任务成功  
- 踩坑后找到正确路径，应 `manage_action=patch` 写回已有 Skill  
- 用户纠正了步骤或发现可复用工作流  

持久偏好、环境事实走 **`memory`**，不要写进 Skill 正文。

### 工具契约（`skills`）

| action | 作用 |
|--------|------|
| `list` | 已启用 name + description |
| `load` / `view`（默认） | 加载 SKILL.md + 路径/scripts |
| `curate` | 启用列表 + 上次加载时间 + 闲置建议 |
| `manage` | `create` / `update` / `patch` / `delete`（仅 Agent `workspace/skills/`） |

`patch`：`old_string` 须在 SKILL.md 中**唯一**匹配，再替换为 `new_string`（对齐 `file_ops.patch`）。

---

## 配置

路径：`~/.astro/config.yaml` 的 `learning:` 段。

```yaml
learning:
  nudge_enabled: true                 # 复杂任务后是否在下一轮注入提示
  complex_task_tool_threshold: 5      # 本轮工具次数达到该值视为「复杂」
  unused_skill_days: 30               # curate 将更久未加载的技能标为闲置建议
```

| 键 | 默认 | 说明 |
|----|------|------|
| `nudge_enabled` | `true` | 关闭后不注入复杂任务 / DecisionLog 失败强化提示 |
| `complex_task_tool_threshold` | `5` | 上一轮工具轮次 ≥ 该值则下一轮挂起学习 nudge |
| `unused_skill_days` | `30` | `curate` 建议阈值 |

使用统计文件：`~/.astro/learning/skill-usage.json`（`load` 成功时更新 `last_loaded_at`）。

---

## 与记忆系统的关系

- **MEMORY / USER**：有界事实与偏好；可选 `write_approval`、background review、入梦。见 [`memory.md`](./memory.md)。  
- **Skills**：可执行流程与脚本索引；Agent 主动 `manage`，本系统**不**在 Done 后自动写 Skill。  
- **DecisionLog**：`~/.astro/learning/decisions.jsonl`；近期 `ToolFailure` 可强化「把正确路径 patch 进 skill」提示。  
- **search**：跨会话原文 / 记忆 / 知识库按需召回；摘要仍由模型完成，不自动入库。

---

## Phase 2（离线进化）

离线遗传优化：读取执行轨迹 → 生成 Skill/提示变体 → 测试与体积门禁 → **应用内人工审批**（始终人审，非 git PR 自动流）。独立流水线，不改变运行时默认行为。

**专页文档**（架构 / 配置 / 三种模式 / 提案 / DSPy 契约 / 命令速查）：[`evolution.md`](./evolution.md)。

**已落地（Rust 内置引擎）**：模型服务页「离线进化」子 Tab 可配置 `evolution.enabled`、`reflection` / `judge` 路由与门禁（run_tests / max_skill_bytes / min_judge_score；`require_pr` 为始终人审不变量），并新增：

- **运行进化**：读 `learning/decisions.jsonl`（工具失败/用户纠错/关键决策等）+ 已启用技能索引 + **相关会话精简 transcript**（按决策的 session_id，最多 3 个）→ `reflection` 模型产出技能候选（新建 / patch）→ 静态门禁 → **`judge` 模型打分**（`min_judge_score` 阈值，0 关闭）→ 存待审提案（`~/.astro/learning/evolution/proposals/`）。
- **应用内审批**：提案在子 Tab 内以 diff + judge 评分展示，**批准**才写入 Agent skills 目录，**绝不自动应用**。
- **批准到分支**（可选）：若技能目录在某 git 仓库内，可「批准到分支」——在**独立 worktree** 的新分支 `astro/evolution/<skill>-<id>` 写入并 commit，不动当前工作树，产出分支供 review / 推送 / 开 PR；不在 git 仓库则回退普通批准。

实现：crate [`evolution`](../evolution)（candidate/reflect/gates/judge/proposal/search）+ Tauri `evolution_run_commands`（run/search/list/approve/reject）+ `EvolutionModelsPanel`。

`run_tests`（默认开）：
- **批准写入后**：若技能含 `scripts/test.sh` / `test.py` 则沙箱执行（60s 超时），失败自动回滚且保留提案；无脚本则跳过。
- **遗传搜索期间**：同一开关开启且目标技能含测试脚本时，候选在 tempdir 预跑，结果作为 Pareto 第三维（score↑ / test↑ / size↓）；无脚本记为 `not_applicable`，不视为失败。

DecisionLog 现覆盖：`ToolFailure`、`MemoryRejected`、`UserCorrection`（启发式识别用户纠错）、`KeyChoice`（`confirm` 决策闸口）。

**遗传搜索（GEPA-lite）**：「离线进化」页可选「遗传搜索」——reflection 产种子（按 skill_id+kind 去重取前 3）→ 多代变异（`generations` × `variants`）→ 可选交叉 → judge / grounded 打分 → **Pareto 选择（分↑ / 体积↓ / 可选测试↑）** + 结构化 critique 回喂 → 种群保留后过门禁入待审。页面可调代数、变体、种群、评测例上限与 **LLM 预算**（默认 `max_llm_calls=40`），并可 **定向某一技能**。

crate：[`evolution::search`](../evolution/src/search.rs)（`pareto_front` / `select_front_capped` / 变异提示与解析）；命令 `run_evolution_search`。

**评测集 + 客观适应度**：「离线进化」页可标注评测例子（task + 期望要点 + 曾通过/失败，可关联 skill_id），存 `~/.astro/learning/evolution/evalset.jsonl`。进化打分时，若候选技能有匹配例子，则由 judge 针对具体 task+expectations 做 **grounded 客观评分**（Fail 加权）；无匹配则回退泛化 judge。同一技能匹配例子 **≥5 条**时，稳定哈希划分约 **20% holdout**：遗传搜索仅在 optimize 分区上打分选型，最终候选在 holdout 上复验，降低过拟合评测集风险。支持从 DecisionLog 关联的**失败会话一键导入**（工具失败 / 用户纠错）。crate [`evolution::evalset`](../evolution/src/evalset.rs)；命令 `list/add/remove_eval_example`、`list_eval_import_candidates` / `import_eval_from_session`。

**交叉算子**：遗传搜索每代对当前 Pareto 前沿 top-2 变体做交叉（`CROSSOVER_SYSTEM_PROMPT` 融合两父代为一个子代），评分后并入本代选择。可在页面开关（`evolution.search.crossover`，默认开）。

**Python DSPy 对接**（默认关）：外部 Python 子项目 [`evolution-dspy/`](../evolution-dspy/)（独立包，非 cargo）通过「临时目录 + JSON 文件」契约被调用——Rust 导出 `skill.md` + `evalset.jsonl` + `config.json`（key 走环境变量）→ `python -m evolution_dspy optimize` 跑 DSPy+GEPA → 回写 `result.json` → 转成候选过门禁入待审队列。命令：`evolution_dspy_status` / `setup_evolution_dspy`（建 venv + pip install）/ `run_evolution_dspy(skill_id)`。配置 `evolution.dspy { enabled=false, python_bin, project_path, timeout_secs }`。

打包形态：源码随 app 作 resource（只读），venv 与临时数据在用户可写的 `~/.astro/evolution-dspy/.venv` 与 `~/.astro/learning/evolution/dspy-run-*/`；首次用 UI「安装依赖」建 venv。DSPy/GEPA API 随版本变，Python 侧带回退（GEPA 不可用退化单轮反思），标 `# ADAPT:` 处按版本调整。

**mock 自测**：`run_evolution_dspy(mock=true)` / CLI `--mock` 不调用 dspy、不需凭据，产确定性候选，用于验证「Rust 导出 → 子进程 → result.json → 提案入队」整条契约是否打通（UI「mock 自测」按钮）。

学习闭环三阶段（评测集/交叉/DSPy）至此全部落地；另含 **技能策展（Curator）**：手动运行结构化健康报告（闲置 / 体积 / 进化采纳 / 评测 Fail / 描述重叠）；可选将 **Disable / Merge** 建议入待审队列（批准后才禁用或合并，绝不自动删改）；Rewrite 建议可跳转定向遗传搜索。`evolution.curator`（默认关）含 `interval_days` / `max_enqueue` / `llm_diagnose`。开启后到期（启动或 Chat Done）仅自动刷新启发式报告，**不入队、不调 LLM**；入队与诊断仍需显式操作。

**可观测**：进化运行与提案去向记入 `~/.astro/learning/evolution/history.jsonl`（`run` / `outcome` 事件）；「离线进化」页「进化历史」小节展示运行次数、生成提案数、采纳率、采纳均分、采纳/拒绝与近期事件。crate [`evolution::history`](../evolution/src/history.rs)；命令 `evolution_history`。
**自动触发**（默认关）：Chat Done 后 fire-and-forget 调用 `maybe_run_evolution_auto`。需同时开启 `evolution.enabled` 与 `evolution.auto.enabled`；仅跑**单轮 reflect**（不跑遗传搜索/DSPy），产物只入待审。成本护栏：

| 键 | 默认 | 说明 |
|----|------|------|
| `evolution.auto.enabled` | `false` | 自动触发总开关 |
| `cooldown_secs` | `3600` | 两次自动运行最小间隔（秒） |
| `min_new_decisions` | `3` | 自上次运行以来 DecisionLog 新增条数下限 |
| `max_runs_per_day` | `3` | 每个 UTC 自然日最多自动运行次数 |

状态水位：`~/.astro/learning/evolution/auto_state.json`。失败也会记冷却，避免热重试烧钱。crate [`evolution::auto`](../evolution/src/auto.rs)；命令 `evolution_auto_status` / `set_evolution_auto` / `maybe_run_evolution_auto`。

模型角色（不复用在线 `auxiliary.*`）：

| 角色 | 职责 | 模型倾向 |
|------|------|----------|
| reflection | 读 trace 诊断失败并提出改写 | 强推理模型 |
| judge | 对候选判分 | 可省或中等模型 |
| target | 被优化对象实际运行 | 生产主模型 |

详见规格 [`specs/2026-07-17-agent-learning-loop-design.md`](./superpowers/specs/2026-07-17-agent-learning-loop-design.md)。
