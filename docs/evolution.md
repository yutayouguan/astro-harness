# Astro 离线进化（Evolution）

Astro 将「可复用工作流」固化为 **Skills**。离线进化在**不改变运行时默认行为**的前提下，从执行轨迹与评测集中提出技能候选（新建 / patch），经门禁与评审后进入**待审提案队列**；只有用户在应用内批准后才会写入 Agent skills 目录。

本页专述 Evolution 模块（Rust crate + 可选 Python DSPy）。学习闭环总览见 [`learning-loop.md`](./learning-loop.md)；设计规格见 [`specs/2026-07-17-agent-learning-loop-design.md`](./superpowers/specs/2026-07-17-agent-learning-loop-design.md)。

---

## 定位与非目标

| 项 | 说明 |
|----|------|
| **定位** | 离线流水线：读轨迹 → 产出候选 → 门禁 / 打分 → 人工审批 → 写入 skills |
| **入口** | 设置 → 模型服务 →「离线进化」子 Tab（`EvolutionModelsPanel`） |
| **默认** | `evolution.enabled = false`；DSPy 另有独立开关且默认关 |
| **非目标** | 运行时自动改 Skill、完整外部 DSPy/GEPA 遗传引擎内置、git/PR 全自动（「批准到分支」仅为可选辅助）；产物始终人工审批 |

LLM 调用**不在** `evolution` crate 内：crate 只负责提示词、解析、门禁、落盘与审批；Tauri 注入已解析的 reflection / judge 路由并执行模型调用。

---

## 架构

```text
DecisionLog + Skills 索引 (+ transcript / evalset)
              │
              ▼
     ┌────────────────┐
     │ reflection /   │  单轮反思 或 GEPA-lite 搜索 或 DSPy 子进程
     │ mutation /     │
     │ DSPy+GEPA      │
     └───────┬────────┘
              │ SkillCandidate[]
              ▼
         静态门禁 (gates)
              │
              ▼
         judge / grounded eval
              │  score ≥ min_judge_score
              ▼
   ~/.astro/learning/evolution/proposals/
              │
         用户批准 / 拒绝 / 批准到分支
              │
              ▼
     Agent skills 目录（可选 scripts/test.* 后置校验）
```

### 代码地图

| 路径 | 职责 |
|------|------|
| [`evolution/`](../evolution/) | Rust 内置极简引擎（cargo workspace 成员） |
| `evolution/src/candidate.rs` | `SkillCandidate` / `CandidateKind` |
| `evolution/src/reflect.rs` | reflection 提示词与候选解析 |
| `evolution/src/gates.rs` | 体积与 patch 结构门禁 |
| `evolution/src/judge.rs` | 泛化 judge 提示词与解析 |
| `evolution/src/evalset.rs` | 标注评测集 + grounded 评分提示 |
| `evolution/src/search.rs` | GEPA-lite：变异 / 交叉 / Pareto |
| `evolution/src/curator.rs` | 技能策展：健康报告、Disable/Merge 入队、LLM 诊断提示 |
| `evolution/src/proposal.rs` | 提案队列、批准写入、唯一 patch |
| `evolution/src/history.rs` | run / outcome 可观测 JSONL |
| [`evolution-dspy/`](../evolution-dspy/) | 外部 Python 包（**非** cargo）；DSPy+GEPA |
| `frontend/src-tauri/src/evolution_run_commands.rs` | Tauri 命令：run / search / approve / dspy / evalset |
| `apps/desktop/src/components/settings/EvolutionModelsPanel.tsx` | UI |
| `memory` crate `EvolutionConfig` | `config.yaml` 的 `evolution:` 段 |

---

## 配置

路径：`~/.astro/config.yaml` 的 `evolution:` 段。

