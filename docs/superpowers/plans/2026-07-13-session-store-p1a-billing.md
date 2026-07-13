# Session Store P1a Billing Implementation Plan

> **已废弃：** 请勿按本计划实现。权威设计见 [`docs/superpowers/specs/2026-07-13-route-aware-usage-pricing-design.md`](../specs/2026-07-13-route-aware-usage-pricing-design.md)；待该 spec 审阅通过后另写 implementation plan。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 LLM token/估算费用累加到 `sessions` billing 列；停写 `usage.db` 的 `kind=llm`；清空历史 `usage_events`；Insights LLM 视图改读 sessions 聚合。

**Architecture:** `SessionStore::update_session_billing` 做会话累计；Agent 流式收尾改调该 API；`UsageDb` 增加一次性清空门控；新建 `query_usage_insights_merged` 门面合并 usage（非 llm）与 sessions（LLM），Tauri `get_usage_insights` 改走门面。无第三套计费表。

**Tech Stack:** Rust（rusqlite、chrono）、既有 `estimate_llm_cost`、Tauri command、可选前端 i18n

**Spec:** `docs/superpowers/specs/2026-07-13-session-store-p1a-billing-design.md`（已取代）

**执行注意:** `main` 上常有 HITL/delegate 未提交 WIP。**必须在干净 worktree**（从已提交 `main` 检出）实现本计划，勿在脏工作区直接改。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Modify: `memory/src/session/store/mod.rs` | 定义 `BillingDelta`（或同文件旁路类型） |
| Modify: `memory/src/session/store/sessions.rs` | 实现 `update_session_billing` |
| Modify: `memory/tests/session_store_test.rs` | billing 累加单测 |
| Modify: `memory/src/usage/db.rs` | 排除 llm 的 calls SQL；`usage_meta` + `ensure_p1a_cleared` |
| Create: `memory/src/usage/insights.rs` | `query_usage_insights_merged` |
| Modify: `memory/src/usage/mod.rs` | `pub mod insights` |
| Modify: `memory/src/lib.rs` | 导出门面 |
| Create/Modify: `memory/tests/usage_insights_merge_test.rs`（或扩展 `usage_db_test.rs`） | 合并与清空测试 |
| Modify: `agent/src/streaming.rs` | `record_llm_usage` → sessions 回填 |
| Modify: `frontend/src-tauri/src/config_commands.rs` | `get_usage_insights` 走合并门面 |
| Modify: `frontend/src/components/InsightsPanel.tsx` + i18n（可选） | 空态文案 |
| Modify: `docs/superpowers/specs/2026-07-13-session-store-p1a-billing-design.md` | 状态 → 已实现 |

---

### Task 1: `BillingDelta` + `update_session_billing`

**Files:**
- Modify: `memory/src/session/store/mod.rs`
- Modify: `memory/src/session/store/sessions.rs`
- Modify: `memory/tests/session_store_test.rs`

- [ ] **Step 1: 写失败测试**

在 `memory/tests/session_store_test.rs` 追加：

