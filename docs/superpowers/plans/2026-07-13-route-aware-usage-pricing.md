# Route-Aware Usage Pricing Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 将 LLM 用量归一为四桶 token，按路由官方快照 / models API 估算费用，双写 `usage_events` 与 `sessions` billing，并摧毁旧观测数据后从零累计。

**Architecture:** `providers` 扩展 `Usage` + `normalize`/`parse_*`；`memory` 重写 `usage/pricing.rs`（`BillingRoute` / `PricingEntry` / `CostResult` / `estimate_usage_cost`）并重建 `usage.db` schema；`SessionStore::update_session_billing` 累加会话列；`agent` 流式/cron 收尾双写。Insights SQL 排除 `cost_status='unknown'` 的金额。费用路径不再读 LiteLLM。

**Tech Stack:** Rust、rusqlite、serde_json、chrono、reqwest（memory 拉 OpenRouter/兼容 `/models` 缓存）、既有 Tauri Insights、tempfile 单测

**Spec:** `docs/superpowers/specs/2026-07-13-route-aware-usage-pricing-design.md`

**状态:** Task 1–8 已全部完成并合入 `main`（见 `ec64d76` 等）。

**执行注意:** 在干净 worktree 实现；勿把上游品牌字样写进代码/路径/文案。废弃计划 `docs/superpowers/plans/2026-07-13-session-store-p1a-billing.md` 勿执行。

---

## File Structure

| File | Responsibility |
|------|----------------|
| Modify: `providers/src/api/streaming.rs` | 四桶 `Usage`、`add_assign`、`prompt_tokens`/`completion_tokens` 访问器 |
| Modify: `providers/src/protocol/http_stream.rs` | `parse_openai_usage`（cache 拆分）；必要时 Anthropic usage 解析 |
| Create/Modify: `providers` 单测（同文件或 `providers/tests`） | normalize / parse |
| Rewrite: `memory/src/usage/pricing.rs` | 路由、快照、models 缓存、`estimate_usage_cost`；删除 LiteLLM 费用读取 |
| Modify: `memory/Cargo.toml` | 增加 `reqwest`（blocking 或 default）供拉 `/models` |
| Modify: `memory/src/usage/db.rs` | 新 DDL + `user_version` 摧毁重建；扩展 `NewUsageEvent`；insights 聚合过滤 unknown |
| Modify: `memory/src/usage/stats.rs` | `NewUsageEvent` 字段适配（非 llm 默认空） |
| Modify: `memory/src/session/store/mod.rs` | 导出 `BillingDelta` |
| Modify: `memory/src/session/store/sessions.rs` | `update_session_billing` |
| Modify: `memory/src/session/store/schema.rs` | `SCHEMA_VERSION=12` + billing 清零迁移 |
| Modify: `memory/src/lib.rs` | 导出新 API；去掉/替换 `estimate_llm_cost` |
| Modify: `memory/tests/usage_db_test.rs` | 新 schema、pricing、insights unknown |
| Modify: `memory/tests/session_store_test.rs` | billing 累加 / 迁移清零 |
| Modify: `memory/tests/collab_insights_test.rs` | `NewUsageEvent` 字段 |
| Modify: `agent/src/streaming.rs` | 双写 + 传入 provider/base_url |
| Modify: `agent/src/cron_exec.rs` | 同上 |
| Modify: `agent/src/orchestration.rs` | 若构造 `NewUsageEvent` 则适配默认字段 |
| Modify: `frontend/src/components/InsightsPanel.tsx` | 未计价改看 `unpriced_llm_events`（若 DTO 增加） |
| Modify: `docs/superpowers/specs/2026-07-13-route-aware-usage-pricing-design.md` | 实现后状态 → 已实现 |

---

### Task 1: 扩展 `providers::Usage` + OpenAI usage 解析

**Files:**
- Modify: `providers/src/api/streaming.rs`
- Modify: `providers/src/protocol/http_stream.rs`
- Test: `providers/src/protocol/http_stream.rs` 内既有 `#[cfg(test)]` 或追加

- [x] **Step 1: 写失败测试（cache 拆分）**

在 `http_stream.rs` 测试模块追加：