```yaml
evolution:
  enabled: false
  reflection:
    provider: auto          # 或具体 provider id
    model: auto             # 建议强推理模型
  judge:
    provider: auto
    model: auto             # 可省或中等模型；min_judge_score≤0 时关闭打分
  gates:
    run_tests: true
    max_skill_bytes: 15360
    require_pr: true        # 始终人审（读写强制 true；不可关）
    min_judge_score: 0.6    # ≤0 关闭 judge 过滤
  search:
    generations: 2
    variants: 3
    crossover: true
    population_size: 3      # 每代 Pareto 保留上限
    max_eval_examples: 5    # 每候选评测例上限；0 = 不限
    max_llm_calls: 40       # 单次搜索 LLM 硬预算；0 = 不限
  auto:
    enabled: false
    cooldown_secs: 3600
    min_new_decisions: 3
    max_runs_per_day: 3
  curator:
    enabled: false          # 仅间隔提示/报告；默认不自动入队
    interval_days: 7
    max_enqueue: 5          # 单次入队 Disable/Merge 上限
    llm_diagnose: false     # 用 judge 路由增强建议 reason
    max_llm_calls: 3
  dspy:
    enabled: false
    python_bin: ""          # 空 = 运行时解析 venv / 系统 python3
    project_path: ""        # 空 = resource / 仓库内 evolution-dspy
    timeout_secs: 600
```

| 键 | 默认 | 说明 |
|----|------|------|
| `enabled` | `false` | 总开关；关则 UI 运行入口不可用 |
| `reflection` | `auto/auto` | 反思 / 变异路由 |
| `judge` | `auto/auto` | 候选评审路由 |
| `gates.run_tests` | `true` | **批准后**沙箱跑 `scripts/test.*`（失败回滚）；**遗传搜索期间**同开关开启时也预跑，作 Pareto 第三维 |
| `gates.max_skill_bytes` | `15360` | 候选写入体积上限（字节） |
| `gates.require_pr` | `true` | **始终人工审批**（产品不变量；配置读写强制为 true） |
| `gates.min_judge_score` | `0.6` | judge 阈值；`≤ 0` 关闭 |
| `search.generations` | `2` | 遗传搜索代数 |
| `search.variants` | `3` | 每代变体目标数 |
| `search.crossover` | `true` | Pareto 前沿 top-2 交叉 |
| `search.population_size` | `3` | 每代保留种群上限 |
| `search.max_eval_examples` | `5` | 每候选 grounded 评测例上限；`0` = 不限 |
| `search.max_llm_calls` | `40` | 单次搜索 LLM 调用硬顶；`0` = 不限 |
| `auto.*` | 见上 | Chat Done 后自动单轮 reflect；默认关 |
| `curator.enabled` | `false` | 策展提醒总开关；**不**自动删改技能 |
| `curator.interval_days` | `7` | 间隔（天）；到期后启动 / Chat Done 自动刷新**启发式报告**（不入队、不调 LLM） |
| `curator.max_enqueue` | `5` | 单次将 Disable/Merge 入待审上限 |
| `curator.llm_diagnose` | `false` | 手动运行策展时用 judge 路由增强 reason |
| `curator.max_llm_calls` | `3` | 单次策展诊断 LLM 调用上限 |
| `dspy.*` | 见上 | 外部 Python 引擎；默认关 |

模型角色不复用在线 `auxiliary.*`：

| 角色 | 职责 | 倾向 |
|------|------|------|
| reflection | 读 trace、提改写 / 变异 | 强推理 |
| judge | 对候选判分 | 中等即可 |
| target | 被优化技能实际运行时使用的生产模型 | 主模型（不在本流水线内调用） |

---

## 三种运行模式

### 1. 单轮反思（`run_evolution`）

1. 读 `~/.astro/learning/decisions.jsonl`（`ToolFailure` / `UserCorrection` / `KeyChoice` / `MemoryRejected` 等）
2. 附已启用技能索引 + 相关会话精简 transcript（按决策 `session_id`，最多 3 个）
3. `reflection` 模型产出最多约 3 条候选（`new_skill` | `patch`）
4. 静态门禁 → `judge`（或 grounded eval）打分
5. 通过者写入 `proposals/{id}.json`