```rust
#[test]
fn update_session_billing_accumulates_tokens_and_cost() {
    let dir = TempDir::new().unwrap();
    let store = SessionStore::open(&dir.path().join("state.db")).unwrap();
    store.create_session("s1", "tauri", Some("gpt-test"), None, None).unwrap();

    store
        .update_session_billing(
            "s1",
            memory::session_store::BillingDelta {
                input_tokens: 10,
                output_tokens: 5,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                estimated_cost_usd: 0.01,
                api_call_count: 1,
                billing_provider: Some("openai".into()),
                billing_base_url: Some("https://api.example".into()),
                billing_mode: Some("chat".into()),
                cost_status: Some("estimated".into()),
                cost_source: Some("litellm_public".into()),
                pricing_version: Some("v1".into()),
                model: Some("gpt-test".into()),
            },
        )
        .unwrap();
    store
        .update_session_billing(
            "s1",
            memory::session_store::BillingDelta {
                input_tokens: 3,
                output_tokens: 2,
                cache_read_tokens: 0,
                cache_write_tokens: 0,
                reasoning_tokens: 0,
                estimated_cost_usd: 0.02,
                api_call_count: 1,
                billing_provider: Some("openai".into()),
                billing_base_url: Some("https://api.example".into()),
                billing_mode: Some("chat".into()),
                cost_status: Some("estimated".into()),
                cost_source: Some("litellm_public".into()),
                pricing_version: Some("v1".into()),
                model: Some("gpt-test".into()),
            },
        )
        .unwrap();

    let row = store.get_session_billing("s1").unwrap().unwrap();
    assert_eq!(row.input_tokens, 13);
    assert_eq!(row.output_tokens, 7);
    assert_eq!(row.api_call_count, 2);
    assert!((row.estimated_cost_usd - 0.03).abs() < 1e-9);
    assert!(row.actual_cost_usd.is_none());
    assert_eq!(row.cost_status.as_deref(), Some("estimated"));
    assert_eq!(row.billing_provider.as_deref(), Some("openai"));
}
```

若 `BillingDelta` / `get_session_billing` 路径与导出名不同，测试里改用 crate 实际公开路径（`memory::session_store` 或 `SessionStore` 同模块）。**优先把 `BillingDelta` 与 `SessionBillingRow` 放在 `memory/src/session/store/mod.rs` 并 `pub use`。**

- [ ] **Step 2: 跑测确认失败**

Run:

```bash
cargo test -p memory --test session_store_test update_session_billing -- --nocapture
```

Expected: FAIL（方法不存在）

- [ ] **Step 3: 实现类型与 API**

在 `memory/src/session/store/mod.rs` 增加：

```rust
#[derive(Debug, Clone, Default)]
pub struct BillingDelta {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub estimated_cost_usd: f64,
    pub api_call_count: i64,
    pub billing_provider: Option<String>,
    pub billing_base_url: Option<String>,
    pub billing_mode: Option<String>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub pricing_version: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct SessionBillingRow {
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_read_tokens: i64,
    pub cache_write_tokens: i64,
    pub reasoning_tokens: i64,
    pub api_call_count: i64,
    pub estimated_cost_usd: f64,
    pub actual_cost_usd: Option<f64>,
    pub cost_status: Option<String>,
    pub cost_source: Option<String>,
    pub pricing_version: Option<String>,
    pub billing_provider: Option<String>,
    pub billing_base_url: Option<String>,
    pub billing_mode: Option<String>,
    pub model: Option<String>,
}
```

在 `sessions.rs` 实现：

```rust
pub fn update_session_billing(&self, id: &str, d: BillingDelta) -> Result<()> {
    self.conn.execute(
        "UPDATE sessions SET
            input_tokens = COALESCE(input_tokens, 0) + ?1,
            output_tokens = COALESCE(output_tokens, 0) + ?2,
            cache_read_tokens = COALESCE(cache_read_tokens, 0) + ?3,
            cache_write_tokens = COALESCE(cache_write_tokens, 0) + ?4,
            reasoning_tokens = COALESCE(reasoning_tokens, 0) + ?5,
            estimated_cost_usd = COALESCE(estimated_cost_usd, 0) + ?6,
            api_call_count = COALESCE(api_call_count, 0) + ?7,
            billing_provider = COALESCE(?8, billing_provider),
            billing_base_url = COALESCE(?9, billing_base_url),
            billing_mode = COALESCE(?10, billing_mode),
            cost_status = COALESCE(?11, cost_status),
            cost_source = COALESCE(?12, cost_source),
            pricing_version = COALESCE(?13, pricing_version),
            model = COALESCE(?14, model)
         WHERE id = ?15",
        params![
            d.input_tokens,
            d.output_tokens,
            d.cache_read_tokens,
            d.cache_write_tokens,
            d.reasoning_tokens,
            d.estimated_cost_usd,
            d.api_call_count,
            d.billing_provider,
            d.billing_base_url,
            d.billing_mode,
            d.cost_status,
            d.cost_source,
            d.pricing_version,
            d.model,
            id,
        ],
    )?;
    Ok(())
}

pub fn get_session_billing(&self, id: &str) -> Result<Option<SessionBillingRow>> {
    self.conn
        .query_row(
            "SELECT COALESCE(input_tokens,0), COALESCE(output_tokens,0),
                    COALESCE(cache_read_tokens,0), COALESCE(cache_write_tokens,0),
                    COALESCE(reasoning_tokens,0), COALESCE(api_call_count,0),
                    COALESCE(estimated_cost_usd,0), actual_cost_usd,
                    cost_status, cost_source, pricing_version,
                    billing_provider, billing_base_url, billing_mode, model
             FROM sessions WHERE id = ?1",
            params![id],
            |row| {
                Ok(SessionBillingRow {
                    input_tokens: row.get(0)?,
                    output_tokens: row.get(1)?,
                    cache_read_tokens: row.get(2)?,
                    cache_write_tokens: row.get(3)?,
                    reasoning_tokens: row.get(4)?,
                    api_call_count: row.get(5)?,
                    estimated_cost_usd: row.get(6)?,
                    actual_cost_usd: row.get(7)?,
                    cost_status: row.get(8)?,
                    cost_source: row.get(9)?,
                    pricing_version: row.get(10)?,
                    billing_provider: row.get(11)?,
                    billing_base_url: row.get(12)?,
                    billing_mode: row.get(13)?,
                    model: row.get(14)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
}
```

