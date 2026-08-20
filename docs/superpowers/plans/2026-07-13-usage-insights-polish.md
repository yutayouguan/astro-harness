# Usage Insights 近期补强 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 修复洞察三处缺口——流错误路径记 LLM usage、Cron 记真实 token/费用、趋势图可切换 calls/tokens/cost。

**Architecture:** 抽出 `record_llm_usage` 辅助函数供 `finish_usage_and_done` / `finish_error` / cron 共用；cron 流循环累加 `ChatChunk.usage`；前端 InsightsPanel 增加 metric tabs，柱高按所选字段缩放。

**Tech Stack:** Rust (agent/memory/providers)、React InsightsPanel、现有 i18n

**Spec:** `docs/superpowers/specs/2026-07-13-usage-insights-polish-design.md`

> **Plan status:** Completed on `main`（2026-07-13）。实现已落地；勿再按本计划重复开工。

---

## File map

| 文件 | 职责 |
|---|---|
| `crates/agent-core/src/streaming.rs` | 抽 `record_llm_usage`；`finish_error` 有 usage 时落库 |
| `crates/agent-core/src/cron_exec.rs` | 累加 chunk.usage；成功/失败写 llm 事件 |
| `agent/tests/` 或 `cron_exec` 可测 helper | 用 `ASTRO_MEMORY_DIR` 断言落库 |
| `apps/desktop/src/components/InsightsPanel.tsx` | metric 切换 + 柱图 |
| `apps/desktop/src/styles/insights.css` | metric tabs 样式 |
| `apps/desktop/src/i18n/messages.ts` | 中英键 |

---

### Task 1: 抽 `record_llm_usage` + 错误路径落库

**Files:**
- Modify: `crates/agent-core/src/streaming.rs`
- Test: 在 `agent/tests/streaming_test.rs` 或新建小测：设 `ASTRO_MEMORY_DIR`，触发带 usage 的错误收尾，断言 `usage.db` 有 `kind=llm`

- [x] **Step 1: 抽出辅助函数**（从现有 `finish_usage_and_done` 体中）

```rust
/// 尽力写入一条 kind=llm 事件；失败忽略。
async fn record_llm_usage(
    session: &Arc<Mutex<AgentLoop>>,
    model: &str,
    usage: &Usage,
) {
    let agent = session.lock().await;
    let agent_id = agent.agent_id().to_string();
    let session_id = Some(agent.session_id().to_string());
    drop(agent);
    let cost = memory::estimate_llm_cost(
        model,
        usage.prompt_tokens,
        usage.completion_tokens,
    );
    memory::UsageDb::try_record(memory::NewUsageEvent {
        ts: chrono::Utc::now().to_rfc3339(),
        kind: "llm".into(),
        name: model.to_string(),
        agent_id,
        session_id,
        prompt_tokens: i64::from(usage.prompt_tokens),
        completion_tokens: i64::from(usage.completion_tokens),
        total_tokens: i64::from(usage.total_tokens),
        cost_usd: cost,
        meta_json: None,
    });
}
```

`finish_usage_and_done` 改为调用该函数后再 emit FinalUsage。

- [x] **Step 2: 扩展 `finish_error`**

```rust
async fn finish_error(
    session: &Arc<Mutex<AgentLoop>>,
    model: &str,
    tx: &mpsc::Sender<anyhow::Result<MultiTurnStreamItem>>,
    msg: impl Into<String>,
    usage: Option<Usage>,
) {
    if let Some(u) = usage {
        if u.prompt_tokens > 0 || u.completion_tokens > 0 || u.total_tokens > 0 {
            record_llm_usage(session, model, &u).await;
        }
    }
    let _ = emit(tx, MultiTurnStreamItem::Error(msg.into())).await;
    let _ = emit(tx, MultiTurnStreamItem::Done).await;
}
```

更新所有 `finish_error(...)` 调用点：传入 `session`、`&config.model`（或当前 model）、`saw_usage.then_some(total_usage.clone())`（按各点实际可用变量调整）。无 usage 处传 `None`。

- [x] **Step 3: 测试**

用 `ASTRO_MEMORY_DIR` + tempfile：跑一条会 `finish_error` 且 mock provider 先吐 usage 再失败的流（可参考现有 streaming_test fixtures）。断言 `UsageDb::open_default().query_insights(...)` 或直接 SQL count `kind='llm' >= 1`。

若难以构造，至少加 `#[cfg(test)]` 对 `record_llm_usage` 的同步包装测（打开临时 db path——若 `try_record` 只走 `open_default`，则必须设 env）。

Run: `cargo test -p agent --test streaming_test`

- [x] **Step 4: Commit**

```bash
git add agent/src/streaming.rs agent/tests/
git commit -m "$(cat <<'EOF'
fix(agent): record llm usage on stream error paths

EOF
)"
```

---

### Task 2: Cron 累加 usage 并写 llm 事件

**Files:**
- Modify: `crates/agent-core/src/cron_exec.rs`
- Test: `agent/tests/cron_exec_test.rs`（扩展）或 memory 侧不测；优先在 cron_exec 测 usage 累加纯函数

- [x] **Step 1: `run_provider_loop` 返回 `(String, Usage)`**

