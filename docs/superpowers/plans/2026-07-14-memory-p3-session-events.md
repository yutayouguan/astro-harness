# Memory P3 SessionEvents Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 落地会话级 `SubscribeSessionEvents`，用 toast / pending 角标 / 可配自动 refresh / 完整 `/memory` 子命令补齐记忆 P3 可感知闭环，并撤掉 Chat 流在 `Done` 后挂起等 review 的临时路径。

**Architecture:** Backend 持有 `SessionEventHub`（broadcast fan-out）；review / pending / dreaming 变更时 publish；新 RPC `SubscribeSessionEvents` 流出事件。Tauri 长订阅后 `app.emit("session_event")`；前端 toast、角标、可选 `refresh_memory`。回合内工具 live 写仍只走 Chat `memory_update`。

**Tech Stack:** tonic/protobuf、tokio broadcast、现有 Tauri listen/emit、`memory::config` yaml 合并、前端 slash registry

**Spec:** [`docs/superpowers/specs/2026-07-14-memory-p3-session-events-design.md`](../specs/2026-07-14-memory-p3-session-events-design.md)

---

## File map

| Path | Responsibility |
|------|----------------|
| `proto/proto/astro.proto` | `SubscribeSessionEvents` + `SessionEvent` messages |
| `crates/agent-server/src/session_events.rs`（新建） | `SessionEventHub`：subscribe / publish |
| `crates/agent-server/src/grpc/astro_service.rs` | 实现 RPC；review 改 publish；删除 `Done` 后 30s 挂起 |
| `crates/agent-server/src/lib.rs` / `main` | hub 注入 `AstroServiceImpl` |
| `crates/agent-core/src/memory_review_spawn.rs` | publish 用钩子（或通过传入 `Fn`/`Sender`）；可保留 `MemoryReviewNotify` 作内部摘要 |
| `crates/agent-memory/src/config.rs` | `auto_refresh_on_update` + setter |
| `crates/agent-memory/src/pending.rs` | approve/reject/enqueue 后回调或返回计数供 emitter |
| `crates/agent-memory/src/dreaming/mod.rs` | live 写成功后 publish 钩子（参数注入，避免 crate 环） |
| `apps/desktop/src-tauri/src/session_events.rs`（新建）或并入 `commands.rs` | 启动订阅任务 |
| `apps/desktop/src-tauri/src/memory_commands.rs` | settings 扩展；`approve_all` / `reject_all` |
| `apps/desktop/src-tauri/src/lib.rs` | 注册命令；setup 里启动订阅 |
| `apps/desktop/src/App.tsx` | listen、toast、角标、auto-refresh、slash |
| `apps/desktop/src/lib/composerCommands.ts` | `/memory` 子命令解析 |
| `apps/desktop/src/components/MemoryPanel.tsx` | 「刷新进对话」+ auto-refresh 开关 |
| `apps/desktop/src/i18n/messages.ts` / `styles/memory.css` | 文案与角标样式 |
| `docs/memory.md` | 用户文档 |

**命名：** 禁止外部参考项目品牌字符串。

---

## Task 1: Proto — `SubscribeSessionEvents`

**Files:**
- Modify: `proto/proto/astro.proto`
- Regenerates via `proto/build.rs` when dependents build

- [x] **Step 1: 在 `service AstroService` 增加 RPC，并追加 message**

在 `ListFiles` 后增加：

```protobuf
  // 会话级副作用事件（记忆更新 / pending 变化）；与 Chat 流生命周期解耦。
  rpc SubscribeSessionEvents(SubscribeSessionEventsRequest) returns (stream SessionEvent);
```

在文件末尾（`MemoryUpdateEvent` 附近或独立区域）追加：