从 `memory/src/lib.rs` / `session` 模块 re-export `BillingDelta`、`SessionBillingRow`（若测试用 `memory::…`）。

- [ ] **Step 4: 跑测确认通过**

Run:

```bash
cargo test -p memory --test session_store_test update_session_billing -- --nocapture
```

Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add memory/src/session/store/mod.rs memory/src/session/store/sessions.rs memory/tests/session_store_test.rs memory/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(session): add update_session_billing for LLM cost deltas

EOF
)"
```

---

### Task 2: Agent 流式改写 sessions（停写 llm 事件）

**Files:**
- Modify: `agent/src/streaming.rs`（约 `record_llm_usage`）

- [ ] **Step 1: 替换 `record_llm_usage` 实现**

将现有 `UsageDb::try_record(kind=llm)` 改为：

```rust
async fn record_llm_usage(session: &Arc<Mutex<AgentLoop>>, model: &str, usage: &Usage) {
    if usage.prompt_tokens == 0 && usage.completion_tokens == 0 && usage.total_tokens == 0 {
        return;
    }
    let agent = session.lock().await;
    let session_id = agent.session_id().to_string();
    let provider = agent.chat_provider().to_string();
    let base_url = agent.chat_base_url().to_string();
    let chat_model = agent.chat_model().to_string();
    // MemoryManager 上的 SessionStore：按现有 AgentLoop 访问路径取值
    // 典型：agent.memory.session_store 或等价 getter
    let cost = memory::estimate_llm_cost(model, usage.prompt_tokens, usage.completion_tokens);
    let delta = memory::BillingDelta {
        input_tokens: i64::from(usage.prompt_tokens),
        output_tokens: i64::from(usage.completion_tokens),
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        estimated_cost_usd: cost,
        api_call_count: 1,
        billing_provider: Some(provider).filter(|s| !s.is_empty()),
        billing_base_url: Some(base_url).filter(|s| !s.is_empty()),
        billing_mode: Some("chat".into()),
        cost_status: Some("estimated".into()),
        cost_source: Some("litellm_public".into()),
        pricing_version: None,
        model: Some(if model.is_empty() { chat_model } else { model.to_string() })
            .filter(|s| !s.is_empty()),
    };
    if let Err(e) = agent.memory.session_store.update_session_billing(&session_id, delta) {
        tracing::warn!("update_session_billing failed: {e:#}");
    }
}
```

**注意：**

- 先 `ensure_session`（若收尾路径未保证会话行存在）。可在 update 前：`let _ = agent.memory.session_store.ensure_session(&session_id, "tauri");`
- 字段访问名以 `AgentLoop` / `MemoryManager` 实际公开 API 为准（`session_store` 为 pub 字段）。
- 函数可改名为 `apply_session_llm_usage`，同步更新注释（去掉「写入 usage.db」）。
- **删除**对本函数内 `NewUsageEvent` / `kind: "llm"` 的引用。

- [ ] **Step 2: 编译检查**

Run:

```bash
cargo check -p agent 2>&1 | tail -30
```

Expected: 无 error

- [ ] **Step 3: Commit**

```bash
git add agent/src/streaming.rs
git commit -m "$(cat <<'EOF'
feat(agent): persist LLM usage on session billing columns

