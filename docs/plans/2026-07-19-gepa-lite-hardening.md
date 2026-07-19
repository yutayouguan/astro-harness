# GEPA-lite 强化实施计划

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 将现有 GEPA-lite 从“单精英 + LLM 主观评分”的搜索骨架，逐步强化为具备严格预算、多谱系种群、结构化反馈、验证集、可选执行反馈，以及 Hermes 风格 Curator（技能库定期健康维护）的可控进化系统。

**Architecture:** 保留 Rust 侧 `reflection → mutation/crossover → fitness → Pareto → gates → proposal` 主链路，不引入新的 Python 运行时依赖。强化按「先正确、再多样、后客观、再护库」的顺序推进：先稳定质量契约和预算，再引入小种群与结构化 critique，然后 holdout 与沙箱执行反馈，最后用 Curator 防止技能库因持续进化而膨胀/腐化。Curator 在现有 `skills::curate_report`（闲置检测）之上扩展，产物仍只写待审提案，绝不自动删改技能。

**Tech Stack:** Rust、Tauri 2、Serde/YAML、现有 Provider 抽象、JSONL evalset/history、`skills` crate 使用统计、Cargo tests。

---

## 0. 当前基线与边界

当前工作树已经包含一部分第一阶段能力，实施前必须先确认并保留：

- `evolution/src/evalset.rs`
  - `Verdict::{Pass, Fail}`
  - `weighted_eval_score`：Fail 权重 2、Pass 权重 1
- `memory/src/config.rs`
  - `search.max_eval_examples`
  - `search.max_llm_calls`
- `frontend/src-tauri/src/evolution_run_commands.rs`
  - `fitness_score` 对 Fail 例优先截断
  - 搜索出口应用 `gates.min_judge_score`
  - 记录 `gated_out` / `judged_out`

这些内容目前可能仍是未提交 WIP。执行本计划时先运行：

```bash
git status --short
git diff -- evolution/src/evalset.rs memory/src/config.rs \
  frontend/src-tauri/src/evolution_run_commands.rs
```

不得覆盖或回退用户现有改动。

本计划不做：

- 不把默认 `generations` / `variants` 直接翻倍；
- 不重写完整 DSPy/GEPA；
- 不自动批准或直接写入技能；
- 不让自动触发路径运行高成本 search；
- 不把会话全文或 API Key 写入 history。

---

## Phase 1：稳定质量契约与硬预算

### Task 1：把 LLM 预算改成严格上限

现状在变异调用前检查预算，但一次 `fitness_score` 可能连续评多个例子，因此最终调用数仍可能超过 `max_llm_calls`。

**Files:**

- Modify: `evolution/src/search.rs`
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`
- Test: `evolution/src/search.rs`

**Step 1: 写预算单元测试**

在 `evolution/src/search.rs` 增加纯 Rust 预算类型测试：

```rust
#[test]
fn search_budget_never_exceeds_limit() {
    let mut budget = SearchBudget::new(3);
    assert!(budget.try_reserve(1));
    assert!(budget.try_reserve(2));
    assert!(!budget.try_reserve(1));
    assert_eq!(budget.used(), 3);
}

#[test]
fn zero_budget_means_unlimited() {
    let mut budget = SearchBudget::new(0);
    assert!(budget.try_reserve(10_000));
}
```

**Step 2: 确认测试失败**

Run:

```bash
cargo test -p evolution search_budget -- --nocapture
```

Expected: FAIL，`SearchBudget` 尚不存在。

**Step 3: 实现预算对象**

在 `evolution/src/search.rs` 增加：

```rust
#[derive(Debug, Clone)]
pub struct SearchBudget {
    limit: u32,
    used: u32,
}

impl SearchBudget {
    pub fn new(limit: u32) -> Self {
        Self { limit, used: 0 }
    }

    pub fn try_reserve(&mut self, calls: u32) -> bool {
        if self.limit > 0 && self.used.saturating_add(calls) > self.limit {
            return false;
        }
        self.used = self.used.saturating_add(calls);
        true
    }