```protobuf
message SubscribeSessionEventsRequest {
  // 空 = 只收全局 pending；非空 = 该 session + 全局 pending
  string session_id = 1;
  // 可选 agent 过滤；空 = 不过滤
  string agent_id = 2;
}

message SessionEvent {
  string session_id = 1;
  string agent_id = 2;
  int64 ts_ms = 3;
  oneof payload {
    MemoryUpdatedEvent memory_updated = 10;
    PendingChangedEvent pending_changed = 11;
  }
}

message MemoryUpdatedEvent {
  string source = 1;   // review | approve | dreaming | tool
  string target = 2;   // memory | user | mixed
  string summary = 3;
  bool live_written = 4;
}

message PendingChangedEvent {
  uint32 pending_count = 1;
  string reason = 2;   // enqueued | approved | rejected
}
```

注意：已有 `message MemoryUpdateEvent`（Chat 用，无 `d`）；新类型名是 **`MemoryUpdatedEvent`**（带 `d`），勿混淆。

- [x] **Step 2: 编译验证**

```bash
cargo check -p proto 2>&1 | tail -20
```

Expected: 成功（或 workspace 中实际 proto crate 名；若无单独 `proto` package，则 `cargo check -p backend`）

- [x] **Step 3: Commit**

```bash
git add proto/proto/astro.proto
git commit -m "$(cat <<'EOF'
feat(proto): add SubscribeSessionEvents for memory P3

EOF
)"
```

---

## Task 2: Backend `SessionEventHub`（TDD）

**Files:**
- Create: `crates/agent-server/src/session_events.rs`
- Modify: `crates/agent-server/src/lib.rs`（`mod session_events; pub use …`）
- Test: unit tests in `session_events.rs`

- [x] **Step 1: 写失败测试**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_reaches_matching_subscriber() {
        let hub = SessionEventHub::new(16);
        let mut rx = hub.subscribe(SubscribeFilter {
            session_id: Some("s1".into()),
            agent_id: None,
        });
        hub.publish(SessionEventMsg {
            session_id: Some("s1".into()),
            agent_id: "workspace".into(),
            memory_updated: Some(MemoryUpdatedPayload {
                source: "review".into(),
                target: "memory".into(),
                summary: "ok".into(),
                live_written: true,
            }),
            pending_changed: None,
        });
        let ev = tokio::time::timeout(std::time::Duration::from_millis(200), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(ev.session_id.as_deref(), Some("s1"));
        assert!(ev.memory_updated.unwrap().live_written);
    }

    #[tokio::test]
    async fn empty_session_filter_receives_global_pending() {
        let hub = SessionEventHub::new(16);
        let mut rx = hub.subscribe(SubscribeFilter {
            session_id: None,
            agent_id: None,
        });
        hub.publish(SessionEventMsg {
            session_id: None,
            agent_id: "workspace".into(),
            memory_updated: None,
            pending_changed: Some(PendingChangedPayload {
                pending_count: 2,
                reason: "enqueued".into(),
            }),
        });
        let ev = rx.recv().await.unwrap();
        assert_eq!(ev.pending_changed.unwrap().pending_count, 2);
    }

    #[test]
    fn publish_with_no_subscribers_does_not_panic() {
        let hub = SessionEventHub::new(8);
        hub.publish(SessionEventMsg {
            session_id: Some("x".into()),
            agent_id: "a".into(),
            memory_updated: Some(MemoryUpdatedPayload {
                source: "approve".into(),
                target: "user".into(),
                summary: "x".into(),
                live_written: true,
            }),
            pending_changed: None,
        });
    }
}
```

- [x] **Step 2: 运行测试 — 期望 FAIL**

```bash
cargo test -p backend session_events:: -- --nocapture 2>&1 | tail -30
```

Expected: 找不到 `SessionEventHub` 或 module

- [x] **Step 3: 最小实现**

`crates/agent-server/src/session_events.rs` 要点：

```rust
use tokio::sync::broadcast;

#[derive(Debug, Clone)]
pub struct SubscribeFilter {
    pub session_id: Option<String>,
    pub agent_id: Option<String>,
}

#[derive(Debug, Clone)]
pub struct MemoryUpdatedPayload {
    pub source: String,
    pub target: String,
    pub summary: String,
    pub live_written: bool,
}