### 2. GEPA-lite 遗传搜索（`run_evolution_search`）

1. reflection 产种子，按 `skill_id`+`kind` 去重取前 3
2. 每目标多代变异（`generations` × `variants`）
3. 可选交叉：当前 Pareto 前沿 top-2 融合为子代
4. 适应度：`judge` 分↑ / 体积↓ / 可选沙箱测试↑；有匹配评测集时用 **optimize 分区** grounded 评分，最终候选可选 **holdout 复验**
5. `pareto_front` + `select_front_capped` → 门禁 → 待审；`history.jsonl` 记录 `search_meta`（预算、holdout、沙箱、定向技能、路由、终止原因）

可选参数：`skill_id` 定向进化（只产该技能候选）。

成本粗估：≈ `目标数 × 代数 × 变体数` 次模型调用（交叉与 judge 另计），并受 `max_llm_calls` 硬顶（默认 40）。

### 3. 外部 DSPy（`run_evolution_dspy`）

独立包 [`evolution-dspy/`](../evolution-dspy/)（详见其 [README](../evolution-dspy/README.md)）。

契约：

```bash
python -m evolution_dspy optimize --input <dir> --output <dir>/result.json
```

| 文件 | 内容 |
|------|------|
| `skill.md` | 目标技能当前 SKILL.md |
| `evalset.jsonl` | `{skill_id?, task, expectations[], verdict}` |
| `config.json` | `{skill_id, model, base_url, provider_backend}` |
| 环境变量 `ASTRO_DSPY_API_KEY` | 模型凭据（不落盘） |
| `result.json` | 成功：`{skill_id, kind:"edit", content, score, rationale, log}`；失败：`{error}` 且退出码非零 |

打包：源码作 app resource（只读）；venv 在 `~/.astro/evolution-dspy/.venv`；临时数据在 `~/.astro/learning/evolution/dspy-run-*/`。UI「安装依赖」对应 `setup_evolution_dspy`。

**mock 自测**：`run_evolution_dspy(mock=true)` / CLI `--mock` 不调真实 dspy，验证「导出 → 子进程 → 提案入队」契约。

---

## 候选与门禁

### `SkillCandidate`

| 字段 | 说明 |
|------|------|
| `id` | uuid，提案文件名 |
| `kind` | `new_skill` \| `patch` \| `disable` \| `merge` |
| `skill_id` | kebab-case 等合法 id（无路径 / `..`） |
| `content` / `description` | 新建 SKILL.md（可无 frontmatter，批准时自动补） |
| `old_string` / `new_string` | patch：原文须在目标 SKILL.md 中**唯一**匹配 |
| `rationale` / `sources` | 理由与决策来源 |
| `judge_score` / `judge_reason` | 评审结果 |
| `created_at` | RFC3339 |

### 静态门禁（`check_candidate`）

- 内容非空；体积 ≤ `max_skill_bytes`
- patch：`old_string` 非空、`new_string` 存在且与 old 不同
- **唯一匹配**在批准 apply 时校验（技能可能已改）
- `run_tests`：批准后、有 `scripts/test.*` 才执行（60s 超时）；失败回滚文件并保留提案

---

## 提案审批

路径：`~/.astro/learning/evolution/proposals/{id}.json`。

| 操作 | 命令 | 效果 |
|------|------|------|
| 列出 | `list_evolution_proposals` | 按创建时间升序 |
| 批准 | `approve_evolution_proposal` | 写入 Agent skills；删除提案；可选跑 test |
| 拒绝 | `reject_evolution_proposal` | 删除提案 |
| 批准到分支 | `approve_evolution_proposal_to_branch` | 技能目录若在 git 仓内：独立 worktree + 分支 `astro/evolution/<skill>-<id>` commit；否则回退普通批准 |

UI 展示 diff + judge 分；**绝不自动应用**。

---

## 评测集

路径：`~/.astro/learning/evolution/evalset.jsonl`（append-only）。

