# GEPA-lite 设计与实现

> Astro 离线技能进化引擎的完整技术文档。覆盖动机、架构决策、五阶段强化路线、关键算法、验收标准与已知边界。
>
> API 级参考（配置键、Tauri 命令、落盘路径）见 [`evolution.md`](./evolution.md)。

---

## 1. 动机

Astro 的 Skills 是可复用工作流的载体。运行时 Agent 可以主动 create/patch 技能，但**离线进化**的价值在于：从**历史执行轨迹**（DecisionLog）中批量挖掘改进机会，以遗传搜索而非单次反思的方式探索候选空间，经评分和门禁后产出**待审提案**——全程人工审批，绝不自动写入。

初始实现是"LLM-as-judge + 二维 Pareto + critique 回喂"的骨架，距离真正的 GEPA（Guided Evolution for Prompt & Agent）还有五个核心短板：

| 短板 | 影响 |
|------|------|
| 搜索出口不吃 `min_judge_score` | 低分候选混入提案 |
| Fail/Pass 例同等均值 | 修复缺陷的候选无优势 |
| 单精英谱系 | 多样性弱，早熟收敛 |
| 成本随 `候选 × 代数 × eval例` 放大且无硬预算 | 无法预估费用 |
| patch 体积与 new_skill 不可比 | Pareto 偏短补丁 |

强化策略：**先修契约，再扩搜索，最后接执行反馈**——在 judge 噪声下盲目加大 generations/variants 只是烧钱。

---

## 2. 架构概览

```text
DecisionLog + Skills 索引 + evalset + transcripts
              │
              ▼
     ┌────────────────────┐
     │  reflection        │  种子产生（最多 3 个目标）
     └────────┬───────────┘
              │ seeds[]
              ▼
     ┌────────────────────┐
     │  遗传搜索循环       │  每代：mutation → fitness → sandbox → Pareto → population
     │  (generations × K) │  可选 crossover（Pareto top-2 融合）
     └────────┬───────────┘
              │ population → select_front_capped(2)
              ▼
     ┌────────────────────┐
     │  holdout 复验       │  仅 ≥5 例且 optimize/holdout 划分启用时
     └────────┬───────────┘
              │
              ▼
         静态门禁 + min_judge_score
              │
              ▼
   proposals/{id}.json  →  用户批准 / 拒绝 / 批准到分支
              │
              ▼
     Agent skills 目录（批准后可选 sandbox 跑 test）
```

**关键约束**：`evolution` crate 不做 LLM 调用——只提供提示词构造、解析、门禁、落盘。Tauri 侧注入 provider 路由并执行调用。

---

## 3. 五阶段强化路线

### Phase 1：稳定质量契约与硬预算

#### Task 1 — `SearchBudget` 严格 LLM 调用上限

**问题**：变异调用前检查预算，但 `fitness_score` 内连续评多个 eval 例，实际调用数可超出 `max_llm_calls`。

**方案**：引入 `SearchBudget` 对象（[search.rs](../evolution/src/search.rs)），所有 LLM 调用方（seed reflection、mutation、crossover、judge、grounded eval）必须在请求前调用 `try_reserve_one()`，失败则不发起。`limit == 0` 表示不限。

```rust
pub struct SearchBudget { limit: u32, used: u32 }
// try_reserve(n) — 充足则扣减返回 true；不足则不扣减返回 false
// try_reserve_one() — 循环内逐次消费的快捷方法
```

**验收**：`search_budget_never_exceeds_limit` / `zero_budget_means_unlimited` / `search_budget_no_partial_reserve` 三个单测覆盖。

#### Task 2 — 统一 Pareto 体积目标 + patch dry-run

**问题**：`payload_len()` 对 new_skill 取 `content.len()`，对 patch 只取 `new_string.len()`——短 patch 获得系统性优势。且 old_string 不唯一要到批准时才失败。

**方案**：`effective_candidate_size(candidate, current_skill)` 纯函数（[search.rs](../evolution/src/search.rs)）：