#[derive(Debug, Clone)]
pub struct PendingChangedPayload {
    pub pending_count: u32,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct SessionEventMsg {
    pub session_id: Option<String>,
    pub agent_id: String,
    pub memory_updated: Option<MemoryUpdatedPayload>,
    pub pending_changed: Option<PendingChangedPayload>,
}

#[derive(Clone)]
pub struct SessionEventHub {
    tx: broadcast::Sender<SessionEventMsg>,
}

impl SessionEventHub {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish(&self, msg: SessionEventMsg) {
        let _ = self.tx.send(msg);
    }

    /// 返回已按 filter 包装的 receiver；调用方循环 `recv` 并自行过滤，
    /// 或提供 `subscribe_filtered` 异步流。推荐：subscribe 返回 `broadcast::Receiver`，
    /// RPC 层过滤：
    /// - `pending_changed` 且 request.session_id 为空或事件 session_id 为空 → 放行
    /// - `memory_updated`：request.session_id 为空则**不**收（除非也带 pending）；
    ///   非空则要求 `event.session_id == request` 或 event.session_id 为空且 agent 匹配
    pub fn subscribe_raw(&self) -> broadcast::Receiver<SessionEventMsg> {
        self.tx.subscribe()
    }
}

pub fn event_matches(filter: &SubscribeFilter, ev: &SessionEventMsg) -> bool {
    if let Some(ref want_agent) = filter.agent_id {
        if !want_agent.is_empty() && &ev.agent_id != want_agent {
            return false;
        }
    }
    match filter.session_id.as_deref().filter(|s| !s.is_empty()) {
        None => ev.pending_changed.is_some() && ev.session_id.is_none(),
        Some(sid) => {
            if ev.pending_changed.is_some() && ev.session_id.is_none() {
                return true;
            }
            ev.session_id.as_deref() == Some(sid)
        }
    }
}
```

把 `SessionEventMsg` → `proto::SessionEvent` 的转换函数放同文件：

```rust
pub fn to_proto(ev: &SessionEventMsg) -> proto::SessionEvent {
    let ts_ms = chrono::Utc::now().timestamp_millis(); // 或传入
    // 填 session_id/agent_id/ts_ms 与 oneof
    ...
}
```

若 backend 无 `chrono`，用 `std::time` 毫秒。

- [x] **Step 4: 测试通过**

```bash
cargo test -p backend session_events:: -- --nocapture 2>&1 | tail -20
```

Expected: PASS

- [x] **Step 5: Commit**

```bash
git add backend/src/session_events.rs backend/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(backend): add SessionEventHub for memory notifications

EOF
)"
```

---

## Task 3: 实现 `SubscribeSessionEvents` RPC + 注入 hub

**Files:**
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Modify: hub 构造处（`AstroServiceImpl::new` / `main`）

- [x] **Step 1: `AstroServiceImpl` 增加字段**

```rust
session_events: SessionEventHub,
```

构造时 `SessionEventHub::new(64)`。

- [x] **Step 2: 实现 RPC**

```rust
async fn subscribe_session_events(
    &self,
    request: Request<SubscribeSessionEventsRequest>,
) -> Result<Response<Self::SubscribeSessionEventsStream>, Status> {
    let req = request.into_inner();
    let filter = SubscribeFilter {
        session_id: if req.session_id.trim().is_empty() {
            None
        } else {
            Some(req.session_id)
        },
        agent_id: if req.agent_id.trim().is_empty() {
            None
        } else {
            Some(req.agent_id)
        },
    };
    let hub = self.session_events.clone();
    let (tx, rx) = tokio::sync::mpsc::channel(16);
    tokio::spawn(async move {
        let mut raw = hub.subscribe_raw();
        loop {
            match raw.recv().await {
                Ok(ev) if event_matches(&filter, &ev) => {
                    if tx.send(Ok(to_proto(&ev))).await.is_err() {
                        break;
                    }
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    });
    Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
}
```

Stream 关联类型按 tonic 生成签名对齐（可与 `ChatStream` 同样模式）。

- [x] **Step 3: `cargo check -p backend`**

Expected: 通过

- [x] **Step 4: Commit**

```bash
git commit -am "$(cat <<'EOF'
feat(backend): expose SubscribeSessionEvents RPC

EOF
)"
```

---

## Task 4: Review 发布走 hub，撤掉 Chat 挂起

**Files:**
- Modify: `crates/agent-server/src/grpc/astro_service.rs`（删除/改写 `spawn_review_and_emit_update`）
- Modify: `crates/agent-core/src/memory_review_spawn.rs`（可选：notify 仍返回摘要，backend 映射为 hub 事件）

- [x] **Step 1: 改写 review 启动**

替换所有 `spawn_review_and_emit_update(&session, &tx).await` 为：

```rust
spawn_review_to_hub(&session, &session_id, &self.session_events);
// 注意：在 spawn 的 async 块内 clone hub + sid，**不要** await review
```

新函数（同文件）：

```rust
fn spawn_review_to_hub(
    session: &SessionHandle,
    session_id: &str,
    hub: &SessionEventHub,
) {
    let hub = hub.clone();
    let sid = session_id.to_string();
    let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel();
    // 在已有 runtime 中：
    // 需要 async lock session — 调用点已在 async 内时：
}
```

在 Chat 处理任务内（已持有 `session`）推荐：

```rust
let hub = session_events_hub.clone();
let sid = session_id.clone();
let (notify_tx, mut notify_rx) = tokio::sync::mpsc::unbounded_channel();
{
    let agent = session.lock().await;
    let agent_id = agent.agent_id().to_string();
    agent::spawn_background_review_after_turn(&agent, Some(notify_tx));
    // agent_id 在 spawn 外 clone
}
let agent_id = { session.lock().await.agent_id().to_string() };
tokio::spawn(async move {
    if let Some(n) = notify_rx.recv().await {
        let live = !n.content.contains("待审批") && !n.content.contains("pending");
        hub.publish(SessionEventMsg {
            session_id: Some(sid),
            agent_id,
            memory_updated: Some(MemoryUpdatedPayload {
                source: "review".into(),
                target: "mixed".into(),
                summary: n.content,
                live_written: live,
            }),
            pending_changed: None, // 若仅 pending，Task 5 会发 pending_changed
        });
    }
});
// Chat 流在 Done 后 **立即** break，不再 timeout 30s
```

更干净：让 `maybe_run_background_review` 返回结构化结果（已有 `Vec<String>`），backend 根据是否含「待审批」/「入队」决定 `live_written` 与是否另发 `pending_changed`。

- [x] **Step 2: 删除 `spawn_review_and_emit_update` 整函数**

- [x] **Step 3: 确认 Chat 在 `is_done` 后不再 await review**

```rust
if is_done {
    // fire-and-forget hub publish path
    break;
}
```

- [x] **Step 4: `cargo test -p agent memory_review` && `cargo check -p backend`**

- [x] **Step 5: Commit**

```bash
git commit -am "$(cat <<'EOF'
feat(backend): publish review completion on SessionEventHub

Stop holding the Chat stream open for background review.
EOF
)"
```

---

## Task 5: Pending / dreaming emitters

**Files:**
- Modify: `crates/agent-memory/src/pending.rs` — `enqueue`/`approve`/`reject` 返回后由调用方 publish；或增加可选 `on_change: Option<&dyn Fn(PendingChangedPayload)>`（避免 memory→backend 依赖）
- Modify: Tauri `approve_pending_memory_write` / `reject_…` / enqueue 路径（manager）
- Modify: dreaming finalize 成功路径（Tauri `dreaming_commands` 或 memory dreaming）

**约定：** `memory` crate **不**依赖 backend。Publish 放在：

1. **Backend** review 路径（Task 4）  
2. **Tauri** approve/reject/enqueue（工具入 pending 若只在 agent 进程，则 agent/backend 在 `handle_memory_op` 返回含「待审批」时 publish）

- [x] **Step 1: 在 backend / agent 工具分发后**

当 `handle_memory_op_with_source` 返回字符串含「待审批」或「入队」时：

```rust
let count = memory::list_pending(&memory_dir).map(|v| v.len() as u32).unwrap_or(0);
hub.publish(SessionEventMsg {
    session_id: None, // 全局 pending
    agent_id: agent_id.clone(),
    memory_updated: Some(MemoryUpdatedPayload {
        source: "tool".into(),
        target: "...".into(),
        summary: "有待审批的记忆写入".into(),
        live_written: false,
    }),
    pending_changed: Some(PendingChangedPayload {
        pending_count: count,
        reason: "enqueued".into(),
    }),
});
```

工具路径若无 hub：在 `astro_service` multi-turn 处理 tool 结果处拦截（与现有 `MemoryUpdate` Chat 事件并列：live → Chat only；pending → hub）。

- [x] **Step 2: Tauri approve/reject**

```rust
// approve_pending_memory_write 成功后：
// 若有全局 SessionEvent 客户端困难，可用 app.emit 本地事件作为桥：
app.emit("session_event", SessionEventDto { ... })?;
```

P3 允许：**Tauri 本机操作**直接 `app.emit("session_event")`；**backend 来源**走 gRPC Subscribe。前端统一 listen `session_event`。

为免双轨复杂：Tauri 订阅 gRPC 的同时，approve 路径也 `emit` 同一 payload shape（本机即时）。

- [x] **Step 3: 入梦 live 写成功**

在 `dreaming_commands` finalize 成功后 `app.emit`：

```json
{
  "sessionId": null,
  "agentId": "...",
  "memoryUpdated": {
    "source": "dreaming",
    "target": "memory",
    "summary": "入梦已更新记忆",
    "liveWritten": true
  }
}
```

- [x] **Step 4: Commit**

```bash
git commit -am "$(cat <<'EOF'
feat(memory): emit pending and dreaming session events

EOF
)"
```

---

## Task 6: `auto_refresh_on_update` 配置

**Files:**
- Modify: `crates/agent-memory/src/config.rs`
- Modify: `apps/desktop/src-tauri/src/memory_commands.rs`（`MemorySettingsDto`）
- Test: `config.rs` 单测

- [x] **Step 1: 扩展 `MemoryConfig`**

```rust
fn default_true() -> bool { true } // 已有