```rust
#[test]
fn parse_openai_usage_splits_cached_prompt_tokens() {
    let v = serde_json::json!({
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 20,
            "total_tokens": 120,
            "prompt_tokens_details": { "cached_tokens": 40, "cache_write_tokens": 10 }
        }
    });
    let u = parse_openai_usage(&v).expect("usage");
    assert_eq!(u.input_tokens, 50); // 100 - 40 - 10
    assert_eq!(u.output_tokens, 20);
    assert_eq!(u.cache_read_tokens, 40);
    assert_eq!(u.cache_write_tokens, 10);
    assert_eq!(u.prompt_tokens(), 100);
    assert_eq!(u.completion_tokens(), 20);
}
```

- [x] **Step 2: Run test to verify it fails**

Run: `cargo test -p providers parse_openai_usage_splits_cached_prompt_tokens -- --nocapture`  
Expected: FAIL（字段不存在或断言失败）

- [x] **Step 3: 实现 `Usage` 四桶**

将 `providers/src/api/streaming.rs` 中 `Usage` 改为：

```rust
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
    pub reasoning_tokens: u32,
    pub request_count: u32,
}

impl Usage {
    pub fn prompt_tokens(&self) -> u32 {
        self.input_tokens
            .saturating_add(self.cache_read_tokens)
            .saturating_add(self.cache_write_tokens)
    }
    pub fn completion_tokens(&self) -> u32 {
        self.output_tokens
    }
    pub fn total_tokens(&self) -> u32 {
        self.prompt_tokens().saturating_add(self.completion_tokens())
    }
    pub fn add_assign(&mut self, other: Usage) {
        self.input_tokens = self.input_tokens.saturating_add(other.input_tokens);
        self.output_tokens = self.output_tokens.saturating_add(other.output_tokens);
        self.cache_read_tokens = self.cache_read_tokens.saturating_add(other.cache_read_tokens);
        self.cache_write_tokens = self.cache_write_tokens.saturating_add(other.cache_write_tokens);
        self.reasoning_tokens = self.reasoning_tokens.saturating_add(other.reasoning_tokens);
        let n = if other.request_count == 0 { 1 } else { other.request_count };
        self.request_count = self.request_count.saturating_add(n);
    }
    pub fn from_parts(input: u32, output: u32) -> Self {
        Self {
            input_tokens: input,
            output_tokens: output,
            request_count: 1,
            ..Default::default()
        }
    }
    pub fn is_empty(&self) -> bool {
        self.input_tokens == 0
            && self.output_tokens == 0
            && self.cache_read_tokens == 0
            && self.cache_write_tokens == 0
            && self.reasoning_tokens == 0
    }
}
```

注意：全仓库将旧字段 `usage.prompt_tokens` 改为 `usage.prompt_tokens()` 或 `input_tokens`；`cargo build` 修编译错误。`request_count`：`Default` 为 0，`from_parts` 设 1；`add_assign` 对非空 other 累加 `other.request_count.max(1)`。

重写 `parse_openai_usage`：

```rust
pub fn parse_openai_usage(v: &Value) -> Option<Usage> {
    let u = v.get("usage")?;
    if u.is_null() {
        return None;
    }
    let prompt_total = u.get("prompt_tokens").or_else(|| u.get("input_tokens")).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let output = u.get("completion_tokens").or_else(|| u.get("output_tokens")).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    let details = u.get("prompt_tokens_details");
    let mut cache_read = details.and_then(|d| d.get("cached_tokens")).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    if cache_read == 0 {
        cache_read = u.get("cache_read_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    }
    let mut cache_write = details.and_then(|d| d.get("cache_write_tokens")).and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    if cache_write == 0 {
        cache_write = u.get("cache_creation_input_tokens").and_then(|x| x.as_u64()).unwrap_or(0) as u32;
    }
    let input = prompt_total.saturating_sub(cache_read).saturating_sub(cache_write);
    let reasoning = u
        .get("completion_tokens_details")
        .or_else(|| u.get("output_tokens_details"))
        .and_then(|d| d.get("reasoning_tokens"))
        .and_then(|x| x.as_u64())
        .unwrap_or(0) as u32;
    if input == 0 && output == 0 && cache_read == 0 && cache_write == 0 && reasoning == 0 {
        return None;
    }
    Some(Usage {
        input_tokens: input,
        output_tokens: output,
        cache_read_tokens: cache_read,
        cache_write_tokens: cache_write,
        reasoning_tokens: reasoning,
        request_count: 1,
    })
}
```

