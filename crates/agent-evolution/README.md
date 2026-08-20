# evolution

离线进化引擎 -- 从执行轨迹中自动发现可复用模式，生成技能候选并经评审、门禁、遗传搜索筛选后交由用户审批。

## 核心职责

1. **反思与候选生成** -- 读取 DecisionLog（工具失败 / 用户纠错）和已启用 Skills 索引，通过 LLM 反思产出技能候选（新建 / patch）。LLM 调用不在本 crate，调用方注入模型并使用本 crate 提供的 system prompt 与解析器。
2. **门禁与评审** -- 对候选做静态检查（体积、patch 结构、路径安全）和 LLM judge 评分（0-1），低于阈值的候选被过滤。支持 tempdir 和 Docker 两种沙箱测试模式。
3. **GEPA-lite 遗传搜索** -- 内置变异、交叉、Pareto 多目标选择（judge 分 vs 体积 vs 测试通过率），通过 `SearchBudget` 严格控制 LLM 调用上限。
4. **评测集与适应度** -- 用户标注的 task + expectations 评测集，支持 optimize / holdout 划分、Fail 加权评分、结构化 critique 聚合。
5. **策展与自动触发** -- Curator 定期生成技能库健康报告（闲置检测、重叠聚类、健康评分），Auto 模块管理自动触发的冷却、日限、决策水位等护栏。

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | crate 入口，re-export 全部子模块的公共 API |
| `reflect.rs` | Reflection：构造反思提示词 (`REFLECTION_SYSTEM_PROMPT`)，解析模型输出为 `SkillCandidate` |
| `candidate.rs` | 候选类型定义：`CandidateKind`（NewSkill / Patch / Disable / Merge）与 `SkillCandidate` |
| `judge.rs` | Judge：构造评审提示词 (`JUDGE_SYSTEM_PROMPT`)，解析评分 (`JudgeVerdict`) |
| `gates.rs` | 候选门禁：体积 / patch 结构校验、tempdir / Docker 沙箱测试 (`TestOutcome`) |
| `search.rs` | GEPA-lite 遗传搜索：`SearchBudget`、`ScoredVariant`、Pareto 前沿、变异 / 交叉提示词 |
| `evalset.rs` | 标注评测集：JSONL 持久化、optimize/holdout 划分、grounded eval judge、加权适应度 |
| `proposal.rs` | 提案队列：候选落盘为 JSON、审批 / 拒绝 / 回滚、写入 Agent skills 目录 |
| `history.rs` | 进化可观测：JSONL 追加式历史记录、run / outcome 事件、聚合统计 (`HistorySummary`) |
| `auto.rs` | 自动触发护栏：冷却时间、日限、决策水位检测 (`AutoGate`)、状态持久化 |
| `curator.rs` | 技能库策展：健康报告生成、闲置检测、重叠聚类 (Jaccard)、LLM 诊断、周期调度 |
| `signal.rs` | 失败信号提取：从 DecisionLog 按 skill 统计 ToolFailure / UserCorrection 计数 |
| `opportunities.rs` | Mutation Hints 探测器：高纠错率 / 大体积 / 闲置 / 低健康分四个本地探测器 |

## 核心类型与 API

### 类型

- `SkillCandidate` -- 进化候选，含 id / kind / skill_id / content / old_string / new_string / judge_score
- `CandidateKind` -- NewSkill / Patch / Disable / Merge
- `JudgeVerdict` -- judge 评审结果：score (0-1) / keep / reason
- `GateOutcome` -- 门禁结果：passed + reasons
- `TestOutcome` -- 沙箱测试结果：Passed / Failed / NotApplicable
- `ScoredVariant` -- 评分变体：candidate + score + size + test_pass
- `SearchBudget` -- LLM 调用预算控制
- `EvalExample` / `EvalJudgement` / `EvalSplit` / `FitnessResult` -- 评测集相关
- `AutoState` / `AutoGate` / `AutoStatus` / `SkipReason` -- 自动触发状态与护栏
- `CurateReport` / `CurateSkillRow` / `CurateSuggestion` / `CuratorDue` -- 策展报告
- `HistoryEvent` / `HistorySummary` / `SearchRunMeta` -- 历史记录与统计
- `ReflectionInput` -- 反思输入：decisions + enabled_skills + transcripts + focus_skill
- `OpportunityHint` / `SkillSignalSummary` -- 信号与探测器输出

### 主要函数

- `build_reflection_user_prompt()` / `parse_candidates()` -- 反思提示词构造与解析
- `build_judge_user_prompt()` / `parse_judge_output()` -- 评审提示词构造与解析
- `check_candidate()` / `sandbox_test_candidate()` -- 静态门禁与沙箱测试
- `pareto_front()` / `select_population()` / `select_front_capped()` -- Pareto 选择
- `build_mutation_prompt()` / `build_crossover_prompt()` / `parse_variants()` -- 变异 / 交叉
- `save_proposals()` / `list_proposals()` / `approve_proposal()` / `reject_proposal()` -- 提案管理
- `evaluate_auto_gate()` / `mark_auto_run()` / `build_auto_status()` -- 自动触发
- `run_curator()` / `run_curator_and_save()` / `evaluate_curator_due()` -- 策展
- `skill_failure_signals()` / `top_failing_skill()` -- 信号提取
- `detect_opportunities()` -- 探测器驱动的 mutation hints

### 常量 (System Prompts)

- `REFLECTION_SYSTEM_PROMPT` / `JUDGE_SYSTEM_PROMPT` / `EVAL_JUDGE_SYSTEM_PROMPT`
- `MUTATION_SYSTEM_PROMPT` / `CROSSOVER_SYSTEM_PROMPT` / `CURATOR_DIAGNOSE_SYSTEM_PROMPT`

## 与其他 crate 的关系

- **types** -- 共享类型基础
- **home** -- `~/.astro` 路径约定、测试环境 (`AstroMemoryDirGuard`)
- **memory** -- `DecisionEntry` / `DecisionKind` / `EvolutionAuto` / `EvolutionGates` 等配置与数据结构
- **skills** -- 技能加载 / 安装 / 启用状态管理；进化产出的候选最终写入 skills 目录

本 crate 不直接调用 LLM；所有模型调用由上层（Tauri 命令层）注入目标并执行，本 crate 只负责提示词构造与输出解析。

## 测试运行命令

```bash
# 全部测试
cargo test -p evolution

# 单个测试函数
cargo test -p evolution pareto_drops_dominated
cargo test -p evolution sandbox_passing_new_skill -- --nocapture
```