- NewSkill → `content.len()`
- Patch → apply `old → new` 到 `current_skill` 后的 **post-image 全文长度**；未命中 / 多命中返回错误

搜索循环在 LLM judge 前 dry-run，失败计入 `gated_out` 而不消耗 judge 调用。`ScoredVariant::with_size` 接受已算好的 `effective_size`。

`check_candidate` 签名同步更新为 `(c, gates, current_skill: Option<&str>)`，内部也用 `effective_candidate_size`。

### Phase 2：从单精英升级为小种群

#### Task 3 — 可配置的小种群

**问题**：每代只沿单一 `current` 继续，多样性弱。

**方案**：

- `population_size`（默认 3，clamp 1–8）控制每代保留个体数
- `candidate_fingerprint(c)` 用 `kind + skill_id + content/old→new` 去重
- `select_population(scored, K)` 纯函数：Pareto front 优先 → 不足时从被支配集按 score↓ size↑ 补齐 → 去重后截断到 K
- 每代 mutation 从 population round-robin 选父代，而非固定 `current`
- `population_size=1` 退化为旧行为（兼容）

#### Task 4 — 结构化 critique

**问题**：`judge_reason` 是 free-text，回喂变异器效果受限。

**方案**：

- `EvalJudgement { score, satisfied: Vec<String>, unmet: Vec<String>, reason }` 结构化结果
- `EVAL_JUDGE_SYSTEM_PROMPT` 要求输出 `{"score":0.7,"satisfied":["..."],"unmet":["..."],"reason":"..."}`
- `parse_eval_judgement(raw)` 容忍围栏和缺失数组
- `aggregate_critiques(judgements, max)` 聚合 unmet 项（去重、按频次排序、截断 8 条）
- `build_mutation_prompt` 分两栏：**必须修复的缺口** + **必须保留的已有能力**

### Phase 3：降低过拟合并提高可复现性

#### Task 5 — 稳定的 optimize/holdout 划分

**问题**：同一 evalset 同时用于每代优化与最终报告，搜索可能过拟合 judge 和 expectations。

**方案**：

- `EvalSplit { optimize, holdout, holdout_enabled }` + `split_eval_examples(examples, holdout_percent)`
- 少于 5 条匹配例子：全部 optimize，holdout 关闭
- 基于 example id 的稳定哈希（`stable_bucket`）分区，增删其它例子不改变已有分区
- Fail/Pass 分层：holdout 保证同时含两种 verdict
- 每代 fitness 仅用 optimize 分区；搜索结束后 Pareto 前沿候选跑一次 holdout 复验
- history 记录 `optimize_examples` / `holdout_examples` / `holdout_enabled`

#### Task 6 — 可复现实验摘要

`SearchRunMeta` 记录所有可复现信息：

| 字段 | 说明 |
|------|------|
| `generations` / `variants` / `population_size` / `crossover` | 搜索参数 |
| `budget_limit` / `budget_used` | LLM 预算 |
| `optimize_examples` / `holdout_examples` / `holdout_enabled` | 数据规模 |
| `sandbox_used` / `sandbox_skills` | 沙箱使用 |
| `focus_skill` | 定向技能（空 = 未定向） |
| `reflection_model` / `judge_model` | 模型路由（不含 key） |
| `termination` | `completed` / `budget_exhausted` / `cancelled` / `no_candidates` |

旧 history 行缺少新字段时仍能读取（`#[serde(default)]`）。

### Phase 4：可选执行反馈

#### Task 7 — 沙箱预跑执行反馈

**问题**：评分完全依赖 LLM judge，无客观执行验证。

**方案**：

- `TestOutcome { Passed, Failed(String), NotApplicable }` 枚举
- `run_skill_tests_in_dir(skill_dir, timeout)` — 查找并运行 `scripts/test.{sh,py}`，清除敏感环境变量
- `sandbox_test_candidate(cand, current_skill, test_scripts_dir, timeout)` — 候选 apply 到 tempdir → 复制 test scripts → 运行 → 返回 TestOutcome
- `TestOutcome::fitness()` → `Some(1.0)` / `Some(0.0)` / `None`
- **三维 Pareto**：`ScoredVariant` 增加 `test_pass: Option<f32>`

  ```
  score↑ × test_pass↑ × size↓
  ```