更新同文件旧测试构造体字段。

- [x] **Step 4: Run tests**

Run: `cargo test -p providers -- --nocapture`  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add providers/src/api/streaming.rs providers/src/protocol/http_stream.rs
# 以及为修编译而改动的其它 crate 文件
git commit -m "$(cat <<'EOF'
feat(providers): canonical four-bucket Usage and cache-aware parse

EOF
)"
```

---

### Task 2: 路由定价（官方快照，无网络）

**Files:**
- Rewrite: `memory/src/usage/pricing.rs`
- Modify: `memory/src/lib.rs`
- Modify: `memory/tests/usage_db_test.rs`（替换 `estimate_llm_cost` 测试）

- [x] **Step 1: 写失败测试**

在 `memory/tests/usage_db_test.rs` 将 LiteLLM 测试改为：

```rust
#[test]
fn estimate_usage_cost_official_snapshot_and_unknown() {
    use memory::{estimate_usage_cost, CostStatus, UsageTokens};
    let usage = UsageTokens {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        request_count: 1,
    };
    let r = estimate_usage_cost(
        "gpt-4o-mini",
        &usage,
        Some("openai"),
        None,
        None,
    );
    assert_eq!(r.status, CostStatus::Estimated);
    assert!(r.amount_usd.unwrap() > 0.0);
    let unk = estimate_usage_cost(
        "totally-unknown-model-xyz",
        &usage,
        Some("custom"),
        Some("http://localhost:9"),
        None,
    );
    assert_eq!(unk.status, CostStatus::Unknown);
    assert!(unk.amount_usd.is_none() || unk.amount_usd == Some(0.0));
}
```

（`UsageTokens` / `CostStatus` 为 memory 侧类型，避免 memory→providers 依赖。）

- [x] **Step 2: Run test — expect FAIL**

Run: `cargo test -p memory estimate_usage_cost_official_snapshot_and_unknown -- --nocapture`

- [x] **Step 3: 实现 pricing 模块**

`memory/src/usage/pricing.rs` 核心类型与 API：

```rust
#[derive(Debug, Clone, Copy, Default)]
pub struct UsageTokens {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub cache_read_tokens: u32,
    pub cache_write_tokens: u32,
    pub request_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostStatus { Estimated, Included, Unknown }

#[derive(Debug, Clone)]
pub struct CostResult {
    pub amount_usd: Option<f64>,
    pub status: CostStatus,
    pub source: String,          // e.g. "official_docs_snapshot" | "provider_models_api" | "none"
    pub pricing_version: Option<String>,
    pub label: String,
}

pub fn resolve_billing_route(model: &str, provider: Option<&str>, base_url: Option<&str>) -> BillingRoute { /* ... */ }

pub fn estimate_usage_cost(
    model: &str,
    usage: &UsageTokens,
    provider: Option<&str>,
    base_url: Option<&str>,
    api_key: Option<&str>,
) -> CostResult { /* 公式见 spec；快照表至少含 gpt-4o-mini / 一两个 claude 条目 */ }
```

内置 `_OFFICIAL_DOCS_PRICING`: 至少 `("openai","gpt-4o-mini")` 使用与公开价一致的 per-million 数（例：input 0.15、output 0.60）。  
`localhost` / 未知 → `Unknown`。  
**禁止** 读取 `litellm-model-meta.json`。

删除旧 `estimate_llm_cost`；`lib.rs`：

```rust
pub use usage_pricing::{estimate_usage_cost, CostResult, CostStatus, UsageTokens, /* BillingRoute if needed */};
```

全仓库替换调用点（先编译，Task 5/6 再接完整参数）。临时可保留：

```rust
#[deprecated]
pub fn estimate_llm_cost(model: &str, prompt: u32, completion: u32) -> f64 {
    let u = UsageTokens { input_tokens: prompt, output_tokens: completion, request_count: 1, ..Default::default() };
    estimate_usage_cost(model, &u, None, None, None).amount_usd.unwrap_or(0.0)
}
```

本 Task 结束前删除 deprecated，并修所有调用。

- [x] **Step 4: Run tests**

Run: `cargo test -p memory estimate_usage_cost_official_snapshot_and_unknown -- --nocapture`  
Expected: PASS

- [x] **Step 5: Commit**

```bash
git add memory/src/usage/pricing.rs memory/src/lib.rs memory/tests/usage_db_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): route-aware estimate_usage_cost with official snapshots