```rust
let mut total_usage = Usage::default();
let mut saw_usage = false;
while let Some(chunk_result) = stream.next().await {
    let chunk = chunk_result.map_err(|e| anyhow::anyhow!("{e}"))?;
    if let Some(token) = chunk.token {
        full_response.push_str(&token);
    }
    if let Some(u) = chunk.usage {
        total_usage.add_assign(u); // 或按 provider 约定覆盖；与 streaming.rs 一致优先
        saw_usage = true;
    }
}
Ok((full_response, if saw_usage { total_usage } else { Usage::default() }))
```

核对 `streaming.rs` 对 usage 是 `add_assign` 还是覆盖；cron **对齐同一策略**。

- [x] **Step 2: 在 `execute_job_with_roots` 成功/失败处写 llm**

成功分支：保留现有 `kind=cron`；若 `usage.total_tokens > 0 || prompt/completion > 0`：

```rust
let cost = memory::estimate_llm_cost(&model, usage.prompt_tokens, usage.completion_tokens);
memory::UsageDb::try_record(memory::NewUsageEvent {
    ts: chrono::Utc::now().to_rfc3339(),
    kind: "llm".into(),
    name: model.clone(),
    agent_id: job.agent_id.clone(),
    session_id: row.session_id.clone(),
    prompt_tokens: i64::from(usage.prompt_tokens),
    completion_tokens: i64::from(usage.completion_tokens),
    total_tokens: i64::from(usage.total_tokens),
    cost_usd: cost,
    meta_json: Some(serde_json::json!({ "source": "cron", "job_id": job.id }).to_string()),
});
```

失败分支：若有非空 usage，同样写 `kind=llm`（不写 cron）。

把 usage 从 `run_provider_loop` 传到 `execute_job_with_roots` 收尾处（调整返回类型 / 局部变量）。

- [x] **Step 3: 测试**

`ASTRO_MEMORY_DIR` + mock/minimal：若现有 cron_exec_test 难接真流，至少单测「给定 Usage 写入后 insights 含 llm」。可抽：

```rust
pub(crate) fn record_cron_llm_usage(agent_id: &str, session_id: Option<String>, model: &str, usage: &Usage, job_id: &str)
```

对抽函数做 tempfile 测。

Run: `cargo test -p agent --test cron_exec_test` 与相关 lib 测

- [x] **Step 4: Commit**

```bash
git add agent/src/cron_exec.rs agent/tests/
git commit -m "$(cat <<'EOF'
feat(agent): record llm token usage for cron job runs

EOF
)"
```

---

### Task 3: Insights 趋势图指标切换

**Files:**
- Modify: `apps/desktop/src/components/InsightsPanel.tsx`
- Modify: `apps/desktop/src/styles/insights.css`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [x] **Step 1: i18n**

```ts
"insights.metric.calls": "调用",
"insights.metric.tokens": "Tokens",
"insights.metric.cost": "费用（估）",
// en:
"insights.metric.calls": "Calls",
"insights.metric.tokens": "Tokens",
"insights.metric.cost": "Cost (est.)",
```

- [x] **Step 2: Panel state + chart**

```tsx
type Metric = "calls" | "tokens" | "cost";
const [metric, setMetric] = useState<Metric>("calls");

function seriesValue(s: UsageInsights["series"][0], m: Metric): number {
  if (m === "tokens") return s.tokens;
  if (m === "cost") return s.cost_usd;
  return s.calls;
}

const maxVal = Math.max(1, ...(data?.series.map((s) => seriesValue(s, metric)) ?? [1]));
```

在图表 heading 旁加与 period tabs 类似的 metric tabs；`height: (seriesValue(s,metric)/maxVal)*100%`；`title` 显示对应值（cost 用 `formatCost`）。

- [x] **Step 3: 样式**

复用 `.insights-period-tab` 或新增 `.insights-metric-tab`（同高、同圆角）。

- [x] **Step 4: 类型检查**

```bash
cd frontend && ./node_modules/.bin/tsc -b --pretty false
```

- [x] **Step 5: Commit**

```bash
git add apps/desktop/src/components/InsightsPanel.tsx apps/desktop/src/styles/insights.css apps/desktop/src/i18n/messages.ts
git commit -m "$(cat <<'EOF'
feat(frontend): toggle Insights chart by calls, tokens, or cost

EOF
)"
```

---

### Task 4: 回归验收

- [x] **Step 1: 命令**

```bash
cargo test -p memory --test usage_db_test
cargo test -p agent --test streaming_test
cargo check -p agent -p backend -p astro-agent
cd frontend && ./node_modules/.bin/tsc -b --pretty false
```

- [x] **Step 2: 对照 spec 验收清单勾选**

- [x] 错误路径有 llm 事件  
- [x] cron 成功有 cron + llm（有 usage 时）  
- [x] 趋势图三指标切换  

- [x] **Step 3: 无额外 commit（除非修 bug）**

---

## Self-review

| Spec 项 | Task |
|---|---|
| finish_error 记 usage | Task 1 |
| cron 累加 usage + llm 事件 | Task 2 |
| 趋势图指标切换 | Task 3 |
| 非目标未列入 | ✅ |
