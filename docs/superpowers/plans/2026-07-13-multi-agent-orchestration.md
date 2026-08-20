# Multi-Agent Orchestration MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 主 Agent 通过 `orchestration_run` 异步串行调度 1～N 个子 Agent（已有或临时角色），`orchestration_status` 可查进度；状态进 SQLite，并写遥测边。

**Architecture:** `memory` 管 `orchestration.db`；`agent` 管串行执行器（对齐 `cron_exec`）；`tools` 只负责参数校验、落库与触发 spawn。因 `tools` 不能依赖 `agent`，用 `memory`（或 `tools`）内 `OnceLock` 注册的 spawner 回调，由 `AgentLoop`/backend 启动时注入 `tokio::spawn(run_orchestration)`。

**Tech Stack:** Rust / rusqlite / tokio / schemars 工具注册、现有 `AgentLoop`

**Spec:** `docs/superpowers/specs/2026-07-13-multi-agent-orchestration-design.md`

> **Plan status:** Completed on `main`（2026-07-13）。实现已落地；勿再按本计划重复开工。

---

## File map

| 文件 | 职责 |
|------|------|
| `crates/agent-memory/src/orchestration_db.rs` | SQLite CRUD + 状态更新 |
| `crates/agent-memory/src/orchestration_spawn.rs` | `OnceLock` spawner + `OrchestrationSpawnRequest` |
| `crates/agent-memory/src/lib.rs` | 导出 |
| `memory/tests/orchestration_db_test.rs` | DB 单测 |
| `crates/agent-core/src/orchestration.rs` | `run_orchestration` 串行执行 |
| `crates/agent-core/src/lib.rs` | 导出 + 注册 spawner 辅助 |
| `crates/agent-tools/src/builtin/orchestration.rs` | `orchestration_run` / `orchestration_status` |
| `crates/agent-tools/src/builtin/mod.rs` / `dispatch.rs` / `lib.rs` | 注册与路由 |
| `crates/agent-memory/src/tools_enabled.rs` | `tool_name_to_toolset` 映射到 `multi_agent` |
| `frontend` hooks（可选） | 若 `KNOWN_TOOLSET`/`AGENT_TOOLS` 需展示新工具名，补文案；toolset 仍用 `multi_agent` 则可能无需改开关 |

**依赖方向：** `tools` → `memory`（insert + `request_spawn`）；`agent` → `memory` + `tools`（执行器 + 启动时 `set_spawner`）。禁止 `tools` → `agent`。

**Spawn 请求字段（勿把 API Key 长期明文堆在 DB）：**

```rust
pub struct OrchestrationSpawnRequest {
    pub orchestration_id: String,
    pub parent_agent_id: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}
```

工具从 `ToolContext` 的 `chat_*` 字段填入（见 `context.rs` 后续字段）。

---

### Task 1: `OrchestrationDb`（TDD）

**Files:**
- Create: `crates/agent-memory/src/orchestration_db.rs`
- Create: `memory/tests/orchestration_db_test.rs`
- Modify: `crates/agent-memory/src/lib.rs`

- [x] **Step 1: 写失败测试**

```rust
//! orchestration.db 测试

use memory::orchestration_db::{
    NewOrchestration, NewOrchestrationStep, OrchestrationDb, OrchestrationStatus, StepStatus,
};
use tempfile::TempDir;

#[test]
fn create_and_list_steps_in_order() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("orchestration.db")).unwrap();
    let id = db
        .create(NewOrchestration {
            parent_agent_id: "workspace".into(),
            session_id: Some("s1".into()),
            goal: "写周报".into(),
            steps: vec![
                NewOrchestrationStep {
                    role: "researcher".into(),
                    agent_id: None,
                    prompt: "收集素材".into(),
                },
                NewOrchestrationStep {
                    role: "writer".into(),
                    agent_id: Some("workspace".into()),
                    prompt: "起草".into(),
                },
            ],
        })
        .unwrap();
    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, OrchestrationStatus::Queued.as_str());
    let steps = db.list_steps(&id).unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].seq, 0);
    assert_eq!(steps[1].seq, 1);
    assert_eq!(steps[0].status, StepStatus::Pending.as_str());
}

#[test]
fn mark_step_failed_stops_semantics_helpers() {
    let dir = TempDir::new().unwrap();
    let db = OrchestrationDb::new(dir.path().join("o.db")).unwrap();
    let id = db
        .create(NewOrchestration {
            parent_agent_id: "workspace".into(),
            session_id: None,
            goal: "g".into(),
            steps: vec![NewOrchestrationStep {
                role: "a".into(),
                agent_id: None,
                prompt: "p".into(),
            }],
        })
        .unwrap();
    let steps = db.list_steps(&id).unwrap();
    db.set_orchestration_status(&id, OrchestrationStatus::Running, None, None)
        .unwrap();
    db.set_step_running(&steps[0].id).unwrap();
    db.set_step_failed(&steps[0].id, "boom").unwrap();
    db.set_orchestration_status(&id, OrchestrationStatus::Failed, Some("boom"), None)
        .unwrap();
    let orch = db.get(&id).unwrap().unwrap();
    assert_eq!(orch.status, "failed");
}
```