EOF
)"
```

---

### Task 3: OpenRouter / 兼容 `/models` 价目缓存

**Files:**
- Modify: `memory/Cargo.toml`（`reqwest` features `json,rustls-tls,blocking`）
- Modify: `memory/src/usage/pricing.rs`
- Test: `memory/tests/usage_db_test.rs`（文件系统缓存，不打真网）

- [x] **Step 1: 写失败测试（读缓存文件）**

```rust
#[test]
fn estimate_usage_cost_reads_openrouter_cache_file() {
    let dir = tempfile::tempdir().unwrap();
    std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
    let cache = dir.path().join("openrouter-model-pricing.json");
    std::fs::write(
        &cache,
        r#"{"fetched_at":"2099-01-01T00:00:00Z","models":{"test/or-model":{"prompt":0.000001,"completion":0.000002}}}"#,
    ).unwrap();
    let usage = memory::UsageTokens {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        request_count: 1,
        ..Default::default()
    };
    let r = memory::estimate_usage_cost(
        "test/or-model",
        &usage,
        Some("openrouter"),
        Some("https://openrouter.ai/api/v1"),
        None,
    );
    assert_eq!(r.status, memory::CostStatus::Estimated);
    assert!((r.amount_usd.unwrap() - 3.0).abs() < 1e-6); // 1.0 + 2.0 per 1M
    std::env::remove_var("ASTRO_MEMORY_DIR");
}
```

（`prompt`/`completion` 按 **每 token USD** 存，估价时 × tokens；与 OpenRouter models API 一致。）

- [x] **Step 2: Run — expect FAIL**

- [x] **Step 3: 实现缓存读写**

- 路径：`default_memory_dir().join("openrouter-model-pricing.json")`  
- `get_pricing_entry`：openrouter 路由先读缓存；过期（>24h）且提供 `api_key` 时 `blocking` GET `{base}/models`，解析 `data[].id` + `pricing`，写回缓存  
- 无缓存且无网 → `Unknown`  
- `source = "provider_models_api"`

- [x] **Step 4: Run test — PASS**

- [x] **Step 5: Commit**

```bash
git add memory/Cargo.toml memory/src/usage/pricing.rs memory/tests/usage_db_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): load OpenRouter-compatible model pricing cache