    pub fn remaining(&self) -> Option<u32> {
        (self.limit > 0).then(|| self.limit.saturating_sub(self.used))
    }

    pub fn used(&self) -> u32 {
        self.used
    }
}
```

**Step 4: 让评分消费剩余预算**

调整 `fitness_score` 入参，使它接收“最多允许的评测调用数”，并只选取不超过剩余额度的 eval examples。泛化 judge 也必须先成功 reserve 1 次，不能 fail-open 偷跑。

建议语义：

- `max_llm_calls == 0`：不限；
- seed reflection、mutation、crossover、judge 全部计数；
- 预算不足以完成某候选评分时，停止生成新候选；
- 已完成评分的候选仍进入 Pareto；
- 未评分候选不得以 `0.5` 中性分进入 front。

**Step 5: 运行测试**

Run:

```bash
cargo test -p evolution search_budget -- --nocapture
cargo test -p memory evolution_search -- --nocapture
cargo check -p astro-agent
```

Expected: PASS；搜索预算不会超出配置上限。

**Step 6: Commit**

```bash
git add evolution/src/search.rs \
  frontend/src-tauri/src/evolution_run_commands.rs
git commit -m "fix(evolution): enforce strict GEPA-lite call budget"
```

---

### Task 2：统一 Pareto 的体积目标并提前验证 patch

当前 `SkillCandidate::payload_len` 对新技能取完整 `content`，对 patch 只取 `new_string`。两者不可比，短 patch 会获得系统性优势；而 old string 不唯一要到批准时才失败。

**Files:**

- Modify: `evolution/src/search.rs`
- Modify: `evolution/src/candidate.rs`
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`
- Test: `evolution/src/search.rs`
- Test: `evolution/src/proposal.rs`

**Step 1: 写失败测试**

覆盖：

1. 新技能的 Pareto size 等于完整候选长度；
2. patch 的 Pareto size 等于应用后的完整 `SKILL.md` 长度；
3. patch 未命中或多次命中时返回错误，不进入评分；
4. 合法 patch 应用后能正常构造 `ScoredVariant`。

**Step 2: 确认测试失败**

Run:

```bash
cargo test -p evolution effective_candidate_size -- --nocapture
```

Expected: FAIL，尚无“应用后体积”计算函数。

**Step 3: 增加有效载荷计算**

在 `evolution/src/search.rs` 增加纯函数：

```rust
pub fn effective_candidate_size(
    candidate: &SkillCandidate,
    current_skill: Option<&str>,
) -> anyhow::Result<usize>
```

规则：

- `NewSkill`：完整 `content.len()`；
- `Patch`：调用现有唯一替换逻辑生成 post-image，再取完整长度；
- 无当前技能、未命中或多次命中：返回错误；
- `ScoredVariant::new` 改为显式接收已计算的 size，避免内部继续调用 `payload_len()`。

**Step 4: 搜索评分前 dry-run**

在 `run_evolution_search` 中，候选进入 LLM judge 前：

1. 根据 `skill_id` 获取现有技能全文；
2. dry-run patch；
3. 失败则计入 `gated_out`，不消耗 judge 调用；
4. 使用 post-image 体积构造 Pareto 维度。

**Step 5: 运行测试**

Run:

```bash
cargo test -p evolution search -- --nocapture
cargo test -p evolution proposal -- --nocapture
cargo check -p astro-agent
```

Expected: PASS；无效 patch 不再进入模型评分。

**Step 6: Commit**

```bash
git add evolution/src/search.rs evolution/src/candidate.rs \
  frontend/src-tauri/src/evolution_run_commands.rs
git commit -m "fix(evolution): compare candidates by effective skill size"
```

---

## Phase 2：从单精英升级为小种群

### Task 3：增加可配置的小种群

当前每代只把 front 中最高分候选赋给 `current`，下一代只沿一条谱系继续，容易早熟收敛。

**Files:**