#[serde(default = "default_true")]
pub auto_refresh_on_update: bool,
```

`Default` 与 `set_auto_refresh_on_update(base, enabled)`（同 `set_write_approval` 的 Value 合并）。

- [x] **Step 2: 测试**

```rust
#[test]
fn auto_refresh_defaults_true() {
    let dir = tempfile::tempdir().unwrap();
    assert!(load_memory_config(dir.path()).auto_refresh_on_update);
}

#[test]
fn set_auto_refresh_false() {
    let dir = tempfile::tempdir().unwrap();
    set_auto_refresh_on_update(dir.path(), false).unwrap();
    assert!(!load_memory_config(dir.path()).auto_refresh_on_update);
}
```

- [x] **Step 3: DTO**

```rust
pub struct MemorySettingsDto {
    pub write_approval: bool,
    pub background_review_enabled: bool,
    pub auto_refresh_on_update: bool,
}
```

增加命令 `set_memory_auto_refresh(enabled: bool)`。

- [x] **Step 4:**

```bash
cargo test -p memory --lib config:: -- --nocapture 2>&1 | tail -20
```

Expected: PASS

- [x] **Step 5: Commit**

```bash
git commit -am "$(cat <<'EOF'
feat(memory): add auto_refresh_on_update config flag

EOF
)"
```

---

## Task 7: Tauri 订阅 gRPC + 前端 toast / 角标 / auto-refresh

**Files:**
- Create or modify: `apps/desktop/src-tauri/src/session_events_cmd.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`（`setup` 启动后台订阅；`listen` 侧）
- Modify: `apps/desktop/src/App.tsx`
- Modify: nav 渲染（记忆项角标）
- Modify: `apps/desktop/src/i18n/messages.ts`

- [x] **Step 1: Tauri 后台任务**

在 `setup` 或首次拿到 `sessionId` 时：

```rust
// pseudocode
let mut client = AstroServiceClient::connect(endpoint).await?;
let mut stream = client
    .subscribe_session_events(SubscribeSessionEventsRequest {
        session_id: sid.unwrap_or_default(),
        agent_id: String::new(),
    })
    .await?
    .into_inner();