EOF
)"
```

---

### Task 4: 重建 `usage.db` schema + insights 过滤 unknown

**Files:**
- Modify: `memory/src/usage/db.rs`
- Modify: `memory/src/usage/stats.rs`
- Modify: `memory/tests/usage_db_test.rs`
- Modify: `memory/tests/collab_insights_test.rs`

**常量：** `USAGE_SCHEMA_VERSION: i32 = 2`（或 `PRAGMA user_version=2`）

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn usage_db_rebuilds_incompatible_schema_and_ignores_unknown_cost() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("usage.db");
    // 写入旧表形状
    {
        let conn = rusqlite::Connection::open(&path).unwrap();
        conn.execute_batch(
            "CREATE TABLE usage_events (
                id TEXT PRIMARY KEY, ts TEXT, kind TEXT, name TEXT, agent_id TEXT,
                session_id TEXT, prompt_tokens INTEGER, completion_tokens INTEGER,
                total_tokens INTEGER, cost_usd REAL, meta_json TEXT
             );",
        ).unwrap();
        conn.execute(
            "INSERT INTO usage_events VALUES ('1','2026-07-01T00:00:00Z','llm','m','a',NULL,1,1,2,9.9,NULL)",
            [],
        ).unwrap();
    }
    let db = memory::UsageDb::new(path.clone()).unwrap();
    // 旧行应消失
    let q = memory::UsageInsightsQuery {
        period: memory::UsagePeriod::Year,
        as_of: Some("2026-07-13T00:00:00Z".into()),
        agent_id: None,
    };
    let insights = db.query_insights(q.clone()).unwrap();
    assert_eq!(insights.kpis.calls, 0);

    db.insert(memory::NewUsageEvent {
        ts: "2026-07-10T12:00:00Z".into(),
        kind: "llm".into(),
        name: "m".into(),
        agent_id: "a".into(),
        session_id: None,
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 15,
        cost_usd: 0.0,
        cost_status: Some("unknown".into()),
        cost_source: Some("none".into()),
        pricing_version: None,
        billing_provider: None,
        billing_base_url: None,
        billing_mode: None,
        meta_json: None,
    }).unwrap();
    db.insert(memory::NewUsageEvent {
        ts: "2026-07-10T13:00:00Z".into(),
        kind: "llm".into(),
        name: "m2".into(),
        agent_id: "a".into(),
        session_id: None,
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        total_tokens: 15,
        cost_usd: 1.25,
        cost_status: Some("estimated".into()),
        cost_source: Some("official_docs_snapshot".into()),
        pricing_version: Some("test".into()),
        billing_provider: Some("openai".into()),
        billing_base_url: None,
        billing_mode: None,
        meta_json: None,
    }).unwrap();
    let insights = db.query_insights(q).unwrap();
    assert!((insights.kpis.cost_usd - 1.25).abs() < 1e-9);
    assert_eq!(insights.kpis.tokens, 30);
    assert_eq!(insights.unpriced_llm_events, 1);
}
```

- [x] **Step 2: Run — expect FAIL**

- [x] **Step 3: 实现新 DDL 与打开逻辑**

`UsageDb::new`：

1. 打开连接；读 `PRAGMA user_version`  
2. 若 `user_version != USAGE_SCHEMA_VERSION`：关闭 → **删除** path（及 `-wal`/`-shm`）→ 重建 → `PRAGMA user_version = USAGE_SCHEMA_VERSION`；`tracing::warn!("usage.db rebuilt, prior events discarded")`  
3. 若表缺新列亦可同样摧毁（简单起见只靠 version）

新 DDL 列与 `NewUsageEvent` 对齐 spec。  
KPI/series/rankings 的 `SUM(cost_usd)` 改为：

```sql
COALESCE(SUM(CASE WHEN cost_status IS NULL OR cost_status IN ('estimated','included') THEN cost_usd ELSE 0 END), 0)
```

（tool 行 `cost_status` NULL 且 cost 0 仍安全。）

`UsageInsights` 增加：

```rust
pub unpriced_llm_events: i64,
```

查询：`COUNT(*) WHERE kind='llm' AND cost_status='unknown'`。

更新所有构造 `NewUsageEvent` 的测试与 `stats.rs`。

- [x] **Step 4: Run `cargo test -p memory -- --nocapture` — PASS**

- [x] **Step 5: Commit**

```bash
git add memory/src/usage/db.rs memory/src/usage/stats.rs memory/tests/
git commit -m "$(cat <<'EOF'
feat(memory): rebuild usage.db schema with billing fields

EOF
)"
```

---

### Task 5: `update_session_billing` + schema v12 账单清零

**Files:**
- Modify: `memory/src/session/store/mod.rs`
- Modify: `memory/src/session/store/sessions.rs`
- Modify: `memory/src/session/store/schema.rs`
- Modify: `memory/tests/session_store_test.rs`

- [x] **Step 1: 写失败测试**