- [x] **Step 2: 运行确认失败**

`cargo test -p memory --test orchestration_db_test -- --nocapture`  
Expected: 编译失败

- [x] **Step 3: 实现 `orchestration_db.rs`**

对齐 `cron_run_db` / `usage_db`：

- DDL：`orchestrations` + `orchestration_steps`（列见 spec）
- 路径：`default_memory_dir().join("orchestration.db")`
- API：`new` / `open_default` / `create` / `get` / `list_steps` / `set_orchestration_status` / `set_step_running` / `set_step_done(output)` / `set_step_failed(error)` / `try_claim_running(id) -> bool`（CAS：仅 `queued`→`running`，防双 spawn）
- `output` 写入前 UTF-8 安全截断至 64KB
- 状态用 `&str` 常量或小 enum + `as_str()`

- [x] **Step 4: 测试通过并 commit**

```bash
cargo test -p memory --test orchestration_db_test
git add memory/src/orchestration_db.rs memory/src/lib.rs memory/tests/orchestration_db_test.rs
git commit -m "$(cat <<'EOF'
feat(memory): add orchestration.db for multi-agent runs

EOF
)"
```

---

### Task 2: Spawn 回调注册

**Files:**
- Create: `crates/agent-memory/src/orchestration_spawn.rs`
- Modify: `crates/agent-memory/src/lib.rs`

- [x] **Step 1: 实现**

```rust
//! 由 agent 在启动时注入；tools 在 orchestration_run 成功落库后调用。

use std::sync::{Arc, OnceLock};

pub struct OrchestrationSpawnRequest {
    pub orchestration_id: String,
    pub parent_agent_id: String,
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

pub type OrchestrationSpawner =
    Arc<dyn Fn(OrchestrationSpawnRequest) + Send + Sync + 'static>;

static SPAWNER: OnceLock<OrchestrationSpawner> = OnceLock::new();

pub fn set_orchestration_spawner(spawner: OrchestrationSpawner) {
    let _ = SPAWNER.set(spawner);
}

/// 未注册 spawner 时仅打 warn，不 panic（便于单测只测落库）。
pub fn request_orchestration_spawn(req: OrchestrationSpawnRequest) {
    if let Some(f) = SPAWNER.get() {
        f(req);
    } else {
        tracing::warn!(
            id = %req.orchestration_id,
            "orchestration spawner not registered; job stays queued"
        );
    }
}
```

- [x] **Step 2: 导出 + commit**

```bash
git add memory/src/orchestration_spawn.rs memory/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(memory): add orchestration spawn hook for tools→agent

EOF
)"
```

---

### Task 3: 串行执行器 `agent::orchestration`