- Modify: `memory/src/config.rs`
- Modify: `memory/src/lib.rs`
- Modify: `frontend/src-tauri/src/compression_settings_commands.rs`
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`
- Modify: `evolution/src/search.rs`
- Test: `memory/src/config.rs`
- Test: `evolution/src/search.rs`

**Step 1: 写配置测试**

新增：

```rust
assert_eq!(cfg.search.population_size, 3);
```

并测试 YAML roundtrip。

建议默认值与约束：

- `population_size = 3`
- UI/命令层 clamp 到 `1..=8`
- `variants` 表示每个父代请求的候选数时成本会乘以种群大小；为控制成本，第一版定义为“每代总变体目标”，不要按父代全部展开。

**Step 2: 写种群选择测试**

在 `evolution/src/search.rs` 新增测试：

- 相同 `skill_id/kind/content` 的候选去重；
- 从 Pareto front 保留最多 K 个；
- 若 front 少于 K，可从被支配集合按 score 补齐，但不能重复；
- 同分时优先体积更小；
- K=1 保持现有行为。

**Step 3: 实现候选指纹和种群选择**

增加纯函数：

```rust
pub fn candidate_fingerprint(candidate: &SkillCandidate) -> String;

pub fn select_population(
    scored: Vec<ScoredVariant>,
    population_size: usize,
) -> Vec<ScoredVariant>;
```

指纹至少包含：

- `kind`
- `skill_id`
- `content` 或 `old_string/new_string`

**Step 4: 调整每代编排**

每代流程改为：

1. 将上一代 population 作为 parents；
2. 在预算范围内按 round-robin 选择父代做 mutation；
3. 合并 parents + offspring；
4. 去重；
5. 评分；
6. Pareto + `select_population(K)`；
7. crossover 从“质量高且内容不同”的两个父代中选择；
8. 下一代继续使用整个 population，而非单一 `current`。

**Step 5: 运行测试**

Run:

```bash
cargo test -p evolution search -- --nocapture
cargo test -p memory evolution_search -- --nocapture
cargo check -p astro-agent
```

Expected: PASS；`population_size=1` 时行为与旧实现兼容。

**Step 6: Commit**

```bash
git add evolution/src/search.rs memory/src/config.rs memory/src/lib.rs \
  frontend/src-tauri/src/compression_settings_commands.rs \
  frontend/src-tauri/src/evolution_run_commands.rs
git commit -m "feat(evolution): maintain diverse GEPA-lite population"
```

---

### Task 4：让 judge 输出结构化 critique

当前 grounded parser 只保留 score，多个 `judge_reason` 以自然语言列表回喂，信息密度低且难以稳定聚合。

**Files:**

- Modify: `evolution/src/evalset.rs`
- Modify: `evolution/src/judge.rs`
- Modify: `evolution/src/search.rs`
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`
- Test: `evolution/src/evalset.rs`
- Test: `evolution/src/judge.rs`

**Step 1: 定义结构化结果**

```rust
pub struct EvalJudgement {
    pub score: f32,
    pub satisfied: Vec<String>,
    pub unmet: Vec<String>,
    pub reason: String,
}
```

Prompt JSON 契约：

```json
{
  "score": 0.7,
  "satisfied": ["要点 A"],
  "unmet": ["缺少错误恢复步骤"],
  "reason": "..."
}
```

**Step 2: 写 parser 测试**

覆盖：

- 完整 JSON；
- 缺少数组时使用空数组；
- score clamp；
- markdown 围栏或前后噪声中的 JSON；
- 非法 JSON 返回错误，不生成中性 critique。

**Step 3: 聚合 critique**

增加纯函数：

```rust
pub fn aggregate_critiques(
    judgements: &[EvalJudgement],
    max_items: usize,
) -> Vec<String>
```

规则：

- 优先未满足项；
- 去重；
- 同一缺口出现次数多的排前；
- 最多 8 条；
- 不把完整 task、会话文本写入 history。

**Step 4: 更新 mutation prompt**

`build_mutation_prompt` 将 critique 分为：

- 必须修复的缺口；
- 必须保留的已有能力；
- 体积/静态门禁提醒。