```rust
#[test]
fn update_session_billing_accumulates_and_unknown_skips_cost() {
    let dir = tempfile::tempdir().unwrap();
    let store = memory::SessionStore::open(&dir.path().join("state.db")).unwrap();
    store.create_session("s1", "test", Some("gpt"), None, None).unwrap();
    store.update_session_billing("s1", memory::BillingDelta {
        input_tokens: 10,
        output_tokens: 5,
        cache_read_tokens: 2,
        cache_write_tokens: 1,
        reasoning_tokens: 3,
        estimated_cost_usd: 0.01,
        api_call_count: 1,
        billing_provider: Some("openai".into()),
        billing_base_url: Some("https://api.openai.com/v1".into()),
        billing_mode: Some("official_docs_snapshot".into()),
        cost_status: Some("estimated".into()),
        cost_source: Some("official_docs_snapshot".into()),
        pricing_version: Some("v1".into()),
        model: Some("gpt".into()),
    }).unwrap();
    store.update_session_billing("s1", memory::BillingDelta {
        input_tokens: 1,
        output_tokens: 1,
        cache_read_tokens: 0,
        cache_write_tokens: 0,
        reasoning_tokens: 0,
        estimated_cost_usd: 0.0,
        api_call_count: 1,
        billing_provider: None,
        billing_base_url: None,
        billing_mode: None,
        cost_status: Some("unknown".into()),
        cost_source: Some("none".into()),
        pricing_version: None,
        model: None,
    }).unwrap();
    let row = store.debug_session_billing("s1").unwrap(); // 测试辅助：读回 billing 列
    assert_eq!(row.input_tokens, 11);
    assert!((row.estimated_cost_usd - 0.01).abs() < 1e-9);
    assert_eq!(row.cost_status.as_deref(), Some("unknown"));
    assert_eq!(row.api_call_count, 2);
}
```

若不愿加 `debug_session_billing`，用 `conn` 测试模块内 `SELECT` 或扩展 `StoredSession`（仅测试）。

另测：打开已有 v11 DB 升到 v12 后 billing 列为 0（在 test 里手工插入非零再 migrate）。

- [x] **Step 2: Run — FAIL**

- [x] **Step 3: 实现**

`BillingDelta` 放 `session/store/mod.rs` 并 `pub use`。

`update_session_billing` SQL：token/`api_call_count` 累加；`estimated_cost_usd` 仅当 `cost_status != "unknown"` 时累加 delta；`cost_status` 取最差（已有或新为 unknown → unknown）；覆盖 provider/base_url/source/version；`actual_cost_usd` 不动。

`SCHEMA_VERSION = 12`：在 `migrate` 中若 `< 12`：

```sql
UPDATE sessions SET
  input_tokens=0, output_tokens=0, cache_read_tokens=0, cache_write_tokens=0,
  reasoning_tokens=0, estimated_cost_usd=NULL, actual_cost_usd=NULL,
  cost_status=NULL, cost_source=NULL, pricing_version=NULL,
  billing_provider=NULL, billing_base_url=NULL, billing_mode=NULL,
  api_call_count=0;
```

并 `INSERT OR REPLACE INTO state_meta(key,value) VALUES ('billing_reset_v12','1');`  
更新 `opens_fresh_db_at_schema_v11` 测试为 v12。

启动时若存在 `usage-stats.json`：`UsageDb::open` 旁路或 `SessionStore::open` 同次调用 `std::fs::remove_file`（幂等）；可放在 `memory` 的 `reset_usage_observability_files()`，由 backend/tauri 启动或 `UsageDb::open_default` 调用一次（meta 门控）。

- [x] **Step 4: tests PASS**

- [x] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(memory): session billing accumulate and v12 billing reset