**Files:**
- Create: `crates/agent-core/src/orchestration.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Modify: `crates/agent-core/src/loop_.rs` 或 `builder` / backend 启动处：调用 `set_orchestration_spawner`
- Test: `agent/tests/orchestration_test.rs`（可用短超时 + 假失败路径测状态机；真 LLM 可选 ignore）

- [x] **Step 1: 实现 `run_orchestration(req: OrchestrationSpawnRequest)`**

伪代码：

```rust
pub async fn run_orchestration(req: OrchestrationSpawnRequest) -> anyhow::Result<()> {
    let db = OrchestrationDb::open_default()?;
    if !db.try_claim_running(&req.orchestration_id)? {
        return Ok(()); // 已有人跑
    }
    let steps = db.list_steps(&req.orchestration_id)?;
    let orch = db.get(&req.orchestration_id)?.ok_or_else(|| anyhow::anyhow!("missing"))?;
    let mut prev_output = String::new();
    let mut summaries = Vec::new();

    for step in steps {
        db.set_step_running(&step.id)?;
        record_telemetry_start(...); // Task 5 可先 noop 或一并做

        let prompt = if prev_output.is_empty() {
            step.prompt.clone()
        } else {
            format!(
                "{}\n\n## Previous step output (truncated)\n{}",
                step.prompt,
                truncate(&prev_output, 8_000)
            )
        };

        let result = tokio::time::timeout(
            Duration::from_secs(120),
            run_step(&req, &orch, &step, &prompt),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                db.set_step_done(&step.id, &output)?;
                record_telemetry_end(ok);
                summaries.push(format!("### {}\n{}", step.role, truncate(&output, 2_000)));
                prev_output = output;
            }
            Ok(Err(e)) => {
                db.set_step_failed(&step.id, &e.to_string())?;
                db.set_orchestration_status(..., Failed, Some(e.to_string()), None)?;
                record_telemetry_end(err);
                return Ok(());
            }
            Err(_) => {
                db.set_step_failed(&step.id, "step timeout (120s)")?;
                db.set_orchestration_status(..., Failed, Some("timeout".into()), None)?;
                return Ok(());
            }
        }
    }

    let summary = summaries.join("\n\n");
    db.set_orchestration_status(..., Done, None, Some(summary))?;
    Ok(())
}
```

`run_step`：

- 若 `step.agent_id` 非空：`AgentConfig`/`MemoryManager` 指向该 agent（参考 `create_agent` / workspace 切换）；凭据：优先该 agent 配置，缺省用 `req` 父凭据。
- 若空：临时 `session_id`，**不**建 workspace；`run_turn` 的用户消息前缀含 `Role: {role}\nGoal: {goal}\n\n{prompt}`；凭据用 `req`。
- 执行：`run_turn`；若 `Continue` 则复用 `cron_exec` 风格短 `run_provider_loop`（可抽公共或复制精简版，**最多 5 轮**）。
- 禁止子编排无限递归：子 Agent 工具列表可暂时禁用 `orchestration_run`（reload 后过滤或 tools-enabled 覆盖）——MVP 至少在 `run_step` 文档注明；实现上优先在临时角色里 `tool_registry` 去掉 orchestration_*。

- [x] **Step 2: 注册 spawner**

在 `AgentLoop::with_session_id` 末尾或 `backend`/`tauri` 启动时：

```rust
memory::set_orchestration_spawner(Arc::new(|req| {
    tokio::spawn(async move {
        if let Err(e) = agent::orchestration::run_orchestration(req).await {
            tracing::warn!(error = %e, "orchestration failed");
        }
    });
}));
```

注意：`OnceLock` 只 set 一次；多 AgentLoop 构造时用 `get_or_init` 或先 `SPAWNER.get().is_none()`。

- [x] **Step 3: 单测**

最少：手动 insert orchestration + steps，直接 `await run_orchestration` 且 `run_step` 通过 `#[cfg(test)]` 可注入假执行器 **或** 测 `try_claim_running` 双调第二次 no-op。若假执行器成本高，则 DB 状态机测已在 Task 1，本 Task 用 `cargo check -p agent` + 一个 ignore 的集成测骨架。

- [x] **Step 4: Commit**

```bash
git add agent/src/orchestration.rs agent/src/lib.rs agent/src/loop_.rs # + tests
git commit -m "$(cat <<'EOF'
feat(agent): serial orchestration runner with spawn hook

EOF
)"
```

---

### Task 4: 工具 `orchestration_run` / `orchestration_status`

**Files:**
- Create: `crates/agent-tools/src/builtin/orchestration.rs`
- Modify: `crates/agent-tools/src/builtin/mod.rs`、`crates/agent-tools/src/lib.rs`（`register_all`）、`crates/agent-tools/src/engine/dispatch.rs`
- Modify: `crates/agent-memory/src/tools_enabled.rs`：`orchestration_run` | `orchestration_status` → `multi_agent`
- Modify: `apps/desktop/src/hooks/useAgentTools.ts`（若有工具名录需展示描述；toolset 仍为 multi_agent）

- [x] **Step 1: 实现工具**