while let Some(ev) = stream.message().await? {
    app.emit("session_event", SessionEventDto::from(ev))?;
}
// 断线 sleep + 重连
```

提供命令 `set_session_events_filter(session_id: Option<String>)` 以便切换会话时重订。

- [x] **Step 2: 前端 listen**

```tsx
useEffect(() => {
  let unlisten: (() => void) | undefined;
  void listen<SessionEventDto>("session_event", (e) => {
    const p = e.payload;
    if (p.pendingChanged) {
      setMemoryPendingCount(p.pendingChanged.pendingCount);
    }
    if (p.memoryUpdated) {
      const live = p.memoryUpdated.liveWritten;
      showTransientToast(
        live ? t("memory.toast.updated") : t("memory.toast.pending"),
      );
      if (live && autoRefreshOnUpdate && sessionId) {
        void invoke("refresh_memory", { agentId: null, sessionId });
      }
    }
  }).then((u) => {
    unlisten = u;
  });
  return () => unlisten?.();
}, [sessionId, autoRefreshOnUpdate, t]);
```

挂载时 `invoke("get_memory_settings")` 读 `autoRefreshOnUpdate`；`list_pending_memory_writes` 校正角标。

- [x] **Step 3: Nav 角标**

记忆 nav 按钮旁：`memoryPendingCount > 0` 时显示数字 badge（CSS：`nav-badge`）。

- [x] **Step 4: 手测 / `cargo check -p astro-harness`**

- [x] **Step 5: Commit**

```bash
git commit -am "$(cat <<'EOF'
feat(ui): toast and badge for SessionEvents memory updates