EOF
)"
```

---

### Task 3: 清空 `usage_events`（幂等）

**Files:**
- Modify: `memory/src/usage/db.rs`
- Modify: `memory/tests/usage_db_test.rs`（或新建测试）

- [ ] **Step 1: 扩展 DDL + 清空 API**

在 `DDL` 末尾追加：

```sql
CREATE TABLE IF NOT EXISTS usage_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
```

实现：

```rust
pub fn ensure_p1a_events_cleared(&self) -> anyhow::Result<()> {
    let already: Option<String> = self
        .conn
        .query_row(
            "SELECT value FROM usage_meta WHERE key = 'p1a_usage_events_cleared'",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if already.as_deref() == Some("1") {
        return Ok(());
    }
    self.conn.execute("DELETE FROM usage_events", [])?;
    self.conn.execute(
        "INSERT INTO usage_meta (key, value) VALUES ('p1a_usage_events_cleared', '1')
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [],
    )?;
    Ok(())
}
```

在 `UsageDb::open` / `open_default` 建表成功后调用 `ensure_p1a_events_cleared()`（失败 `warn` 或返回 Err——推荐 **返回 Err 使 open 失败可见**，测试可覆盖）。

同时把 `CALLS_KIND_SQL` 改为不含 llm：

```rust
const CALLS_KIND_SQL: &str =
    "CASE WHEN kind IN ('tool','mcp','cron') THEN 1 ELSE 0 END";
```

模块顶注释改为：不再记录 llm 事件（由 sessions 承担）。

- [ ] **Step 2: 测试清空幂等**

```rust
#[test]
fn p1a_clears_usage_events_once() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    {
        let db = UsageDb::new(&path).unwrap();
        db.insert(NewUsageEvent {
            ts: "2026-01-01T00:00:00Z".into(),
            kind: "llm".into(),
            name: "m".into(),
            agent_id: "a".into(),
            session_id: None,
            prompt_tokens: 1,
            completion_tokens: 1,
            total_tokens: 2,
            cost_usd: 0.1,
            meta_json: None,
        }).unwrap();
    }
    let db = UsageDb::new(&path).unwrap(); // open 应清空
    let n: i64 = db.conn_for_test().query_row(
        "SELECT COUNT(*) FROM usage_events", [], |r| r.get(0)
    ).unwrap();
    // 若无 conn_for_test：用 query_insights / 私有 helper 断言 calls==0
    assert_eq!(n, 0);
    // 再插入一条 tool，重新 open 不应被删
    db.insert(NewUsageEvent { kind: "tool".into(), /* …其余字段 */ … }).unwrap();
    let db2 = UsageDb::new(&path).unwrap();
    // count == 1
}
```

若 `conn` 私有，改为：`ensure_p1a` 已在 `new` 内调用；第一次 `new` 后 `insert` tool，第二次 `new` 后 insights KPI calls 仍为 1。**不要**为测试随意把 `conn` 公开；可用 `#[cfg(test)]` 方法 `event_count()`。

- [ ] **Step 3: 跑测**

```bash
cargo test -p memory --test usage_db_test p1a_clears -- --nocapture
cargo test -p memory --test usage_db_test -- --nocapture
```

Expected: PASS（若旧测依赖库内预置 llm 行，同步改掉）

- [ ] **Step 4: Commit**