```rust
#[derive(Deserialize, Serialize, JsonSchema)]
pub struct OrchestrationStepArgs {
    pub role: String,
    pub prompt: String,
    #[serde(default)]
    pub agent_id: Option<String>,
}

#[derive(Deserialize, Serialize, JsonSchema)]
pub struct OrchestrationRunArgs {
    pub goal: String,
    pub steps: Vec<OrchestrationStepArgs>,
}

// register: toolset "multi_agent", names orchestration_run / orchestration_status
```

`dispatch_run(ctx, args)`：

1. 校验 `goal` 非空、`steps.len()` ∈ 1..=8，每步 `role`/`prompt` 非空  
2. `OrchestrationDb::open_default()?.create(...)`  
3. `request_orchestration_spawn(OrchestrationSpawnRequest { ..., provider: ctx.chat_provider.clone(), ... })`  
4. 返回 JSON 字符串：`{"orchestration_id":"...","status":"queued"}`

`dispatch_status`：`get` + `list_steps`，序列化为可读 JSON/Markdown。

- [x] **Step 2: 接线 dispatch**

```rust
"orchestration_run" => crate::orchestration::dispatch_run(ctx, args),
"orchestration_status" => crate::orchestration::dispatch_status(args),
```

- [x] **Step 3: 更新 `multi_agent` / `delegate` 描述文案**（可选一句：推荐改用 orchestration_*）

- [x] **Step 4: 测试**

`tools` 侧单测：校验空 steps / >8 报错（不依赖 spawn）。

```bash
cargo test -p tools
cargo check -p agent -p tools
```

- [x] **Step 5: Commit**

```bash
git add tools/src/builtin/orchestration.rs tools/src/builtin/mod.rs tools/src/lib.rs \
  tools/src/engine/dispatch.rs memory/src/tools_enabled.rs
git commit -m "$(cat <<'EOF'
feat(tools): add orchestration_run and orchestration_status

EOF
)"
```

---

### Task 5: 遥测边

**Files:**
- Modify: `crates/agent-core/src/orchestration.rs`（step start/end）
- Optionally extend Insights KPI later — **本 Task 不改前端**

- [x] **Step 1: 写入 `usage_events`**

```rust
memory::UsageDb::try_record(NewUsageEvent {
    kind: "orchestration".into(), // Insights KPI 暂不计入 calls（与 skill 类似）；或先用 tool/multi_agent
    name: "orchestration_step".into(),
    agent_id: parent_or_step,
    meta_json: Some(json!({
        "orchestration_id": ...,
        "step_id": ...,
        "seq": ...,
        "from": parent_agent_id,
        "to": agent_id_or_format!("role:{role}"),
        "phase": "start" | "end",
        "ok": true/false
    }).to_string()),
    ...
});
```

**决定（写死在实现）：** `kind = "orchestration"`，且 **暂不**把该 kind 加入 Insights `CALLS_KIND_SQL`（避免虚高）；协作图二期再读。

- [x] **Step 2: 单测** — tempfile + `ASTRO_MEMORY_DIR`，跑完一步后查 `usage.db` 有 orchestration 行（可在 executor 测里做）。

- [x] **Step 3: Commit**

```bash
git commit -m "$(cat <<'EOF'
feat(agent): emit orchestration usage_events for handoff edges

EOF
)"
```

---

### Task 6: 回归与验收对照

- [x] **Step 1: 命令**

```bash
cargo test -p memory --test orchestration_db_test
cargo test -p memory --test usage_db_test
cargo check -p memory -p tools -p agent -p backend -p astro-agent
```

- [x] **Step 2: 手工（可选）**  
聊天启用 multi_agent 工具集 → 调 `orchestration_run` 两步（一步临时角色、一步 agent_id）→ 轮询 `orchestration_status` 至 done/failed。

- [x] **Step 3: 对照 spec 验收清单勾选；无阻断则完成**

---

## Self-review（plan vs spec）

| Spec | Task |
|------|------|
| orchestration.db 两表 | Task 1 |
| 异步 spawn + 防双跑 | Task 2–3 |
| 串行 / 失败中止 / 120s | Task 3 |
| 已有 Agent + 临时角色 | Task 3 |
| orchestration_run/status | Task 4 |
| 遥测 meta 边 | Task 5 |
| 旧工具并存、无 2D/并行/队列 | 未列入 ✅ |

**类型名：** `OrchestrationSpawnRequest` / `OrchestrationDb` / 工具名在各 Task 保持一致。