**Step 5: 运行测试**

Run:

```bash
cargo test -p evolution evalset -- --nocapture
cargo test -p evolution judge -- --nocapture
cargo test -p evolution search -- --nocapture
```

Expected: PASS；下一代 prompt 包含可验证的 unmet expectation。

**Step 6: Commit**

```bash
git add evolution/src/evalset.rs evolution/src/judge.rs \
  evolution/src/search.rs frontend/src-tauri/src/evolution_run_commands.rs
git commit -m "feat(evolution): feed structured judge critique into mutations"
```

---

## Phase 3：降低过拟合并提高可复现性

### Task 5：引入稳定的 optimize/holdout 划分

同一 evalset 同时用于每代优化与最终报告，会让搜索过拟合 judge 和 expectations。

**Files:**

- Modify: `evolution/src/evalset.rs`
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`
- Modify: `evolution/src/history.rs`
- Test: `evolution/src/evalset.rs`

**Step 1: 写稳定划分测试**

要求：

- 相同 example id 每次落在相同分区；
- 至少 5 条匹配样本时启用 holdout；
- Fail/Pass 分层，避免 holdout 只有一种 verdict；
- 样本不足时全部作为 optimize，并在报告中标记“无 holdout”。

**Step 2: 实现确定性划分**

增加：

```rust
pub struct EvalSplit<'a> {
    pub optimize: Vec<&'a EvalExample>,
    pub holdout: Vec<&'a EvalExample>,
}

pub fn split_eval_examples(
    examples: &[&EvalExample],
    holdout_percent: u8,
) -> EvalSplit<'_>;
```

不要使用进程随机 seed。使用 example id 的稳定哈希，并按 verdict 分层。

**Step 3: 分离优化分与最终分**

- 每代 fitness 使用 optimize 集；
- 搜索结束后仅对 Pareto 前沿候选跑一次 holdout；
- 最终排序以 holdout 为主，optimize 为次；
- history 同时记录 `optimize_score`、`holdout_score`、样本数和模型路由；
- holdout 原始任务文本不进入 history。

**Step 4: 运行测试**

Run:

```bash
cargo test -p evolution evalset -- --nocapture
cargo test -p evolution history -- --nocapture
cargo check -p astro-agent
```

Expected: PASS；重复运行划分结果稳定。

**Step 5: Commit**

```bash
git add evolution/src/evalset.rs evolution/src/history.rs \
  frontend/src-tauri/src/evolution_run_commands.rs
git commit -m "feat(evolution): validate GEPA-lite candidates on holdout examples"
```

---

### Task 6：记录可复现实验摘要

**Files:**

- Modify: `evolution/src/history.rs`
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`
- Modify: `docs/evolution.md`
- Test: `evolution/src/history.rs`

**Step 1: 定义搜索摘要**

history 记录：

- 配置：generations、variants、population_size、crossover；
- 预算：limit、used；
- 数据：optimize/holdout 样本数；
- 路由：reflection/judge provider + model（不记录 key）；
- 结果：evaluated、pareto_kept、gated_out、judged_out、proposals；
- 终止原因：completed、budget_exhausted、provider_error、no_candidates。

**Step 2: 写序列化兼容测试**

旧 history 行缺少新字段时必须仍能读取。

**Step 3: 实现并更新文档**

同步修正：

- `evolution/src/search.rs` 顶部过时的“无评测集、不做交叉”注释；
- `docs/evolution.md` 的成本公式，加入 grounded eval 倍数；
- `docs/learning-loop.md` 中与交叉/evalset 冲突的描述。

**Step 4: 运行测试**

Run:

```bash
cargo test -p evolution history -- --nocapture
cargo test -p evolution --lib
cargo check -p astro-agent
```

Expected: PASS；旧 history 数据兼容。

**Step 5: Commit**

```bash
git add evolution/src/history.rs evolution/src/search.rs \
  frontend/src-tauri/src/evolution_run_commands.rs \
  docs/evolution.md docs/learning-loop.md
git commit -m "docs(evolution): record reproducible GEPA-lite search summaries"
```