每条例子：`task`、`expectations[]`、`verdict`（`pass`/`fail`）、可选 `skill_id` / `source_session`。

打分策略：

- 候选 `skill_id` 有匹配例子 → judge 按 task+expectations **grounded** 评分（多例取均值）
- 无匹配 → 回退泛化 judge

命令：`list_eval_examples` / `add_eval_example` / `remove_eval_example`；`list_eval_import_candidates` / `import_eval_from_session`（从含工具失败或用户纠错的会话一键导入，verdict 固定为 fail）。

匹配例子 ≥5 条时启用 **holdout**（约 20%，稳定 id 哈希）：搜索适应度仅用 optimize 分区，终选候选在 holdout 复验。

---

## 可观测历史

路径：`~/.astro/learning/evolution/history.jsonl`。

| `type` | 字段要点 |
|--------|----------|
| `run` | `mode`=`reflect`\|`search`\|`dspy`；`generated` / `gated_out` / `judged_out` / `proposals` |
| `outcome` | `proposal_id`、`skill_id`、`score`、`outcome`=`approved`\|`rejected`\|`branch` |

UI「进化历史」聚合：运行次数、生成提案数、采纳率、采纳均分、近期事件。命令：`evolution_history`。

---

## 落盘目录一览

```text
~/.astro/
  config.yaml                          # evolution: 段
  learning/
    decisions.jsonl                    # 输入：DecisionLog
    evolution/
      proposals/{id}.json              # 待审提案
      evalset.jsonl                    # 标注评测集
      history.jsonl                    # 运行与去向
      dspy-run-*/                      # DSPy 临时目录
  evolution-dspy/.venv/                # Python 虚拟环境（可选）
```

Agent skills 写入目标由 `skills` crate 的 agent skills 目录决定（通常为 Agent 工作区下 `skills/`）。

---

## Tauri 命令速查

| 命令 | 用途 |
|------|------|
| `run_evolution` | 单轮反思 |
| `run_evolution_search` | GEPA-lite 搜索（可选 `skill_id` 定向；holdout；沙箱三维 Pareto；history 写 search_meta） |
| `cancel_evolution_search` | 取消进行中的遗传搜索 |
| `run_skill_curator` / `get_curator_last` / `enqueue_curator_proposals` | 技能策展：报告（可选 LLM 诊断）/ 读取上次 / 将 Disable·Merge 入待审 |
| `curator_status` / `maybe_run_skill_curator` | 调度状态 / 到期自动刷新启发式报告（不入队） |
| `set_evolution_curator` | 读写 `evolution.curator`（enabled / interval / max_enqueue / llm_diagnose） |
| `list_evolution_proposals` | 待审列表 |
| `approve_evolution_proposal` | 批准写入 |
| `approve_evolution_proposal_to_branch` | 批准到 git 分支 |
| `reject_evolution_proposal` | 拒绝 |
| `evolution_history` | 历史汇总 |
| `list_eval_examples` / `add_eval_example` / `remove_eval_example` | 评测集 CRUD |
| `list_eval_import_candidates` / `import_eval_from_session` | 从失败会话导入评测例 |
| `evolution_dspy_status` / `setup_evolution_dspy` / `run_evolution_dspy` | DSPy 状态 / 装依赖 / 运行 |

配置读写经 `memory` crate 的 `load_evolution_config` / `set_evolution_*`，由 `evolution_commands` 暴露给前端。

---

## 与学习闭环其它层的关系

| 层 | 关系 |
|----|------|
| 运行时 `skills` 工具 | Agent 可主动 create/patch；进化是**离线批量提案**，不替代运行时 |
| DecisionLog | 进化的主要信号源 |
| Memory / review | 管 MEMORY/USER；不要把偏好写进 Skill，也不要在进化里改记忆 |
| learning nudge | 运行时提醒沉淀；进化是事后从轨迹挖掘 |

完整三层表见 [`learning-loop.md`](./learning-loop.md)。