EOF
)"
```

---

### Task 6: Agent 流式 / cron 双写

**Files:**
- Modify: `agent/src/streaming.rs`
- Modify: `agent/src/cron_exec.rs`
- Modify: `agent/src/orchestration.rs`（若有 `NewUsageEvent`）
- Test: 优先单测 hook；若无现成 mock，加 `agent` 内对 `apply_llm_usage_record` 的纯函数测

- [x] **Step 1: 提取可测函数并写测**

在 `streaming.rs`（或 `agent/src/usage_record.rs`）：

```rust
pub(crate) fn build_llm_usage_event(
    agent_id: &str,
    session_id: &str,
    model: &str,
    usage: &providers::Usage,
    provider: &str,
    base_url: &str,
    api_key: &str,
) -> (memory::NewUsageEvent, memory::BillingDelta) {
    let tokens = memory::UsageTokens {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        cache_read_tokens: usage.cache_read_tokens,
        cache_write_tokens: usage.cache_write_tokens,
        request_count: usage.request_count.max(1),
    };
    let cost = memory::estimate_usage_cost(model, &tokens, Some(provider), Some(base_url), Some(api_key));
    let cost_usd = match cost.status {
        memory::CostStatus::Unknown => 0.0,
        _ => cost.amount_usd.unwrap_or(0.0),
    };
    let status = match cost.status {
        memory::CostStatus::Estimated => "estimated",
        memory::CostStatus::Included => "included",
        memory::CostStatus::Unknown => "unknown",
    };
    // 构造 NewUsageEvent + BillingDelta …
}
```

单测：已知快照模型 → status estimated；空 usage 不调用方提前 return。

- [x] **Step 2: `record_llm_usage` 改为**

1. `usage.is_empty()` → return  
2. `build_llm_usage_event(...)`  
3. `UsageDb::try_record(event)`  
4. `SessionStore::open_default()?.update_session_billing(session_id, delta)`（失败 warn）  
5. 需要 `AgentLoop` 的 `chat_provider` / `chat_base_url` / `chat_api_key`

- [x] **Step 3: cron_exec 同样传入 provider/base_url**

- [x] **Step 4: `cargo test -p agent -p memory -- --nocapture`**

- [x] **Step 5: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): dual-write route-aware LLM usage to events and sessions

EOF
)"
```

---

### Task 7: Insights 前端未计价提示

**Files:**
- Modify: `frontend/src/components/InsightsPanel.tsx`
- Modify: `frontend/src-tauri` 若 DTO 透传需改（serde 字段 `unpriced_llm_events`）

- [x] **Step 1: 扩展 TS 类型**

```ts
type UsageInsights = {
  kpis: { ... };
  unpriced_llm_events?: number;
  ...
};
```

- [x] **Step 2: 替换启发式**

```ts
const hasUnpriced =
  (data.unpriced_llm_events ?? 0) > 0 ||
  data.rankings.by_model.some((m) => m.calls > 0 && m.cost_usd <= 0 && /* 保留兜底 */ true);
```

优先只信 `unpriced_llm_events`：

```ts
const hasUnpriced = (data.unpriced_llm_events ?? 0) > 0;
```

- [x] **Step 3: 手动或既有前端构建确认类型通过**

Run: `cd frontend && npm run build`（若仓库惯用）或跳过若无 CI 前端

- [x] **Step 4: Commit**

```bash
git commit -m "$(cat <<'EOF'
fix(frontend): show unpriced LLM banner from insights field

EOF
)"
```

---

### Task 8: 文档收尾

**Files:**
- Modify: `docs/superpowers/specs/2026-07-13-route-aware-usage-pricing-design.md` → **状态: 已实现**
- Modify: `docs/superpowers/specs/2026-07-13-usage-insights-design.md` 费用来源备注改为路由定价（一句）

- [x] **Step 1: 更新状态与交叉引用**

- [x] **Step 2: Commit**

```bash
git commit -m "$(cat <<'EOF'
docs: mark route-aware usage pricing as implemented

EOF
)"
```

---

## Spec coverage checklist

| Spec 项 | Task |
|---------|------|
| 四桶 Usage + normalize | 1 |
| 路由定价公式 / 快照 | 2 |
| OpenRouter models 缓存 | 3 |
| usage.db 重建 + 字段 | 4 |
| Insights 排除 unknown + unpriced | 4、7 |
| sessions billing + 清零 | 5 |
| 双写流式/cron | 6 |
| 清 usage-stats | 5 |
| 禁用 LiteLLM 费用 | 2 |
| 无品牌字样 | 全程 |
| 文档状态 | 8 |

## Self-review notes

- `UsageTokens` 放 memory，避免 `memory` 依赖 `providers`。  
- `request_count` 在 `add_assign` 语义于 Task 1 固定为「每个非空 Usage 片段至少 +1」。  
- OpenRouter 真网刷新仅在有 api_key 且缓存过期时；单测只覆盖文件缓存。  
- 旧 P1a「停写 llm / Insights 读 sessions」**不**实现。