---

## Phase 4：可选执行反馈

### Task 7：为可测试技能增加沙箱预跑

这是高风险、高收益阶段；必须在前 3 个阶段稳定后再做。

**Files:**

- Modify: `evolution/src/gates.rs`
- Modify: `evolution/src/search.rs`
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`
- Reuse: `evolution/src/proposal.rs` 中现有技能测试执行逻辑
- Test: `evolution/src/gates.rs`
- Test: `evolution/src/proposal.rs`

**Step 1: 先抽取现有测试 runner**

将批准阶段测试逻辑抽成可复用函数，入参必须是临时技能目录，不能直接修改真实技能。

**Step 2: 设计安全边界**

- 候选应用到 `tempdir`；
- 超时默认 60 秒；
- 清理敏感环境变量；
- 不继承模型 API Key；
- 禁止访问真实 skill 工作区；
- 输出截断并只记录摘要；
- 测试执行仍受用户配置开关控制。

**Step 3: 将执行结果作为第三适应度维度**

建议 Pareto 维度：

1. holdout/grounded quality ↑
2. test pass ratio ↑
3. effective skill size ↓

无测试脚本的技能不得被视为失败，只标记 `not_applicable`。

**Step 4: 运行测试**

Run:

```bash
cargo test -p evolution gates -- --nocapture
cargo test -p evolution proposal -- --nocapture
cargo test -p evolution --lib
```

Expected: PASS；测试只在临时目录执行，超时可控。

**Step 5: Commit**

```bash
git add evolution/src/gates.rs evolution/src/search.rs \
  evolution/src/proposal.rs frontend/src-tauri/src/evolution_run_commands.rs
git commit -m "feat(evolution): score testable skills with sandbox feedback"
```

---

## Phase 5：Curator — 技能库定期健康维护（Hermes 对齐）

> 参考 Hermes Agent v0.12 Curator：每周给 skill 打分、合并重叠、剪枝过时项，防止技能库变成坟场。  
> Astro 现状：`skills/src/usage.rs` 的 `curate_report` 仅基于 `last_loaded` 报告闲置建议；`skills` 工具 `action=curate` 已暴露给模型。本阶段把它升级为**可调度、可评分、可入待审**的策展环，仍不自动删除。

### Task 8：结构化策展报告 + 配置周期

把纯文本 `curate_report` 升级为结构化结果，并增加可配置的策展周期（默认每周，手动也可跑）。

**Files:**

- Modify: `skills/src/usage.rs`
- Modify: `skills/src/lib.rs`
- Modify: `memory/src/config.rs`（`learning.unused_skill_days` 旁增加 `evolution.curator` 或 `learning.curator`）
- Modify: `tools/src/builtins/memory/skills_tool.rs`
- Create: `evolution/src/curator.rs`（或先放 `skills`，若需写 proposal 再由 Tauri 调用）
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`（或新建 `curator_commands.rs`）
- Test: `skills/src/usage.rs`
- Test: `memory/src/config.rs`