EOF
)"
```

---

## Task 8: MemoryPanel —「刷新进对话」+ 开关

**Files:**
- Modify: `apps/desktop/src/components/MemoryPanel.tsx`
- Modify: `apps/desktop/src/styles/memory.css`
- Modify: i18n

- [x] **Step 1: 审批设置区增加开关**

与 `writeApproval` 并列：

```tsx
<label className="mem-settings-row">
  ...
  <button
    role="switch"
    aria-checked={memorySettings.autoRefreshOnUpdate}
    onClick={() => void setAutoRefresh(!memorySettings.autoRefreshOnUpdate)}
  />
</label>
```

- [x] **Step 2: 长期记忆 / 顶栏「刷新进对话」**

```tsx
await invoke("refresh_memory", {
  agentId: filterAgentId === ALL_AGENTS ? null : filterAgentId,
  sessionId: sessionId ?? null,
});
setSaveMsg(t("memory.refresh.done"));
```

- [x] **Step 3: Commit**

```bash
git commit -am "$(cat <<'EOF'
feat(ui): memory panel refresh-into-chat and auto-refresh toggle

EOF
)"
```

---

## Task 9: Slash 子命令 + approve/reject all

**Files:**
- Modify: `apps/desktop/src/lib/composerCommands.ts`
- Modify: `apps/desktop/src/App.tsx`（`handleSlashAction`）
- Modify: `apps/desktop/src-tauri/src/memory_commands.rs`
- Test: 可加 `composerCommands` 的 vitest/纯函数测；或 Rust 侧 all API 测

- [x] **Step 1: 解析扩展**

`parseSlashInput`：若 `name === "memory"` 且 `args` 非空，返回新 action：

```ts
| "memory_slash"; // args 原样交给 handler
```

或细分：`memory_list` | `memory_approve` | `memory_reject` | `memory_refresh`。

推荐在 `parseSlashInput` 内：

```ts
if (builtin.name === "memory" && args) {
  const [sub, rest] = splitFirst(args);
  switch (sub) {
    case "list":
      return { name: "memory", args, action: "memory_list" };
    case "approve":
      return { name: "memory", args: rest, action: "memory_approve" };
    case "reject":
      return { name: "memory", args: rest, action: "memory_reject" };
    case "refresh":
      return { name: "memory", args: "", action: "memory_refresh" };
    default:
      return { name: "memory", args, action: "memory_help" };
  }
}
```

扩展 `SlashAction` 联合类型；`BUILTIN` 仍保留裸 `nav_memory`。

- [x] **Step 2: Tauri batch API**

```rust
#[tauri::command]
pub async fn approve_all_pending_memory_writes() -> Result<String, String> {
    let root = memory::default_memory_dir();
    let items = memory::list_pending(&root).map_err(|e| e.to_string())?;
    let mut ok = 0usize;
    let mut err = 0usize;
    for p in items {
        match memory::approve_pending_memory(&root, &p.id) {
            Ok(_) => ok += 1,
            Err(_) => err += 1,
        }
    }
    Ok(format!("approved={ok} failed={err}"))
}
```

`reject_all` 同理。

- [x] **Step 3: App handler**

```ts
case "memory_approve": {
  const id = (args ?? "").trim();
  if (!id || id === "all") {
    const msg = await invoke<string>("approve_all_pending_memory_writes");
    showTransientToast(msg);
  } else {
    await invoke("approve_pending_memory_write", { id });
    showTransientToast(t("memory.pending.approved"));
  }
  if (autoRefresh) await invoke("refresh_memory", { agentId: null, sessionId });
  break;
}
```

`memory_list`：`list_pending_memory_writes` → toast 摘要（截断）。

- [x] **Step 4: 纯函数单测（若项目有 vitest）或手工核对解析**

```ts
expect(parseSlashInput("/memory approve all")?.action).toBe("memory_approve");
expect(parseSlashInput("/memory")?.action).toBe("nav_memory");
```

- [x] **Step 5: Commit**

```bash
git commit -am "$(cat <<'EOF'
feat(chat): /memory list approve reject refresh slash commands