```bash
git add memory/src/usage/db.rs memory/tests/usage_db_test.rs
git commit -m "$(cat <<'EOF'
feat(usage): clear legacy usage_events once for P1a billing

EOF
)"
```

---

### Task 4: Insights 合并门面

**Files:**
- Create: `memory/src/usage/insights.rs`
- Modify: `memory/src/usage/mod.rs`
- Modify: `memory/src/lib.rs`
- Modify: `frontend/src-tauri/src/config_commands.rs`
- Test: `memory/tests/usage_insights_merge_test.rs`

- [ ] **Step 1: 写失败/目标测试**

```rust
#[test]
fn merged_insights_uses_sessions_for_llm_cost() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    // sessions
    let store = SessionStore::open(&dir.path().join("sessions").join("state.db")).unwrap();
    // 注意：默认 SessionStore 路径是 memory_dir/sessions/state.db — 与 MemoryManager 一致
    std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
    let store = SessionStore::open(&dir.path().join("sessions/state.db")).unwrap();
    store.create_session("s1", "tauri", Some("model-a"), None, None).unwrap();
    store.update_session_billing("s1", BillingDelta {
        input_tokens: 100,
        output_tokens: 50,
        estimated_cost_usd: 1.25,
        api_call_count: 1,
        cost_status: Some("estimated".into()),
        cost_source: Some("litellm_public".into()),
        model: Some("model-a".into()),
        ..BillingDelta::default()
    }).unwrap();

    let usage = UsageDb::new(dir.path().join("usage.db")).unwrap();
    usage.insert(NewUsageEvent {
        ts: chrono::Utc::now().to_rfc3339(),
        kind: "tool".into(),
        name: "web_search".into(),
        agent_id: "workspace".into(),
        session_id: None,
        prompt_tokens: 0,
        completion_tokens: 0,
        total_tokens: 0,
        cost_usd: 0.0,
        meta_json: None,
    }).unwrap();

    let insights = memory::query_usage_insights_merged(UsageInsightsQuery {
        period: UsagePeriod::Year,
        as_of: None,
        agent_id: None,
    }).unwrap();

    assert!(insights.kpis.cost_usd >= 1.25);
    assert!(insights.kpis.tokens >= 150);
    assert!(insights.rankings.by_model.iter().any(|m| m.name == "model-a"));
    assert!(insights.rankings.by_kind.iter().any(|k| k.kind == "tool"));
    // 不应再出现 kind=llm 排行依赖
    std::env::remove_var("ASTRO_MEMORY_DIR");
}
```

（测试里路径/ENV 与 `default_memory_dir` / `SessionStore::open` 惯例对齐；以仓库现有 `MemoryManager::new` 打开双库更稳则可改用 Manager。）

- [ ] **Step 2: 实现 `query_usage_insights_merged`**

`memory/src/usage/insights.rs` 核心逻辑：

1. `UsageDb::open_default()?.query_insights(q)` 得到非 llm 为主的 usage 侧结果（Task 3 已改 CALLS_KIND；**额外**在 `query_rank_by_kind` / series / kpis 的 SQL 加 `AND kind != 'llm'`，双保险）。  
2. 打开 `SessionStore`：`SessionStore::open(&default_memory_dir().join("sessions/state.db"))`。  
3. 用 `period_window(q.period, q.as_of.as_deref())` 得 RFC3339 `[start,end)`，转为 epoch：

```rust
fn rfc3339_to_epoch(s: &str) -> anyhow::Result<f64> {
    Ok(chrono::DateTime::parse_from_rfc3339(s)?.timestamp() as f64)
}
```

4. 若 `q.agent_id` 为 `Some`：**跳过** sessions LLM 合并（spec），只返回 usage 侧。  
5. 否则查询 sessions：

```sql
SELECT COALESCE(model, ''),
       COALESCE(SUM(api_call_count), 0),
       COALESCE(SUM(input_tokens), 0) + COALESCE(SUM(output_tokens), 0),
       COALESCE(SUM(estimated_cost_usd), 0.0)
FROM sessions
WHERE started_at >= ?1 AND started_at < ?2
GROUP BY COALESCE(model, '')
```