- `dominates()` 对 `test_pass` 的三种 match：
  - `(Some(a), Some(b))` → 正常比较
  - `(None, None)` → 退化二维
  - 混合 `(Some, None)` / `(None, Some)` → **不可比较**（阻止支配）
- `sort_scored()` 统一排序：score↓ → test_pass↓（None 排末）→ size↑
- 由 `gates.run_tests` 开关控制；关闭时所有候选 `test_pass = None`

### Phase 5：Curator — 技能库健康维护

#### Task 8 — 结构化策展报告

对标 Hermes Curator：防止技能库因持续进化而膨胀/腐化。

**方案**（[curator.rs](../evolution/src/curator.rs)）：

- `CurateReport` 结构化输出：启用技能列表 → 每项 `CurateSkillRow`（health_score + reasons）
- 健康分信号（纯启发式，无 LLM）：

  | 信号 | 来源 | 影响 |
  |------|------|------|
  | 闲置 | `skill-usage.json` last_loaded | 降到 0.35 / stale |
  | 体积 | SKILL.md 字节数 | > 15KB 扣 0.1 |
  | 进化采纳 | history.jsonl approved/rejected | 采纳多加 0.1，拒绝多扣 0.15 |
  | 评测 Fail | evalset.jsonl 匹配 Fail 占比 | ≥ 50% 扣 0.2 |

- `CurateSuggestion::Disable` / `Rewrite` 根据闲置和评测信号生成

#### Task 9 — 重叠检测 + Merge 建议

- `tokenize(text)` — 名称+描述分词，过滤 < 2 字符
- `jaccard(a, b)` — token 集合 Jaccard 相似度
- `find_overlap_clusters(skills, threshold)` — 阈值 0.5，Union-Find 单链接聚合
- `CurateSuggestion::Merge { keep, absorb, reason }` — keep 选择：非 stale > stale → health 更高 → 字母序
- `CandidateKind::Disable` / `Merge` 变体用于策展提案
- `enqueue_curator_suggestions(base, report, max)` — 将建议转为 `SkillCandidate` 入待审队列，上限 `max_enqueue`

#### Task 10 — LLM 辅助策展诊断

- `CURATOR_DIAGNOSE_SYSTEM_PROMPT` — 一句可操作诊断（≤ 80 字），JSON only
- `build_diagnose_prompt(suggestion, rows)` — 构造单条建议的 context
- `parse_diagnose_output(raw)` → `Option<String>`，失败静默回退
- `apply_diagnoses(suggestions, diagnoses)` — 替换 reason（保留原因作后缀）
- 由 `curator.llm_diagnose` 开关控制（默认关），预算 `curator.max_llm_calls`（默认 3）
- 使用 judge 路由，不额外引入模型角色

---

## 4. 适应度函数设计

### Fail 加权

`weighted_eval_score(verdicts: &[(Verdict, f32)])`:

- `Verdict::Fail` 权重 2.0
- `Verdict::Pass` 权重 1.0
- 加权均值 clamp 到 [0, 1]

背景：修复已知失败比维持通过率更有价值。

### 例数上限

`max_eval_examples`（默认 5）：超出时截断，**Fail 例优先保留**（排序后截断）。防止大型评测集导致成本线性放大。

### 适应度路径

```
候选有匹配 eval 例?
  ├─ 是 → split_eval_examples → optimize 分区
  │       → 逐例 grounded eval（预算逐次扣减）
  │       → weighted_eval_score（Fail 双权重）
  │       → 搜索结束后 holdout 复验（仅 front 候选）
  └─ 否 → 泛化 judge（单次调用）
           失败 → None（fail-closed，不以 0.5 中性分进入 Pareto）
```

---

## 5. 遗传搜索循环（伪代码）