**Step 1: 定义结构化报告**

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateSkillRow {
    pub skill_id: String,
    pub description: String,
    pub last_loaded: Option<String>,
    pub stale: bool,
    /// 0–1；无足够信号时为 None
    pub health_score: Option<f32>,
    pub health_reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurateReport {
    pub generated_at: String,
    pub unused_skill_days: u32,
    pub enabled_count: usize,
    pub stale: Vec<String>,
    pub rows: Vec<CurateSkillRow>,
    /// 重叠簇：同一簇内 skill_id 列表（相似度高）
    pub overlap_clusters: Vec<Vec<String>>,
    pub suggestions: Vec<CurateSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CurateSuggestion {
    Disable { skill_id: String, reason: String },
    Merge { keep: String, absorb: Vec<String>, reason: String },
    Rewrite { skill_id: String, reason: String },
}
```

保持 `curate_report` / `curate_report_at` 返回 Markdown 的兼容包装，内部调用结构化版本，避免破坏现有 `skills curate` 工具。

**Step 2: 写失败测试**

- 从未加载 / 超过 N 天 → `stale=true` 且进入 `suggestions::Disable`；
- 近期加载 → 不 stale；
- 空启用列表 → 空报告、非 panic；
- YAML 增加 `curator.enabled` / `curator.interval_days`（默认 7）roundtrip。

**Step 3: 实现健康分（纯启发式，无模型）**

第一版不用 LLM，避免成本；信号来自已有数据：

| 信号 | 来源 | 影响 |
|------|------|------|
| 闲置 | `skill-usage.json` last_loaded | 降分 / stale |
| 体积 | `SKILL.md` 字节 | 过大轻度降分 |
| 进化采纳 | `evolution/history.jsonl` 该 skill 近期 approved/rejected | 批准↑ / 拒绝↓ |
| 评测 | `evalset.jsonl` 匹配 Fail 例占比 | Fail 多则降分 |

`health_score` 缺信号时 `None`，不得伪造 0.5。

**Step 4: Tauri 命令与 UI 入口（最小）**

- `run_skill_curator` → 返回 `CurateReport` + 可选写入 `learning/evolution/curator-last.json`；
- 进化面板增加「策展」按钮或在历史旁展示上次报告摘要；
- **不**自动 disable/delete。

**Step 5: 运行测试**

```bash
cargo test -p skills usage -- --nocapture
cargo test -p memory learning -- --nocapture
cargo check -p astro-agent
```

Expected: PASS；旧 `skills curate` 仍返回可读 Markdown。

**Step 6: Commit**

```bash
git add skills/src/usage.rs skills/src/lib.rs memory/src/config.rs \
  tools/src/builtins/memory/skills_tool.rs \
  frontend/src-tauri/src/evolution_run_commands.rs \
  frontend/src/components/settings/
git commit -m "feat(skills): structured curator report with health signals"
```

---

### Task 9：重叠检测 + 合并/剪枝提案入队

在结构化报告之上，把高置信建议变成与 GEPA 同口径的**待审提案**（`SkillCandidate` 或独立 `CurateProposal`），人批准后才改库——对齐 Hermes「Curator 建议 → 人审」而非自动剪枝。

**Files:**

- Modify: `skills/src/usage.rs` 或 Create: `evolution/src/curator.rs`
- Modify: `evolution/src/proposal.rs`（若复用提案目录）
- Modify: `evolution/src/history.rs`（`mode=curator`）
- Modify: `frontend/src-tauri/src/evolution_run_commands.rs`
- Modify: `docs/evolution.md`
- Test: `evolution/src/curator.rs`（或 `skills` 侧）

**Step 1: 重叠检测（无模型优先）**

纯函数：

```rust
pub fn find_overlap_clusters(
    skills: &[(String, String)], // id, description(+可选正文摘要)
    threshold: f32,
) -> Vec<Vec<String>>;
```

建议实现（由简到繁，本 Task 只做到 1–2）：

1. 描述 + 标题 token Jaccard / 字符 n-gram；
2. 可选：正文前 N 字的 simhash；
3. **不做**嵌入模型调用（留给后续可选增强）。

簇大小 ≥2 才进入 `Merge` 建议；保留「描述更完整 / 最近加载更新 / health 更高」者为 `keep`。

**Step 2: 建议 → 待审提案**

映射规则：

| 建议 | 提案形态 | 批准效果 |
|------|----------|----------|
| `Disable` | 记录型提案或 UI 专项动作 | `set_enabled(false)`，不删文件 |
| `Merge` | `patch`/`new_skill`：把 absorb 要点并入 keep，并附带「禁用 absorb」清单 | 先写 keep，再禁用 absorb |
| `Rewrite` | 可选触发一次 targeted `run_evolution_search`（仅该 skill）或人工编辑链接 | 不自动开搜，默认只生成说明提案 |

所有提案必须：

- 带 `rationale` + 证据（闲置天数、重叠分数、health 原因）；
- 走现有审批 UI；
- `history` 记 `mode=curator` 的 run/outcome。

**Step 3: 调度与安全**

- `curator.enabled` 默认 **false**（与 `evolution.auto` 一致，防误烧/误改）；
- 开启后可在 App 启动或每日 tick 检查 `interval_days`，到期提示用户或仅产生报告（**默认只报告，不自动入队提案**；入队需显式 `enqueue_suggestions=true`）；
- 单次最多入队 N 条（建议 5），避免刷屏；
- 绝不在 Curator 路径调用高成本 GEPA search，除非用户点「为此技能进化」。

**Step 4: 写测试**

- Jaccard 重叠：同义描述聚成一簇，无关技能不聚；
- Merge 提案：`keep`/`absorb` 合法且不自吸收；
- Disable 提案：不生成 delete；
- `enqueue_suggestions=false` 时只写报告文件。

**Step 5: 运行测试**

```bash
cargo test -p skills --lib
cargo test -p evolution curator -- --nocapture
cargo test -p evolution history -- --nocapture
cargo check -p astro-agent
```

Expected: PASS；文档说明 Curator 与 search/reflect/dspy 并列。

**Step 6: Commit**

```bash
git add skills/src/usage.rs evolution/src/curator.rs evolution/src/lib.rs \
  evolution/src/history.rs evolution/src/proposal.rs \
  frontend/src-tauri/src/evolution_run_commands.rs docs/evolution.md
git commit -m "feat(evolution): curator overlap merge proposals with human review"
```

---

### Task 10（可选）：LLM 辅助策展诊断

仅在 Task 8–9 稳定后考虑。用 cheap judge 路由对「健康分低或重叠簇」生成一句可操作诊断（Hermes 的 Actionable Side Information 轻量版），写入 suggestion.reason；**禁止**直接改 SKILL.md。

预算：单次 Curator 跑最多 `curator.max_llm_calls`（默认 3）。失败则回退纯启发式 reason。

---

## 验收标准

完成 Phase 1–3 后应满足：

1. `max_llm_calls` 是严格上限，不会被单候选多例评分突破；
2. 无效 patch 在消耗 judge 调用前被拒绝；
3. patch 与 new_skill 使用可比较的 post-image 体积；
4. `population_size=1` 与旧行为兼容，默认小种群保留至少 2 条不同谱系；
5. critique 明确指出 unmet expectations，而非只有总分；
6. 样本足够时最终候选必须经过 holdout；
7. history 可解释成本、路由、数据规模、终止原因与筛选结果；
8. 全流程仍只写待审 proposal，不自动修改技能；
9. API Key、会话全文和完整评测任务不写入 history；
10. 以下命令通过：

```bash
cargo fmt --check
cargo test -p evolution --lib
cargo test -p memory --lib
cargo check -p astro-agent
```

完成 Phase 5（Task 8–9）后额外满足：

11. `curate_report` 仍可用，且存在等价结构化 `CurateReport`；
12. 健康分仅基于本地启发式信号，缺信号时为 `None`；
13. 重叠簇与 Disable/Merge 建议可生成，但默认不自动改技能、不自动入队；
14. 显式入队时提案走现有人审，history 含 `mode=curator`；
15. `cargo test -p skills --lib` 通过。

## 推荐实施顺序

严格按以下顺序：

1. Task 1 严格预算；
2. Task 2 有效体积 + patch dry-run；
3. Task 3 小种群；
4. Task 4 结构化 critique；
5. Task 5 holdout；
6. Task 6 可复现 history；
7. Task 7 执行反馈（可选）；
8. Task 8 结构化 Curator 报告 + 周期配置；
9. Task 9 重叠检测与人审合并/剪枝提案；
10. Task 10 LLM 策展诊断（可选）。

不要在 Task 1–2 尚未稳定时扩大搜索规模，否则只会放大成本和评分噪声。  
不要在 Task 8 尚未提供结构化报告时做自动入队；Curator 的第一原则是**护库、可逆、人审**。