以及 KPI 总和、按 `strftime` 桶（把 `started_at` 用 `datetime(started_at, 'unixepoch')`）聚合 series。

6. 合并：

- `kpis.tokens += sess_tokens`；`kpis.cost_usd += sess_cost`；`kpis.calls += sess_api_calls`  
- `rankings.by_model =` sessions 结果（`kind: "llm".into()`）  
- `series`：同 bucket 相加 calls/tokens/cost  

7. `lib.rs`：`pub use usage::insights::query_usage_insights_merged;`

- [ ] **Step 3: 改 Tauri**

`config_commands.rs`：

```rust
memory::query_usage_insights_merged(memory::UsageInsightsQuery {
    period,
    as_of: args.as_of,
    agent_id,
})
.map_err(|e| e.to_string())
```

- [ ] **Step 4: 跑测**

```bash
cargo test -p memory --test usage_insights_merge_test -- --nocapture
cargo check -p astro-ui 2>&1 | tail -20
# 若 crate 名不同：cargo check -p 对应 tauri package
```

Expected: PASS / 无 error

- [ ] **Step 5: Commit**

```bash
git add memory/src/usage/insights.rs memory/src/usage/mod.rs memory/src/lib.rs memory/src/usage/db.rs memory/tests/usage_insights_merge_test.rs frontend/src-tauri/src/config_commands.rs
git commit -m "$(cat <<'EOF'
feat(insights): merge session LLM billing into usage insights

EOF
)"
```

---

### Task 5: 前端空态（可选但推荐）+ 文档收尾

**Files:**
- Modify: `frontend/src/components/InsightsPanel.tsx`（空态旁提示）
- Modify: 对应 i18n messages（`frontend/src/i18n/…`）
- Modify: `docs/superpowers/specs/2026-07-13-session-store-p1a-billing-design.md`

- [ ] **Step 1: i18n**

增加类似：`insights.billingSinceP1a` → 「LLM 费用自会话计费起统计；历史用量事件已清空。」

在费用 KPI 或空态附近展示一行小字（不改 DTO）。

- [ ] **Step 2: Spec 状态**

将 P1a design 状态改为：`**状态:** 已批准 / 已实现`

- [ ] **Step 3: 回归**

```bash
cargo test -p memory --test session_store_test update_session_billing
cargo test -p memory --test usage_db_test
cargo test -p memory --test usage_insights_merge_test
cargo check -p agent
```

Expected: 全绿

- [ ] **Step 4: Commit**

```bash
git add frontend/src/components/InsightsPanel.tsx frontend/src/i18n docs/superpowers/specs/2026-07-13-session-store-p1a-billing-design.md
git commit -m "$(cat <<'EOF'
docs: mark session store P1a billing implemented

EOF
)"
```

---

## Spec coverage（self-review）

| Spec 要求 | Task |
|-----------|------|
| `update_session_billing` / BillingDelta | 1 |
| Agent 停写 llm、回填 sessions | 2 |
| 清空 usage_events + meta 幂等 | 3 |
| Insights 分流合并 / by_model 来自 sessions | 4 |
| agent_id 过滤时跳过 sessions LLM | 4 |
| 仅估算、actual 不动 | 1–2 |
| 空态文案 + spec Implemented | 5 |
| 无 hermes 命名 / 无第三套表 | 全任务 |
| Compaction 不做 | 未列入 |

## Placeholder / Consistency

- 无 TBD；`BillingDelta` 字段名前后一致。  
- `ensure_p1a_events_cleared` 键名与 spec `p1a_usage_events_cleared` 一致。  
- Tauri 仍返回 `UsageInsights`。

---

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-07-13-session-store-p1a-billing.md`.

**两种执行方式：**

1. **Subagent-Driven（推荐）** — 每任务新开子代理，任务间复审  
2. **Inline Execution** — 本会话按 executing-plans 连续执行并设检查点  

选哪一种？