```
budget = SearchBudget::new(max_llm_calls)
budget.try_reserve_one()  // seed reflection

for seed in seeds (最多 3):
    population = [ScoredVariant::new(seed, 0.0)]
    current_skill = load_skill_by_name(seed.skill_id)
    test_scripts_dir = skill_dir/scripts (if run_tests && exists)

    for gen in 0..generations:
        if !budget.try_reserve_one(): break 'seed  // mutation 预算
        parent = population[gen % pop.len()]
        variants = mutation(parent, critiques, strengths)
        candidates = variants ∪ population  // 保留基线

        scored = []
        for c in candidates:
            eff_size = effective_candidate_size(c, current_skill)?  // dry-run
            (score, reason, judgements) = fitness_score(c, evalset, budget)?
            test_pass = if run_sandbox: sandbox_test(c).fitness() else None
            scored.push(ScoredVariant { c, score, eff_size, test_pass })

        if crossover && scored.len() >= 2 && budget.try_reserve_one():
            child = crossover(pareto_front top-2)
            ... 同上评分 ...

        population = select_population(scored, population_size)
        critiques = aggregate_critiques(judgements, 8)
        strengths = collect_satisfied(judgements, 8)

    for v in select_front_capped(population, 2):
        holdout_score = holdout_fitness_score(v, holdout_refs, budget)
        if !check_candidate(v, gates): gated_out++; continue
        if score < min_judge_score: judged_out++; continue
        final_props.push(v)

save_proposals(final_props)
record_run_meta("search", SearchRunMeta { ... })
```

---

## 6. 三维 Pareto 选择

维度：**judge score↑ × test_pass↑ × effective_size↓**

### 支配规则

A 支配 B iff：所有维度 A ≥ B，且至少一维 A > B。

`test_pass` 的特殊处理：

| A | B | test_ge | test_strict |
|---|---|---------|-------------|
| `Some(a)` | `Some(b)` | `a >= b` | `a > b` |
| `None` | `None` | `true` | `false` （退化二维） |
| 混合 | 混合 | `false` | `false` （不可比较） |

混合 case 设为不可比较，避免未测试候选支配已测试候选。

### 排序

`sort_scored()` 统一排序用于 `select_front_capped` 和 `select_population`：

```
score↓ → test_pass↓ (None 排末) → size↑
```

---

## 7. Curator 工作流

```
list_enabled_for_prompt()
  → 每技能计算 health_score + reasons（闲置/体积/进化历史/评测Fail）
  → find_overlap_clusters(skills, 0.5)  // Jaccard + Union-Find
  → 生成 suggestions: Disable / Merge / Rewrite
  → 去重
  → [可选] LLM 诊断增强 reason（judge 路由，max_llm_calls 次）
  → 落盘 curator-last.json
  → [可选] enqueue_curator_suggestions（入待审，上限 max_enqueue）
```

### Merge 决策

簇内 keep 选择优先级：非 stale > stale → health_score 更高 → 字母序。
Absorb 列表中的技能在批准后被 `set_enabled(false)`，不删文件。

### 安全边界

- 默认 `curator.enabled = false`
- 到期只刷新启发式报告，不自动入队
- `enqueue_suggestions` 需显式调用（UI 按钮或命令参数）
- 单次入队上限 `max_enqueue`（默认 5）
- 绝不在 Curator 路径调用高成本 GEPA search

---

## 8. 代码地图