EOF
)"
```

---

## Task 10: 文档与验收勾选

**Files:**
- Modify: `docs/memory.md`
- Modify: spec 验收列表（可选勾选）

- [x] **Step 1: 更新 `docs/memory.md`**

增加：

- `SubscribeSessionEvents` 说明  
- `memory.auto_refresh_on_update`  
- slash 表  
- 说明 Chat `Done` 不再等待 review  

- [x] **Step 2: 回归**

```bash
cargo test -p memory --lib 2>&1 | tail -15
cargo test -p backend session_events:: 2>&1 | tail -15
cargo check -p astro-harness 2>&1 | tail -15
rg -i hermes frontend memory agent backend proto --glob '!docs/**' || true
```

- [x] **Step 3: Commit**

```bash
git commit -am "$(cat <<'EOF'
docs(memory): document SessionEvents and /memory slash (P3)

EOF
)"
```

---

## Spec coverage check

| Spec 要求 | Task |
|-----------|------|
| `SubscribeSessionEvents` proto | 1 |
| Hub fan-out / 无订阅者丢弃 | 2 |
| RPC 实现 | 3 |
| review → SessionEvent；停 Chat 挂起 | 4 |
| pending / dreaming emit | 5 |
| `auto_refresh_on_update` | 6 |
| Toast / 角标 / 自动 refresh | 7 |
| 「刷新进对话」+ 开关 UI | 8 |
| Slash 矩阵 + approve all | 9 |
| 文档 / 品牌名检查 | 10 |
| 回合内 Chat `memory_update` 不变 | 4、5（不改 streaming MemoryUpdate 路径） |

---

## 执行提示

- 先完成 Task 1–4，否则前端 subscribe 无后端。  
- Tauri 与 backend 需同时跑才能手测 review toast。  
- 工作区若有无关 WIP，提交时只 stage 本计划文件。  