| 文件 | 职责 |
|------|------|
| `crates/agent-evolution/src/search.rs` | `SearchBudget`、`effective_candidate_size`、`ScoredVariant`（三维）、`pareto_front`、`select_population`、`candidate_fingerprint`、mutation/crossover 提示词 |
| `crates/agent-evolution/src/evalset.rs` | `EvalSplit`、`split_eval_examples`、`weighted_eval_score`、`EvalJudgement`、`aggregate_critiques`、grounded eval 提示词 |
| `crates/agent-evolution/src/gates.rs` | `check_candidate`（含 dry-run）、`TestOutcome`、`run_skill_tests_in_dir`、`sandbox_test_candidate` |
| `crates/agent-evolution/src/curator.rs` | `CurateReport`、`find_overlap_clusters`、`merge_suggestions_for_clusters`、LLM 诊断提示词/解析/回填、`enqueue_curator_suggestions` |
| `crates/agent-evolution/src/history.rs` | `SearchRunMeta`、`record_run_meta` |
| `crates/agent-evolution/src/candidate.rs` | `CandidateKind { NewSkill, Patch, Disable, Merge }` |
| `crates/agent-evolution/src/reflect.rs` | reflection 提示词、`parse_candidates`（过滤 Disable/Merge 阻止泄入搜索） |
| `crates/agent-evolution/src/judge.rs` | 泛化 judge 提示词 |
| `crates/agent-evolution/src/proposal.rs` | 提案落盘/批准/回滚 |
| `crates/agent-memory/src/config.rs` | `EvolutionSearch`（含 max_eval_examples / max_llm_calls / population_size）、`EvolutionCurator`（含 llm_diagnose / max_llm_calls） |
| `apps/desktop/src-tauri/src/commands/evolution_run.rs` | Tauri 命令：search 主循环（注入 LLM 调用）、fitness_score、holdout、sandbox、curator LLM 诊断 |

---

## 9. 配置速查

```yaml
evolution:
  search:
    generations: 2          # 代数
    variants: 3             # 每代变体目标
    crossover: true         # Pareto top-2 交叉
    population_size: 3      # 种群保留上限
    max_eval_examples: 5    # 每候选评测例上限
    max_llm_calls: 40       # LLM 硬预算
  gates:
    run_tests: true         # 沙箱测试（搜索期 + 批准后）
    min_judge_score: 0.6    # 搜索出口 + 单轮 reflect 共用
  curator:
    enabled: false
    interval_days: 7
    max_enqueue: 5
    llm_diagnose: false
    max_llm_calls: 3
```

完整配置见 [`evolution.md`](./evolution.md) § 配置。

---

## 10. 验收标准

Phase 1–4 完成后应满足：

1. `max_llm_calls` 是严格上限，不被单候选多例评分突破
2. 无效 patch 在消耗 judge 调用前被拒绝
3. patch 与 new_skill 使用可比较的 post-image 体积
4. `population_size=1` 与旧行为兼容，默认小种群保留至少 2 条不同谱系
5. critique 明确指出 unmet expectations，而非只有总分
6. 样本足够时最终候选必须经过 holdout
7. history 可解释成本、路由、数据规模、终止原因与筛选结果
8. 全流程仍只写待审 proposal，不自动修改技能
9. API Key、会话全文和完整评测任务不写入 history
10. 混合 `None`/`Some` test_pass 不导致误支配

Phase 5 完成后额外满足：

11. 健康分仅基于本地启发式信号，缺信号时为 `None`
12. 重叠簇与 Disable/Merge 建议可生成，但默认不自动改技能、不自动入队
13. LLM 诊断失败静默回退启发式 reason
14. 显式入队时提案走现有人审，history 含 `mode=curator`

通过命令：

```bash
cargo test -p evolution   # 92 tests
cargo test -p memory      # 59 tests
cargo check -p astro-agent
```

---

## 11. 已知边界与未来方向

| 项 | 说明 |
|----|------|
| 嵌入向量重叠检测 | 当前 Jaccard token 级；正文相似需 simhash 或 embedding（未实现） |
| 异步沙箱 | `sandbox_test_candidate` 是同步阻塞（tempdir + 子进程），搜索循环是 async 但会在此处阻塞线程 |
| 交叉选择策略 | 当前取 Pareto front top-2；未来可改为"内容距离最远的两个高分个体" |
| 自动 evalset 扩充 | 用户拒绝提案时可自动追加 Fail 例（未实现） |
| Curator 自动调度 | `maybe_run_skill_curator` 已实现到期检测，但默认只刷新报告不入队 |
| ε-greedy 探索 | 纯贪心选种群；可加小概率随机选非最优，防早熟收敛 |
| DSPy 集成 | 独立 Python 包，可并行于 Rust 搜索；两者共享 evalset 但不共享种群 |
