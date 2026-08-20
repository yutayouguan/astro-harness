# Agent Loop Codex Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 将 Astro Agent Loop 落成 `AstroThread::submit(Op) → bounded submission queue → Session::submission_loop → run_turn → EventMsg → rollout → app-server/TUI/exec` 的单一事件事实链。

**Architecture:** 新增独立的 `agent-protocol` 与 `agent-rollout` crate，分别承载 Core 领域协议和 append-only JSONL 历史。Core 只暴露一个 `AstroThread` 顺序事件 receiver；Server 以单 listener 重建活动 Turn，并通过每连接 128 容量的队列分发。恢复使用 rollout + active snapshot，旧 Chat/SessionEvents 仅在迁移期作为新协议适配器。

**Tech Stack:** Rust 2021、Tokio、async-channel、serde/JSONL、tonic/protobuf、Tauri 2、React/TypeScript

**Spec:** [`docs/superpowers/specs/2026-08-18-agent-loop-codex-alignment-design.md`](../specs/2026-08-18-agent-loop-codex-alignment-design.md)

**Codex source baseline:** `/Users/iswm/CodeRope/codex/codex-rs` at `ede5247893a50297a47c9aa5038e6ab28312ff50`

---

## Execution gate

The implementation worktree must be clean before Task 1. The main Astro checkout currently contains owner-controlled, uncommitted changes in:

- `crates/agent-core/src/runtime/context_maintenance.rs`
- `crates/agent-core/src/runtime/mod.rs`
- `crates/agent-core/src/runtime/recording.rs`
- `crates/agent-core/src/runtime/turn_lifecycle.rs`

Those changes move conversation history into `SessionState` and convert recording methods to async. They are a prerequisite for removing the outer `Arc<Mutex<Session>>`, but they must not be copied from a dirty worktree.

- [x] Run `git -C /Users/iswm/Desktop/04-知识库/Rust/code/astro status --short`.
- [x] If any of the four files above are still modified, stop execution and ask their owner to commit them or explicitly discard them.
- [x] Rebase `codex/agent-loop-codex-alignment` onto the resulting committed branch tip.
- [x] Run `cargo test -p agent runtime:: --lib` and `cargo test -p agent tasks:: --lib`.

Expected: both commands pass before protocol work begins.

## File map

| Path | Responsibility |
|---|---|
| `crates/agent-protocol/` | `Submission`, `Op`, `Event`, `EventMsg`, `TurnItem`, terminal and delta payloads |
| `crates/agent-rollout/` | JSONL recorder, persistence policy, flush/shutdown, history reconstruction |
| `crates/agent-core/src/runtime/session_io.rs` | 512 submission channel, single Core event channel, status and termination handles |
| `crates/agent-core/src/runtime/astro_thread.rs` | Stable `AstroThread` API and Session loop ownership |
| `crates/agent-core/src/runtime/submission_loop.rs` | Ordered Op dispatch |
| `crates/agent-core/src/runtime/session_state.rs` | Session-owned mutable history/config state |
| `crates/agent-core/src/tasks/` | Spawned task lifecycle and exactly-one terminal event |
| `crates/agent-core/src/streaming/` | Provider/tool events mapped into `EventMsg` |
| `crates/agent-core/src/exec/background.rs` | Headless consumer of the same EventMsg stream |
| `crates/agent-server/src/thread_state.rs` | Active Turn snapshot and subscription membership |
| `crates/agent-server/src/thread_listener.rs` | Single receiver per loaded Thread and EventMsg mapping |
| `crates/agent-server/src/transport.rs` | Per-connection capacity-128 queues and slow-consumer disconnect |
| `crates/agent-server/src/grpc/thread_service.rs` | submit/resume/unsubscribe/thread-events RPC implementation |
| `crates/agent-proto/proto/astro.proto` | gRPC thread protocol and compatibility messages |
| `apps/desktop/src-tauri/src/infra/thread_events.rs` | One Tauri connection stream, reconnect, resume, app events |
| `apps/desktop/src-tauri/src/commands/chat.rs` | Submit TurnInput instead of owning an Agent loop |
| `crates/agent-server/src/session_events.rs` | Removed after Extension events migrate |
| `crates/agent-core/src/event_bus.rs` | Removed after all consumers use EventMsg |

## Batch A — Domain protocol and rollout

### Task 1: Create the Core domain protocol crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/agent-protocol/Cargo.toml`
- Create: `crates/agent-protocol/src/lib.rs`
- Create: `crates/agent-protocol/src/items.rs`
- Create: `crates/agent-protocol/src/event.rs`
- Create: `crates/agent-protocol/src/submission.rs`

- [x] **Step 1: Write protocol round-trip and terminal tests**

Add these tests at the bottom of `crates/agent-protocol/src/event.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::items::{TextItem, TurnItem};

    #[test]
    fn item_completed_roundtrips_without_losing_identity() {
        let event = Event {
            id: "turn-1".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "turn-1".into(),
                item: TurnItem::AgentMessage(TextItem {
                    id: "item-1".into(),
                    content: "done".into(),
                }),
            }),
        };
        let json = serde_json::to_string(&event).unwrap();
        let restored: Event = serde_json::from_str(&json).unwrap();
        assert_eq!(restored, event);
    }

    #[test]
    fn only_complete_and_aborted_are_terminal() {
        assert!(EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "t".into(),
            last_agent_message: None,
            error: None,
        })
        .is_terminal());
        assert!(EventMsg::TurnAborted(TurnAbortedEvent {
            turn_id: Some("t".into()),
            reason: TurnAbortReason::Interrupted,
        })
        .is_terminal());
        assert!(!EventMsg::Error(ErrorEvent {
            message: "failed".into(),
            error_type: "internal".into(),
        })
        .is_terminal());
    }
}
```

- [x] **Step 2: Run the tests and verify they fail**

Run:

```bash
cargo test -p agent-protocol
```

Expected: FAIL because package `agent-protocol` does not exist.

- [x] **Step 3: Add the package and workspace member**

Add `"crates/agent-protocol"` to `[workspace].members` in root `Cargo.toml`.

Create `crates/agent-protocol/Cargo.toml`:

```toml
[package]
name = "agent-protocol"
version = "0.1.0"
edition = "2021"

[dependencies]
types = { path = "../agent-types" }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }
tokio = { workspace = true }
```

Create `crates/agent-protocol/src/lib.rs`:

```rust
pub mod event;
pub mod items;
pub mod submission;

pub use event::*;
pub use items::*;
pub use submission::*;
```

- [x] **Step 4: Implement `TurnItem` payloads**

Create `crates/agent-protocol/src/items.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TextItem {
    pub id: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolItem {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    pub output: Option<Value>,
    pub status: ToolStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolStatus {
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExtensionItem {
    pub id: String,
    pub namespace: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum TurnItem {
    UserMessage(TextItem),
    HookPrompt(TextItem),
    AgentMessage(TextItem),
    Plan(TextItem),
    Reasoning(TextItem),
    CommandExecution(ToolItem),
    DynamicToolCall(ToolItem),
    McpToolCall(ToolItem),
    CollabAgentToolCall(ToolItem),
    SubAgentActivity(TextItem),
    WebSearch(ToolItem),
    ImageView(ToolItem),
    ImageGeneration(ToolItem),
    FileChange(ToolItem),
    ContextCompaction(TextItem),
    EnteredReviewMode(TextItem),
    ExitedReviewMode(TextItem),
    Extension(ExtensionItem),
}

impl TurnItem {
    pub fn id(&self) -> &str {
        match self {
            Self::UserMessage(item)
            | Self::HookPrompt(item)
            | Self::AgentMessage(item)
            | Self::Plan(item)
            | Self::Reasoning(item)
            | Self::SubAgentActivity(item)
            | Self::ContextCompaction(item)
            | Self::EnteredReviewMode(item)
            | Self::ExitedReviewMode(item) => &item.id,
            Self::CommandExecution(item)
            | Self::DynamicToolCall(item)
            | Self::McpToolCall(item)
            | Self::CollabAgentToolCall(item)
            | Self::WebSearch(item)
            | Self::ImageView(item)
            | Self::ImageGeneration(item)
            | Self::FileChange(item) => &item.id,
            Self::Extension(item) => &item.id,
        }
    }
}
```

- [x] **Step 5: Implement the unified Event envelope**

Create `crates/agent-protocol/src/event.rs` with these public types:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::items::{TextItem, TurnItem};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    pub id: String,
    pub msg: EventMsg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ItemEvent {
    pub turn_id: String,
    pub item: TurnItem,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeltaEvent {
    pub turn_id: String,
    pub item_id: String,
    pub delta: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ControlRequestEvent {
    pub turn_id: String,
    pub item_id: String,
    pub request_id: String,
    pub payload: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnStartedEvent {
    pub turn_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorEvent {
    pub message: String,
    pub error_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnCompleteEvent {
    pub turn_id: String,
    pub last_agent_message: Option<String>,
    pub error: Option<ErrorEvent>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnAbortReason {
    Interrupted,
    Replaced,
    ReviewEnded,
    BudgetLimited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnAbortedEvent {
    pub turn_id: Option<String>,
    pub reason: TurnAbortReason,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCountEvent {
    pub turn_id: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub total_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum EventMsg {
    TurnStarted(TurnStartedEvent),
    ItemStarted(ItemEvent),
    ItemCompleted(ItemEvent),
    AgentMessageContentDelta(DeltaEvent),
    PlanDelta(DeltaEvent),
    ReasoningContentDelta(DeltaEvent),
    ExecCommandOutputDelta(DeltaEvent),
    PatchApplyUpdated(DeltaEvent),
    ExecApprovalRequest(ControlRequestEvent),
    ApplyPatchApprovalRequest(ControlRequestEvent),
    RequestPermissions(ControlRequestEvent),
    RequestUserInput(ControlRequestEvent),
    ElicitationRequest(ControlRequestEvent),
    DynamicToolCallRequest(ControlRequestEvent),
    DynamicToolCallResponse(ControlRequestEvent),
    McpToolCallBegin(ItemEvent),
    McpToolCallEnd(ItemEvent),
    HookStarted(ItemEvent),
    HookCompleted(ItemEvent),
    SubAgentActivity(ItemEvent),
    ContextCompacted(ItemEvent),
    LegacyUserMessage(TextItem),
    LegacyAgentMessage(TextItem),
    LegacyReasoning(TextItem),
    LegacyMcpToolCallEnd(ItemEvent),
    LegacyPatchApplyEnd(ItemEvent),
    LegacyContextCompacted(ItemEvent),
    LegacySubAgentActivity(ItemEvent),
    TokenCount(TokenCountEvent),
    ThreadSettingsApplied(Value),
    ThreadRolledBack(Value),
    Error(ErrorEvent),
    Warning(ErrorEvent),
    StreamError(ErrorEvent),
    TurnComplete(TurnCompleteEvent),
    TurnAborted(TurnAbortedEvent),
    ShutdownComplete,
}

impl EventMsg {
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::TurnComplete(_) | Self::TurnAborted(_))
    }
}
```

- [x] **Step 6: Implement submission operations and turn-input decisions**

Create `crates/agent-protocol/src/submission.rs`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::oneshot;

use crate::items::ExtensionItem;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnInput {
    pub content: String,
    pub image_data_urls: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnInputRequest {
    pub input: Vec<TurnInput>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnInputMode {
    StartOrSteer,
    StartIfIdle,
    Steer { expected_turn_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TurnInputSubmission {
    Started { turn_id: String },
    Steered { turn_id: String },
    NotSubmitted { reason: String },
}

impl TurnInputSubmission {
    pub fn turn_id(&self) -> Option<&str> {
        match self {
            Self::Started { turn_id } | Self::Steered { turn_id } => Some(turn_id),
            Self::NotSubmitted { .. } => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum TurnInputError {
    #[error("session submission queue is closed")]
    QueueClosed,
    #[error("turn input reply channel closed")]
    ReplyClosed,
    #[error("invalid turn input: {0}")]
    Invalid(String),
}

#[derive(Debug)]
pub enum Op {
    TurnInput {
        request: TurnInputRequest,
        mode: TurnInputMode,
        reply: oneshot::Sender<Result<TurnInputSubmission, TurnInputError>>,
    },
    Interrupt,
    ThreadSettings { settings: Value },
    ExecApproval { id: String, decision: Value },
    PatchApproval { id: String, decision: Value },
    UserInputAnswer { id: String, response: Value },
    RequestPermissionsResponse { id: String, response: Value },
    DynamicToolResponse { id: String, response: Value },
    RefreshMcpServers,
    ReloadUserConfig,
    Compact,
    ThreadRollback { num_turns: u32 },
    Review { request: Value },
    InterAgentCommunication { communication: Value },
    EmitExtension { item: ExtensionItem },
    Shutdown,
}

#[derive(Debug)]
pub struct Submission {
    pub id: String,
    pub op: Op,
}
```

- [x] **Step 7: Run protocol tests**

Run:

```bash
cargo test -p agent-protocol
cargo check -p agent-protocol
```

Expected: PASS.

- [x] **Step 8: Commit**

```bash
git add Cargo.toml Cargo.lock crates/agent-protocol
git commit -m "feat(protocol): add unified thread event contract"
```

### Task 2: Create rollout persistence policy

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/agent-rollout/Cargo.toml`
- Create: `crates/agent-rollout/src/lib.rs`
- Create: `crates/agent-rollout/src/policy.rs`

- [x] **Step 1: Write policy tests**

Add to `crates/agent-rollout/src/policy.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{
        DeltaEvent, ErrorEvent, EventMsg, ItemEvent, TextItem, TurnCompleteEvent, TurnItem,
    };

    #[test]
    fn paginated_persists_completed_items_but_not_deltas() {
        let completed = EventMsg::ItemCompleted(ItemEvent {
            turn_id: "t".into(),
            item: TurnItem::AgentMessage(TextItem {
                id: "i".into(),
                content: "answer".into(),
            }),
        });
        let delta = EventMsg::AgentMessageContentDelta(DeltaEvent {
            turn_id: "t".into(),
            item_id: "i".into(),
            delta: "a".into(),
        });
        assert!(should_persist_event_msg(&completed, ThreadHistoryMode::Paginated));
        assert!(!should_persist_event_msg(&delta, ThreadHistoryMode::Paginated));
    }

    #[test]
    fn terminal_state_is_durable_but_error_notification_is_transient() {
        let complete = EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "t".into(),
            last_agent_message: None,
            error: None,
        });
        let error = EventMsg::Error(ErrorEvent {
            message: "failed".into(),
            error_type: "internal".into(),
        });
        assert!(should_persist_event_msg(&complete, ThreadHistoryMode::Paginated));
        assert!(!should_persist_event_msg(&error, ThreadHistoryMode::Paginated));
    }

    #[test]
    fn legacy_completion_events_only_persist_in_legacy_mode() {
        let legacy = EventMsg::LegacyAgentMessage(TextItem {
            id: "i".into(),
            content: "answer".into(),
        });
        assert!(should_persist_event_msg(&legacy, ThreadHistoryMode::Legacy));
        assert!(!should_persist_event_msg(&legacy, ThreadHistoryMode::Paginated));
    }
}
```

- [x] **Step 2: Run the test and verify it fails**

Run `cargo test -p agent-rollout policy`.

Expected: FAIL because package `agent-rollout` does not exist.

- [x] **Step 3: Add the rollout package**

Add `"crates/agent-rollout"` to root workspace members.

Create `crates/agent-rollout/Cargo.toml`:

```toml
[package]
name = "agent-rollout"
version = "0.1.0"
edition = "2021"

[dependencies]
agent-protocol = { path = "../agent-protocol" }
types = { path = "../agent-types" }
serde = { workspace = true }
serde_json = { workspace = true }
tokio = { workspace = true }
thiserror = { workspace = true }
chrono = { version = "0.4", features = ["clock"] }

[dev-dependencies]
tempfile = "3"
```

Create `crates/agent-rollout/src/lib.rs`:

```rust
mod policy;

pub use policy::*;

use agent_protocol::EventMsg;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThreadHistoryMode {
    Legacy,
    Paginated,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "data", rename_all = "snake_case")]
pub enum RolloutItem {
    SessionMeta(serde_json::Value),
    ResponseItem(types::message::Message),
    EventMsg(EventMsg),
    TurnContext(serde_json::Value),
    WorldState(serde_json::Value),
    Compacted(serde_json::Value),
    InterAgentCommunication(serde_json::Value),
}
```

- [x] **Step 4: Implement the centralized policy**

Create `crates/agent-rollout/src/policy.rs`:

```rust
use agent_protocol::EventMsg;

use crate::{RolloutItem, ThreadHistoryMode};

pub fn should_persist_event_msg(event: &EventMsg, mode: ThreadHistoryMode) -> bool {
    match event {
        EventMsg::ItemCompleted(_) => matches!(mode, ThreadHistoryMode::Paginated),
        EventMsg::LegacyUserMessage(_)
        | EventMsg::LegacyAgentMessage(_)
        | EventMsg::LegacyReasoning(_)
        | EventMsg::LegacyMcpToolCallEnd(_)
        | EventMsg::LegacyPatchApplyEnd(_)
        | EventMsg::LegacyContextCompacted(_)
        | EventMsg::LegacySubAgentActivity(_) => matches!(mode, ThreadHistoryMode::Legacy),
        EventMsg::TurnStarted(_)
        | EventMsg::TurnComplete(_)
        | EventMsg::TurnAborted(_)
        | EventMsg::TokenCount(_)
        | EventMsg::ThreadSettingsApplied(_)
        | EventMsg::ThreadRolledBack(_) => true,
        EventMsg::ItemStarted(_)
        | EventMsg::AgentMessageContentDelta(_)
        | EventMsg::PlanDelta(_)
        | EventMsg::ReasoningContentDelta(_)
        | EventMsg::ExecCommandOutputDelta(_)
        | EventMsg::PatchApplyUpdated(_)
        | EventMsg::ExecApprovalRequest(_)
        | EventMsg::ApplyPatchApprovalRequest(_)
        | EventMsg::RequestPermissions(_)
        | EventMsg::RequestUserInput(_)
        | EventMsg::ElicitationRequest(_)
        | EventMsg::DynamicToolCallRequest(_)
        | EventMsg::DynamicToolCallResponse(_)
        | EventMsg::McpToolCallBegin(_)
        | EventMsg::McpToolCallEnd(_)
        | EventMsg::HookStarted(_)
        | EventMsg::HookCompleted(_)
        | EventMsg::SubAgentActivity(_)
        | EventMsg::ContextCompacted(_)
        | EventMsg::Error(_)
        | EventMsg::Warning(_)
        | EventMsg::StreamError(_)
        | EventMsg::ShutdownComplete => false,
    }
}

pub fn is_persisted_rollout_item(item: &RolloutItem, mode: ThreadHistoryMode) -> bool {
    match item {
        RolloutItem::EventMsg(event) => should_persist_event_msg(event, mode),
        RolloutItem::SessionMeta(_)
        | RolloutItem::ResponseItem(_)
        | RolloutItem::TurnContext(_)
        | RolloutItem::WorldState(_)
        | RolloutItem::Compacted(_)
        | RolloutItem::InterAgentCommunication(_) => true,
    }
}
```

- [x] **Step 5: Run policy tests**

Run `cargo test -p agent-rollout policy`.

Expected: PASS.

- [x] **Step 6: Commit**

```bash
git add Cargo.toml Cargo.lock crates/agent-rollout
git commit -m "feat(rollout): add codex persistence policy"
```

### Task 3: Implement append-only JSONL recording and reconstruction

**Files:**
- Create: `crates/agent-rollout/src/recorder.rs`
- Create: `crates/agent-rollout/src/reconstruction.rs`
- Create: `crates/agent-rollout/src/path.rs`
- Modify: `crates/agent-rollout/src/lib.rs`

- [x] **Step 1: Write recorder ordering and flush tests**

Add to `crates/agent-rollout/src/recorder.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{read_rollout, RolloutItem, ThreadHistoryMode};
    use agent_protocol::{EventMsg, TurnStartedEvent};

    #[tokio::test]
    async fn record_then_flush_preserves_append_order() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("rollout.jsonl");
        let recorder = RolloutRecorder::open(path.clone(), ThreadHistoryMode::Paginated)
            .await
            .unwrap();
        recorder
            .record(vec![
                RolloutItem::SessionMeta(serde_json::json!({"thread_id":"thread-1"})),
                RolloutItem::EventMsg(EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: "turn-1".into(),
                })),
            ])
            .await
            .unwrap();
        recorder.flush().await.unwrap();
        let items = read_rollout(&path).await.unwrap();
        assert!(matches!(items[0], RolloutItem::SessionMeta(_)));
        assert!(matches!(items[1], RolloutItem::EventMsg(EventMsg::TurnStarted(_))));
        recorder.shutdown().await.unwrap();
    }
}
```

- [x] **Step 2: Run the test and verify it fails**

Run `cargo test -p agent-rollout recorder::tests::record_then_flush_preserves_append_order`.

Expected: FAIL because `RolloutRecorder` is not defined.

- [x] **Step 3: Implement the recorder command loop**

First extend `crates/agent-rollout/src/lib.rs`:

```rust
mod path;
mod recorder;
mod reconstruction;

pub use path::*;
pub use recorder::*;
pub use reconstruction::*;
```

Create `crates/agent-rollout/src/recorder.rs`:

```rust
use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;
use tokio::sync::{mpsc, oneshot};

use crate::{is_persisted_rollout_item, RolloutItem, ThreadHistoryMode};

enum RecorderCommand {
    Record {
        items: Vec<RolloutItem>,
        reply: oneshot::Sender<std::io::Result<()>>,
    },
    Flush {
        reply: oneshot::Sender<std::io::Result<()>>,
    },
    Shutdown {
        reply: oneshot::Sender<std::io::Result<()>>,
    },
}

#[derive(Clone)]
pub struct RolloutRecorder {
    path: PathBuf,
    mode: ThreadHistoryMode,
    tx: mpsc::UnboundedSender<RecorderCommand>,
}

impl RolloutRecorder {
    pub async fn open(path: PathBuf, mode: ThreadHistoryMode) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        let (tx, mut rx) = mpsc::unbounded_channel();
        tokio::spawn(async move {
            let mut file = file;
            while let Some(command) = rx.recv().await {
                match command {
                    RecorderCommand::Record { items, reply } => {
                        let result = async {
                            for item in items {
                                let line = serde_json::to_vec(&item)
                                    .map_err(std::io::Error::other)?;
                                file.write_all(&line).await?;
                                file.write_all(b"\n").await?;
                            }
                            Ok(())
                        }
                        .await;
                        let _ = reply.send(result);
                    }
                    RecorderCommand::Flush { reply } => {
                        let _ = reply.send(file.flush().await);
                    }
                    RecorderCommand::Shutdown { reply } => {
                        let result = file.flush().await;
                        let _ = reply.send(result);
                        break;
                    }
                }
            }
        });
        Ok(Self { path, mode, tx })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn record(&self, items: Vec<RolloutItem>) -> std::io::Result<()> {
        let items = items
            .into_iter()
            .filter(|item| is_persisted_rollout_item(item, self.mode))
            .collect();
        let (reply, recv) = oneshot::channel();
        self.tx
            .send(RecorderCommand::Record { items, reply })
            .map_err(|_| std::io::Error::other("rollout writer closed"))?;
        recv.await
            .map_err(|_| std::io::Error::other("rollout writer reply closed"))?
    }

    pub async fn flush(&self) -> std::io::Result<()> {
        let (reply, recv) = oneshot::channel();
        self.tx
            .send(RecorderCommand::Flush { reply })
            .map_err(|_| std::io::Error::other("rollout writer closed"))?;
        recv.await
            .map_err(|_| std::io::Error::other("rollout writer reply closed"))?
    }

    pub async fn shutdown(&self) -> std::io::Result<()> {
        let (reply, recv) = oneshot::channel();
        self.tx
            .send(RecorderCommand::Shutdown { reply })
            .map_err(|_| std::io::Error::other("rollout writer closed"))?;
        recv.await
            .map_err(|_| std::io::Error::other("rollout writer reply closed"))?
    }
}
```

Add `features = ["fs", "io-util", "macros", "rt"]` only if the workspace Tokio features are later narrowed; the current workspace already enables `full`.

- [x] **Step 4: Implement rollout reading**

Create `crates/agent-rollout/src/reconstruction.rs`:

```rust
use std::path::Path;

use tokio::io::{AsyncBufReadExt, BufReader};

use crate::RolloutItem;

pub async fn read_rollout(path: &Path) -> std::io::Result<Vec<RolloutItem>> {
    let file = tokio::fs::File::open(path).await?;
    let mut lines = BufReader::new(file).lines();
    let mut items = Vec::new();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        items.push(serde_json::from_str(&line).map_err(std::io::Error::other)?);
    }
    Ok(items)
}
```

- [x] **Step 5: Implement deterministic rollout creation and lookup**

Create `crates/agent-rollout/src/path.rs`:

```rust
use std::path::{Path, PathBuf};

pub fn new_rollout_path(root: &Path, thread_id: &str, now: chrono::DateTime<chrono::Utc>) -> PathBuf {
    root.join(now.format("%Y").to_string())
        .join(now.format("%m").to_string())
        .join(now.format("%d").to_string())
        .join(format!(
            "rollout-{}-{thread_id}.jsonl",
            now.format("%Y%m%dT%H%M%S%.3fZ")
        ))
}

pub fn find_rollout(root: &Path, thread_id: &str) -> std::io::Result<Option<PathBuf>> {
    let suffix = format!("-{thread_id}.jsonl");
    let mut matches = Vec::new();
    if !root.exists() {
        return Ok(None);
    }
    for year in std::fs::read_dir(root)? {
        let year = year?.path();
        if !year.is_dir() {
            continue;
        }
        for month in std::fs::read_dir(year)? {
            let month = month?.path();
            if !month.is_dir() {
                continue;
            }
            for day in std::fs::read_dir(month)? {
                let day = day?.path();
                if !day.is_dir() {
                    continue;
                }
                for file in std::fs::read_dir(day)? {
                    let path = file?.path();
                    if path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .is_some_and(|name| name.ends_with(&suffix))
                    {
                        matches.push(path);
                    }
                }
            }
        }
    }
    matches.sort();
    Ok(matches.pop())
}
```

Add tests that create two dated paths and assert `find_rollout` returns the newest lexical timestamp.

- [x] **Step 6: Run rollout tests**

Run:

```bash
cargo test -p agent-rollout
cargo check -p agent-rollout
```

Expected: PASS.

- [x] **Step 7: Commit**

```bash
git add crates/agent-rollout
git commit -m "feat(rollout): add ordered jsonl recorder"
```

## Batch B — Session actor and ordered Core events

### Task 4: Finish Session interior mutability

**Files:**
- Modify: `crates/agent-core/Cargo.toml`
- Modify: `crates/agent-core/src/runtime/mod.rs`
- Modify: `crates/agent-core/src/runtime/session_state.rs`
- Modify: `crates/agent-core/src/runtime/session_services.rs`
- Modify: `crates/agent-core/src/runtime/model_ctx.rs`
- Modify: `crates/agent-core/src/runtime/recording.rs`
- Modify: `crates/agent-core/src/runtime/context_maintenance.rs`
- Modify: `crates/agent-core/src/runtime/turn_lifecycle.rs`
- Test: unit tests in `crates/agent-core/src/runtime/mod.rs`

- [x] **Step 1: Add a compile-time Send/Sync test and concurrent history test**

Add to the existing `runtime::tests` module:

```rust
fn assert_send_sync<T: Send + Sync>() {}

#[test]
fn session_is_send_and_sync_without_an_outer_mutex() {
    assert_send_sync::<Session>();
}

#[tokio::test]
async fn arc_session_owns_concurrent_history_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let session = Arc::new(Session::new(test_config(&dir)).unwrap());
    let writer = Arc::clone(&session);
    tokio::spawn(async move {
        writer.record_items(vec![Message::user("first")]).await;
    })
    .await
    .unwrap();
    assert_eq!(session.clone_history().await[0].content_str(), "first");
}
```

- [x] **Step 2: Run the test and verify the current API fails**

Run:

```bash
cargo test -p agent session_is_send_and_sync_without_an_outer_mutex --lib
cargo test -p agent arc_session_owns_concurrent_history_snapshots --lib
```

Expected: FAIL until the execution-gate history patch is integrated and all shared services are Sync.

- [x] **Step 3: Add protocol and rollout dependencies**

Add to `crates/agent-core/Cargo.toml`:

```toml
agent-protocol = { path = "../agent-protocol" }
agent-rollout = { path = "../agent-rollout" }
async-channel = "2"
```

- [x] **Step 4: Make SessionState the owner of mutable conversation state**

The final `SessionState` fields must be:

```rust
pub(crate) struct SessionState {
    pub(crate) history: Vec<Message>,
    pub(crate) model_ctx: model_ctx::ModelContext,
    pub(crate) compression: compression_state::CompressionState,
    pub(crate) turn: turn_budget::TurnState,
    pub(crate) pending_inject_context: Option<String>,
    pub(crate) pending_learning_nudge: Option<String>,
    pub(crate) interaction_mode: types::InteractionMode,
    pub(crate) current_turn_context: Option<Arc<TurnContext>>,
    pub(crate) current_step_context: Option<Arc<StepContext>>,
    pub(crate) mcp_config_override: Vec<mcp::McpServerConfig>,
    pub(crate) mcp_instructions: Vec<mcp::McpServerInstructions>,
    pub(crate) project_root: Option<PathBuf>,
    pub(crate) permission_profile: Option<String>,
    pub(crate) skill_config_overrides: Vec<(PathBuf, bool)>,
}
```

Initialize these fields in `SessionState::new(history, project_root)`. Remove their duplicate mutable fields from `Session`.

- [x] **Step 5: Move mutable services behind focused locks**

Extend `SessionServices` with:

```rust
pub(crate) memory: tokio::sync::Mutex<MemoryManager>,
pub(crate) tool_registry: tokio::sync::RwLock<ToolRegistry>,
```

Change its constructor to accept both values. Keep `SharedConversationStore` behind `std::sync::Mutex`; no SQLite guard may cross an `.await`.

- [x] **Step 6: Convert recording and history APIs to `&self`**

The public signatures after this step must be:

```rust
pub async fn record_items(&self, items: Vec<Message>);
pub async fn clone_history(&self) -> Vec<Message>;
pub async fn replace_history(&self, history: Vec<Message>);
pub async fn record_assistant_message(&self, content: &str) -> anyhow::Result<()>;
pub async fn record_assistant_message_with_tools(
    &self,
    content: &str,
    tool_calls: Option<Vec<types::message::ToolCall>>,
    reasoning: Option<&str>,
    reasoning_details: Option<serde_json::Value>,
) -> anyhow::Result<()>;
pub async fn record_user_message(&self, content: &str) -> anyhow::Result<()>;
pub async fn record_tool_result_with_id(
    &self,
    tool_call_id: Option<&str>,
    tool_name: Option<&str>,
    content: &str,
) -> anyhow::Result<()>;
```

Update call sites in `streaming/summary.rs`, `streaming/maintenance.rs`, and `runtime/turn_lifecycle.rs` to await these methods.

- [x] **Step 7: Convert model/config reads to snapshots**

Add these methods to `Session` and use them instead of returning references tied to a state lock:

```rust
pub(crate) async fn model_context_snapshot(&self) -> model_ctx::ModelContext {
    self.state.lock().await.model_ctx.clone()
}

pub(crate) async fn project_root_snapshot(&self) -> Option<PathBuf> {
    self.state.lock().await.project_root.clone()
}

pub(crate) async fn permission_profile_snapshot(&self) -> Option<String> {
    self.state.lock().await.permission_profile.clone()
}
```

Derive `Clone` for `ModelContext`. Update `context_maintenance.rs`, `system_prompt.rs`, `tool_dispatch.rs`, and `turn_lifecycle.rs` so no state guard is held across provider, tool, MCP, or hook awaits.

- [x] **Step 8: Run focused and all-target checks**

Run:

```bash
cargo test -p agent runtime:: --lib
cargo check -p agent --all-targets
```

Expected: PASS and no `Arc<Mutex<Session>>` is needed for history/config safety.

- [x] **Step 9: Commit**

```bash
git add crates/agent-core/Cargo.toml crates/agent-core/src/runtime crates/agent-core/src/streaming/summary.rs crates/agent-core/src/streaming/maintenance.rs
git commit -m "refactor(agent): internalize mutable session state"
```

### Task 5: Add SessionIo and AstroThread

**Files:**
- Create: `crates/agent-core/src/runtime/session_io.rs`
- Create: `crates/agent-core/src/runtime/astro_thread.rs`
- Create: `crates/agent-core/src/runtime/submission_loop.rs`
- Modify: `crates/agent-core/src/runtime/mod.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Test: unit tests in the new modules

- [x] **Step 1: Write bounded submission and ordered receive tests**

Add to `session_io.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{EventMsg, Op, TurnStartedEvent};

    #[test]
    fn submission_channel_is_bounded_at_512() {
        let (tx, _rx) = submission_channel();
        for index in 0..SUBMISSION_CHANNEL_CAPACITY {
            tx.try_send(Submission {
                id: format!("submission-{index}"),
                op: Op::Interrupt,
            })
            .unwrap();
        }
        assert!(matches!(
            tx.try_send(Submission {
                id: "overflow".into(),
                op: Op::Interrupt,
            }),
            Err(async_channel::TrySendError::Full(_))
        ));
    }

    #[tokio::test]
    async fn event_receiver_preserves_send_order() {
        let (tx, rx) = event_channel();
        for id in ["one", "two"] {
            tx.send(Event {
                id: id.into(),
                msg: EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: id.into(),
                }),
            })
            .await
            .unwrap();
        }
        assert_eq!(rx.recv().await.unwrap().id, "one");
        assert_eq!(rx.recv().await.unwrap().id, "two");
    }
}
```

- [x] **Step 2: Run tests and verify failure**

Run `cargo test -p agent session_io::tests --lib`.

Expected: FAIL because `session_io` is not defined.

- [x] **Step 3: Implement SessionIo channels**

Create `session_io.rs`:

```rust
use agent_protocol::{Event, Op, Submission};
use tokio::sync::watch;
use uuid::Uuid;

pub(crate) const SUBMISSION_CHANNEL_CAPACITY: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentStatus {
    Idle,
    Running { turn_id: String },
    Errored(String),
    Shutdown,
}

pub(crate) fn submission_channel() -> (
    async_channel::Sender<Submission>,
    async_channel::Receiver<Submission>,
) {
    async_channel::bounded(SUBMISSION_CHANNEL_CAPACITY)
}

pub(crate) fn event_channel() -> (
    async_channel::Sender<Event>,
    async_channel::Receiver<Event>,
) {
    async_channel::unbounded()
}

pub struct SessionIo {
    pub(crate) tx_sub: async_channel::Sender<Submission>,
    pub(crate) rx_event: async_channel::Receiver<Event>,
    pub(crate) status_rx: watch::Receiver<AgentStatus>,
    pub(crate) termination_rx: watch::Receiver<bool>,
}

impl SessionIo {
    pub async fn submit(&self, op: Op) -> Result<String, async_channel::SendError<Submission>> {
        let id = Uuid::new_v4().to_string();
        self.tx_sub
            .send(Submission { id: id.clone(), op })
            .await?;
        Ok(id)
    }

    pub async fn next_event(&self) -> Result<Event, async_channel::RecvError> {
        self.rx_event.recv().await
    }

    pub fn status(&self) -> AgentStatus {
        self.status_rx.borrow().clone()
    }

    pub fn subscribe_status(&self) -> watch::Receiver<AgentStatus> {
        self.status_rx.clone()
    }

    pub async fn wait_terminated(&mut self) {
        while !*self.termination_rx.borrow() {
            if self.termination_rx.changed().await.is_err() {
                break;
            }
        }
    }
}
```

- [x] **Step 4: Implement AstroThread ownership**

Create `astro_thread.rs`:

```rust
use std::sync::Arc;

use agent_protocol::{Event, Op, TurnInputMode, TurnInputRequest, TurnInputSubmission};
use agent_rollout::RolloutRecorder;

use super::session_io::{event_channel, submission_channel, AgentStatus, SessionIo};
use super::submission_loop::submission_loop;
use super::Session;

pub struct AstroThread {
    session: Arc<Session>,
    io: SessionIo,
}

impl AstroThread {
    pub fn spawn(session: Arc<Session>, rollout: RolloutRecorder) -> Arc<Self> {
        let (tx_sub, rx_sub) = submission_channel();
        let (tx_event, rx_event) = event_channel();
        let (status_tx, status_rx) = tokio::sync::watch::channel(AgentStatus::Idle);
        let (termination_tx, termination_rx) = tokio::sync::watch::channel(false);
        session.bind_runtime_io(tx_event, status_tx, rollout);
        let session_for_loop = Arc::clone(&session);
        tokio::spawn(async move {
            submission_loop(session_for_loop, rx_sub).await;
            let _ = termination_tx.send(true);
        });
        Arc::new(Self {
            session,
            io: SessionIo {
                tx_sub,
                rx_event,
                status_rx,
                termination_rx,
            },
        })
    }

    pub async fn submit(&self, op: Op) -> anyhow::Result<String> {
        self.io.submit(op).await.map_err(anyhow::Error::from)
    }

    pub async fn submit_turn(
        &self,
        request: TurnInputRequest,
        mode: TurnInputMode,
    ) -> anyhow::Result<(String, TurnInputSubmission)> {
        let (reply, recv) = tokio::sync::oneshot::channel();
        let submission_id = self.submit(Op::TurnInput { request, mode, reply }).await?;
        let result = recv
            .await
            .map_err(|_| anyhow::anyhow!("turn input reply channel closed"))??;
        Ok((submission_id, result))
    }

    pub async fn next_event(&self) -> anyhow::Result<Event> {
        self.io.next_event().await.map_err(anyhow::Error::from)
    }

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    pub async fn flush_rollout(&self) -> std::io::Result<()> {
        self.session.flush_rollout().await
    }
}
```

Declare `pub(crate) mod session_io; pub(crate) mod submission_loop; mod astro_thread;` in `runtime/mod.rs`, re-export `AstroThread`, and re-export it from `lib.rs`.

- [x] **Step 5: Add the compiling submission-loop scaffold**

Create `submission_loop.rs` so this commit remains buildable before Task 6 adds dispatch:

```rust
use std::sync::Arc;

use agent_protocol::{Op, Submission};

use super::Session;

pub(crate) async fn submission_loop(
    _session: Arc<Session>,
    rx_sub: async_channel::Receiver<Submission>,
) {
    while let Ok(submission) = rx_sub.recv().await {
        if matches!(submission.op, Op::Shutdown) {
            break;
        }
    }
}
```

This scaffold is private and is replaced by the exhaustive dispatcher in Task 6. The `AstroThread` type is public so each intermediate commit compiles, but do not migrate production callers to it until Task 6 completes the dispatcher.

- [x] **Step 6: Add Session runtime I/O binding**

Add these fields to `Session`:

```rust
event_tx: std::sync::OnceLock<async_channel::Sender<agent_protocol::Event>>,
status_tx: std::sync::OnceLock<tokio::sync::watch::Sender<session_io::AgentStatus>>,
rollout: std::sync::OnceLock<agent_rollout::RolloutRecorder>,
```

Add:

```rust
pub(crate) fn bind_runtime_io(
    &self,
    event_tx: async_channel::Sender<agent_protocol::Event>,
    status_tx: tokio::sync::watch::Sender<session_io::AgentStatus>,
    rollout: agent_rollout::RolloutRecorder,
) {
    assert!(self.event_tx.set(event_tx).is_ok(), "event sender bound once");
    assert!(self.status_tx.set(status_tx).is_ok(), "status sender bound once");
    assert!(self.rollout.set(rollout).is_ok(), "rollout bound once");
}
```

Initialize all three fields with `OnceLock::new()` in every Session constructor.

- [x] **Step 7: Run tests**

Run:

```bash
cargo test -p agent session_io::tests --lib
cargo check -p agent --all-targets
```

Expected: PASS.

- [x] **Step 8: Commit**

```bash
git add crates/agent-core/src/runtime crates/agent-core/src/lib.rs crates/agent-core/Cargo.toml Cargo.lock
git commit -m "feat(agent): add bounded thread session io"
```

### Task 6: Make submission_loop the sole control dispatcher

**Files:**
- Modify: `crates/agent-core/src/runtime/submission_loop.rs`
- Modify: `crates/agent-core/src/tasks/mod.rs`
- Modify: `crates/agent-core/src/tasks/regular.rs`
- Modify: `crates/agent-core/src/runtime/turn_lifecycle.rs`
- Modify: `crates/agent-core/src/runtime/turn_context.rs`
- Test: unit tests in `submission_loop.rs` and `tasks/mod.rs`

- [x] **Step 1: Write a non-blocking dispatch test**

Add to `submission_loop.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use agent_protocol::{Op, Submission, TurnInput};
    use tokio::sync::Notify;
    use tokio_util::sync::CancellationToken;

    use crate::tasks::{SessionTask, SessionTaskResult, TaskKind};

    struct PendingTask {
        started: Arc<Notify>,
    }

    impl SessionTask for PendingTask {
        fn kind(&self) -> TaskKind {
            TaskKind::Regular
        }

        fn span_name(&self) -> &'static str {
            "session_task.submission_loop_test"
        }

        async fn run(
            self: Arc<Self>,
            _session: Arc<Session>,
            _ctx: Arc<TurnContext>,
            _input: Vec<TurnInput>,
            cancellation_token: CancellationToken,
        ) -> SessionTaskResult {
            self.started.notify_one();
            cancellation_token.cancelled().await;
            Ok(None)
        }
    }

    #[tokio::test]
    async fn interrupt_is_dispatched_while_a_turn_task_is_running() {
        let dir = tempfile::tempdir().unwrap();
        let session = Arc::new(
            Session::with_session_id(
                Config::with_defaults(dir.path().to_path_buf()),
                "submission-loop-test".into(),
            )
            .unwrap(),
        );
        let started = Arc::new(Notify::new());
        let context = session.create_turn_context("turn-1".into()).await;
        session
            .spawn_task(
                context,
                Vec::new(),
                PendingTask {
                    started: Arc::clone(&started),
                },
            )
            .await
            .unwrap();
        started.notified().await;

        let (tx, rx) = async_channel::bounded(4);
        let loop_task = tokio::spawn(submission_loop(Arc::clone(&session), rx));
        tx
            .send(Submission {
                id: "interrupt-1".into(),
                op: Op::Interrupt,
            })
            .await
            .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            loop {
                if session.active_turn.lock().await.is_none() {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        drop(tx);
        loop_task.await.unwrap();
    }
}
```

- [x] **Step 2: Run the test and verify it fails**

Run `cargo test -p agent interrupt_is_dispatched_while_a_turn_task_is_running --lib`.

Expected: FAIL because `submission_loop` and the asynchronous task handle are not implemented.

- [x] **Step 3: Change SessionTask to operate on Arc<Session>**

Replace both trait signatures with:

```rust
fn run(
    self: Arc<Self>,
    session: Arc<Session>,
    ctx: Arc<TurnContext>,
    input: Vec<TurnInput>,
    cancellation_token: CancellationToken,
) -> BoxFuture<'static, SessionTaskResult>;

fn abort<'a>(
    &'a self,
    session: Arc<Session>,
    ctx: Arc<TurnContext>,
) -> BoxFuture<'a, ()>;
```

Update `AnySessionTask`, `RegularTask`, and test tasks to match. Remove `Arc<Mutex<Session>>` from `tasks/`.

Delete the local `tasks::TurnInput` enum and import `agent_protocol::TurnInput` throughout Core. Keep `pub use agent_protocol::TurnInput` in `agent-core/src/lib.rs` for downstream source compatibility.

- [x] **Step 4: Spawn rather than await the task inside dispatch**

Add a `tokio::task::JoinHandle<()>` to `RunningTask`. Change `Session::spawn_task` to return after installing the task:

```rust
pub(crate) async fn spawn_task<T: SessionTask>(
    self: &Arc<Self>,
    turn_context: Arc<TurnContext>,
    input: Vec<TurnInput>,
    task: T,
) -> anyhow::Result<()> {
    self.abort_all_tasks(agent_protocol::TurnAbortReason::Replaced).await?;
    let task: Arc<dyn AnySessionTask> = Arc::new(task);
    let cancellation_token = CancellationToken::new();
    let done = Arc::new(Notify::new());
    let session = Arc::clone(self);
    let ctx = Arc::clone(&turn_context);
    let task_for_run = Arc::clone(&task);
    let child = cancellation_token.child_token();
    let cancellation_for_run = cancellation_token.clone();
    let done_for_run = Arc::clone(&done);
    let handle = tokio::spawn(async move {
        let result = task_for_run.run(Arc::clone(&session), Arc::clone(&ctx), input, child).await;
        if !cancellation_for_run.is_cancelled() {
            session.on_task_finished(ctx, result).await;
        }
        done_for_run.notify_waiters();
    });
    self.install_running_task(task, cancellation_token, turn_context, done, handle)
        .await
}
```

`install_running_task` must reject a second task and store the JoinHandle. `abort_all_tasks` must cancel, wait up to five seconds, abort the JoinHandle, call the task abort hook, emit one `TurnAborted`, and clear the active task.

- [x] **Step 5: Implement TurnInput start/steer/reject**

Add these helpers in `turn_lifecycle.rs`:

```rust
pub(crate) async fn submit_turn_input(
    self: &Arc<Self>,
    submission_id: String,
    request: agent_protocol::TurnInputRequest,
    mode: agent_protocol::TurnInputMode,
) -> Result<agent_protocol::TurnInputSubmission, agent_protocol::TurnInputError>;

async fn start_turn(
    self: &Arc<Self>,
    turn_id: String,
    input: Vec<agent_protocol::TurnInput>,
) -> Result<agent_protocol::TurnInputSubmission, agent_protocol::TurnInputError>;

async fn steer_turn(
    &self,
    expected_turn_id: Option<&str>,
    input: Vec<agent_protocol::TurnInput>,
) -> Result<agent_protocol::TurnInputSubmission, agent_protocol::TurnInputError>;
```

Behavior must be exhaustive:

```rust
match mode {
    TurnInputMode::StartOrSteer => match self.active_turn_id().await {
        Some(turn_id) => self.steer_turn(Some(&turn_id), request.input).await,
        None => self.start_turn(submission_id, request.input).await,
    },
    TurnInputMode::StartIfIdle => match self.active_turn_id().await {
        Some(_) => Ok(TurnInputSubmission::NotSubmitted {
            reason: "not_idle".into(),
        }),
        None => self.start_turn(submission_id, request.input).await,
    },
    TurnInputMode::Steer { expected_turn_id } => {
        self.steer_turn(Some(&expected_turn_id), request.input).await
    }
}
```

Convert protocol `TurnInput` into the existing task input at this boundary.

- [x] **Step 6: Implement the ordered submission loop**

Create `submission_loop.rs`:

```rust
use std::sync::Arc;

use agent_protocol::{Event, EventMsg, Op, Submission};

use super::Session;

pub(crate) async fn submission_loop(
    session: Arc<Session>,
    rx_sub: async_channel::Receiver<Submission>,
) {
    let mut shutdown_received = false;
    while let Ok(submission) = rx_sub.recv().await {
        let should_exit = match submission.op {
            Op::TurnInput { request, mode, reply } => {
                let result = session
                    .submit_turn_input(submission.id, request, mode)
                    .await;
                let _ = reply.send(result);
                false
            }
            Op::Interrupt => {
                let _ = session
                    .abort_all_tasks(agent_protocol::TurnAbortReason::Interrupted)
                    .await;
                false
            }
            Op::EmitExtension { item } => {
                session.record_extension(submission.id, item).await;
                false
            }
            Op::Shutdown => {
                session.shutdown_runtime().await;
                true
            }
            op => {
                session.dispatch_control_op(submission.id, op).await;
                false
            }
        };
        if should_exit {
            shutdown_received = true;
            break;
        }
    }
    if !shutdown_received {
        session.shutdown_runtime().await;
    }
}
```

At this stage `Shutdown` exits the loop; Task 7 replaces that arm with the full flush and `ShutdownComplete` sequence. Implement `shutdown_runtime` by moving the existing session-release cleanup (task cancellation, hook cleanup, MCP shutdown, terminal cleanup) behind an idempotent `&self` method.

`dispatch_control_op` must contain explicit match arms for every remaining `Op`; it may not silently ignore variants. Unsupported operations emit `EventMsg::Error` with `error_type = "unsupported_op"`.

- [x] **Step 7: Run lifecycle tests**

Run:

```bash
cargo test -p agent submission_loop:: --lib
cargo test -p agent tasks:: --lib
cargo check -p agent --all-targets
```

Expected: PASS; Interrupt is processed before the blocking task exits naturally.

- [x] **Step 8: Commit**

```bash
git add crates/agent-core/src/runtime crates/agent-core/src/tasks
git commit -m "refactor(agent): route session control through submissions"
```

### Task 7: Persist EventMsg before Core delivery and enforce one terminal

**Files:**
- Modify: `crates/agent-core/src/runtime/mod.rs`
- Modify: `crates/agent-core/src/runtime/session_io.rs`
- Modify: `crates/agent-core/src/tasks/mod.rs`
- Modify: `crates/agent-core/src/streaming/lifecycle.rs`
- Create: `crates/agent-core/tests/common/mod.rs`
- Test: `crates/agent-core/tests/thread_event_lifecycle_test.rs`

- [x] **Step 1: Write persistence-before-delivery tests**

Split the following code at the first `#[tokio::test]`: put the imports plus `new_thread` in `crates/agent-core/tests/common/mod.rs` (make `new_thread` `pub(crate)`), and put the tests in `thread_event_lifecycle_test.rs` with `mod common; use common::new_thread;`:

```rust
use std::{path::PathBuf, sync::Arc};

use agent::{AstroThread, Config, EventMsg, Session};
use agent_protocol::TurnStartedEvent;
use agent_rollout::{
    read_rollout, RolloutItem, RolloutRecorder, ThreadHistoryMode,
};
use tempfile::TempDir;

async fn new_thread() -> (
    TempDir,
    Arc<Session>,
    Arc<AstroThread>,
    RolloutRecorder,
    PathBuf,
) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    let recorder = RolloutRecorder::open(path.clone(), ThreadHistoryMode::Paginated)
        .await
        .unwrap();
    let recorder_control = recorder.clone();
    let session = Arc::new(
        Session::with_session_id(
            Config::with_defaults(dir.path().to_path_buf()),
            "thread-event-test".into(),
        )
        .unwrap(),
    );
    let thread = AstroThread::spawn(Arc::clone(&session), recorder);
    (dir, session, thread, recorder_control, path)
}

#[tokio::test]
async fn durable_event_is_in_rollout_when_receiver_observes_it() {
    let (_dir, session, thread, _recorder, rollout_path) = new_thread().await;
    session
        .send_event(
            "turn-1",
            EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: "turn-1".into(),
            }),
        )
        .await;
    let event = thread.next_event().await.unwrap();
    assert!(matches!(event.msg, EventMsg::TurnStarted(_)));
    let rollout = read_rollout(&rollout_path).await.unwrap();
    assert!(rollout.iter().any(|item| matches!(
        item,
        RolloutItem::EventMsg(EventMsg::TurnStarted(event)) if event.turn_id == "turn-1"
    )));
}

#[tokio::test]
async fn closed_rollout_writer_does_not_suppress_live_event() {
    let (_dir, session, thread, recorder, _path) = new_thread().await;
    recorder.shutdown().await.unwrap();
    session
        .send_event(
            "turn-1",
            EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: "turn-1".into(),
            }),
        )
        .await;
    let event = thread.next_event().await.unwrap();
    assert!(matches!(event.msg, EventMsg::TurnStarted(_)));
}
```

- [x] **Step 2: Write exactly-one-terminal tests**

Add the tests to the existing `#[cfg(test)]` module in `tasks/mod.rs`, where the private task lifecycle is directly accessible. Reuse the concrete `PendingTask` from Task 6 and add:

```rust
struct FailingTask;

impl SessionTask for FailingTask {
    fn kind(&self) -> TaskKind { TaskKind::Regular }
    fn span_name(&self) -> &'static str { "session_task.failing_test" }
    async fn run(
        self: Arc<Self>,
        _session: Arc<Session>,
        _ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        _cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        Err(anyhow::anyhow!("provider failed"))
    }
}

async fn task_test_thread(test_name: &str) -> (TempDir, Arc<Session>, Arc<AstroThread>) {
    let dir = tempfile::tempdir().unwrap();
    let rollout = RolloutRecorder::open(
        dir.path().join("rollout.jsonl"),
        ThreadHistoryMode::Paginated,
    )
    .await
    .unwrap();
    let session = Arc::new(
        Session::with_session_id(
            Config::with_defaults(dir.path().to_path_buf()),
            test_name.into(),
        )
        .unwrap(),
    );
    let thread = AstroThread::spawn(Arc::clone(&session), rollout);
    (dir, session, thread)
}

async fn collect_terminal(thread: &AstroThread, turn_id: &str) -> Vec<Event> {
    let mut events = Vec::new();
    loop {
        let event = thread.next_event().await.unwrap();
        if event.id != turn_id { continue; }
        let terminal = event.msg.is_terminal();
        events.push(event);
        if terminal { return events; }
    }
}

#[tokio::test]
async fn unexpected_error_emits_error_then_complete_with_error() {
    let (_dir, session, thread) = task_test_thread("task-failure-test").await;
    let turn_id = "turn-failure";
    let context = session.create_turn_context(turn_id.into()).await;
    session.spawn_task(context, Vec::new(), FailingTask).await.unwrap();
    let events = collect_terminal(&thread, turn_id).await;
    assert!(matches!(events[events.len() - 2].msg, EventMsg::Error(_)));
    assert!(matches!(
        events.last().unwrap().msg,
        EventMsg::TurnComplete(ref event) if event.error.is_some()
    ));
    assert_eq!(events.iter().filter(|event| event.msg.is_terminal()).count(), 1);
}

#[tokio::test]
async fn interrupt_emits_only_turn_aborted() {
    let (_dir, session, thread) = task_test_thread("task-interrupt-test").await;
    let turn_id = "turn-interrupt";
    let started = Arc::new(Notify::new());
    let context = session.create_turn_context(turn_id.into()).await;
    session
        .spawn_task(context, Vec::new(), PendingTask { started: Arc::clone(&started) })
        .await
        .unwrap();
    started.notified().await;
    thread.submit(Op::Interrupt).await.unwrap();
    let events = collect_terminal(&thread, turn_id).await;
    assert!(matches!(events.last().unwrap().msg, EventMsg::TurnAborted(_)));
    assert_eq!(events.iter().filter(|event| event.msg.is_terminal()).count(), 1);
}
```

Import `Event`, `Op`, `AstroThread`, `RolloutRecorder`, `ThreadHistoryMode`, and `TempDir` in that test module. Keep these helpers test-local; do not add a production `runtime::test_support` module.

- [x] **Step 3: Run the tests and verify failure**

Run:

```bash
cargo test -p agent --test thread_event_lifecycle_test
```

Expected: FAIL because Session has no unified `send_event` and task completion still uses legacy terminal items.

- [x] **Step 4: Implement send_event persistence ordering**

Add to `Session`:

```rust
pub async fn send_event(&self, turn_id: &str, msg: agent_protocol::EventMsg) {
    let event = agent_protocol::Event {
        id: turn_id.to_string(),
        msg,
    };
    self.send_event_raw_with_persistence(event, true).await;
}

pub(crate) async fn send_event_raw_with_persistence(
    &self,
    event: agent_protocol::Event,
    persist: bool,
) {
    if persist {
        if let Some(rollout) = self.rollout.get() {
            if let Err(error) = rollout
                .record(vec![agent_rollout::RolloutItem::EventMsg(event.msg.clone())])
                .await
            {
                tracing::warn!(%error, event_id = %event.id, "failed to persist event");
            }
        }
    }
    self.deliver_event_raw(event).await;
}

pub(crate) async fn deliver_event_raw(&self, event: agent_protocol::Event) {
    if let Some(status) = self.status_tx.get() {
        match &event.msg {
            agent_protocol::EventMsg::TurnStarted(started) => {
                let _ = status.send(session_io::AgentStatus::Running {
                    turn_id: started.turn_id.clone(),
                });
            }
            agent_protocol::EventMsg::TurnComplete(_)
            | agent_protocol::EventMsg::TurnAborted(_) => {
                let _ = status.send(session_io::AgentStatus::Idle);
            }
            agent_protocol::EventMsg::ShutdownComplete => {
                let _ = status.send(session_io::AgentStatus::Shutdown);
            }
            _ => {}
        }
    }
    if let Some(tx) = self.event_tx.get() {
        let _ = tx.send(event).await;
    }
}

pub async fn flush_rollout(&self) -> std::io::Result<()> {
    match self.rollout.get() {
        Some(rollout) => rollout.flush().await,
        None => Ok(()),
    }
}
```

- [x] **Step 5: Centralize task completion**

Define the cancellation marker next to the task lifecycle code, then implement `on_task_finished` exactly as this decision table:

```rust
#[derive(Debug, thiserror::Error)]
#[error("turn cancelled")]
struct TurnCancelled;

pub(crate) async fn on_task_finished(
    self: &Arc<Self>,
    turn_context: Arc<TurnContext>,
    result: SessionTaskResult,
) {
    let turn_id = turn_context.sub_id().to_string();
    if !self.take_active_task_if_matches(&turn_id).await {
        return;
    }
    match result {
        Ok(last_agent_message) => {
            self.send_event(
                &turn_id,
                EventMsg::TurnComplete(TurnCompleteEvent {
                    turn_id,
                    last_agent_message,
                    error: None,
                }),
            )
            .await;
        }
        Err(error) if error.downcast_ref::<TurnCancelled>().is_some() => {
            self.send_event(
                &turn_id,
                EventMsg::TurnAborted(TurnAbortedEvent {
                    turn_id: Some(turn_id),
                    reason: TurnAbortReason::Interrupted,
                }),
            )
            .await;
        }
        Err(error) => {
            let terminal_error = ErrorEvent {
                message: error.to_string(),
                error_type: "internal".into(),
            };
            self.send_event(&turn_id, EventMsg::Error(terminal_error.clone()))
                .await;
            self.send_event(
                &turn_id,
                EventMsg::TurnComplete(TurnCompleteEvent {
                    turn_id,
                    last_agent_message: None,
                    error: Some(terminal_error),
                }),
            )
            .await;
        }
    }
    let _ = self.flush_rollout().await;
}
```

Use a concrete private `TurnCancelled` error type. The explicit abort path must remove the task before emitting `TurnAborted`, so the spawned task cannot emit a second terminal event.

- [x] **Step 6: Implement shutdown ordering**

Add:

```rust
pub(crate) async fn shutdown(self: &Arc<Self>, submission_id: String) {
    let _ = self
        .abort_all_tasks(agent_protocol::TurnAbortReason::Interrupted)
        .await;
    self.shutdown_runtime().await;
    if let Some(rollout) = self.rollout.get() {
        if let Err(error) = rollout.shutdown().await {
            self.send_event_raw_with_persistence(
                agent_protocol::Event {
                    id: submission_id.clone(),
                    msg: EventMsg::Error(ErrorEvent {
                        message: error.to_string(),
                        error_type: "rollout_shutdown".into(),
                    }),
                },
                false,
            )
            .await;
        }
    }
    self.deliver_event_raw(agent_protocol::Event {
        id: submission_id,
        msg: EventMsg::ShutdownComplete,
    })
    .await;
}
```

Replace the Task 6 `Op::Shutdown` arm with:

```rust
Op::Shutdown => {
    session.shutdown(submission.id).await;
    true
}
```

- [x] **Step 7: Run tests**

Run:

```bash
cargo test -p agent --test thread_event_lifecycle_test
cargo test -p agent tasks:: --lib
```

Expected: PASS and each Turn has exactly one terminal event.

- [x] **Step 8: Commit**

```bash
git add crates/agent-core/src/runtime crates/agent-core/src/tasks crates/agent-core/tests/thread_event_lifecycle_test.rs
git commit -m "feat(agent): persist and deliver unified turn events"
```

### Task 8: Map streaming, tools, HITL, hooks, and background execution into EventMsg

**Files:**
- Modify: `crates/agent-core/src/streaming/lifecycle.rs`
- Modify: `crates/agent-core/src/streaming/multi_turn.rs`
- Modify: `crates/agent-core/src/streaming/summary.rs`
- Modify: `crates/agent-core/src/streaming/maintenance.rs`
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`
- Modify: `crates/agent-core/src/streaming/hitl_bridge.rs`
- Modify: `crates/agent-core/src/tasks/regular.rs`
- Modify: `crates/agent-core/src/exec/background.rs`
- Modify: `crates/agent-core/tests/common/mod.rs`
- Test: `crates/agent-core/tests/streaming_test.rs`
- Test: unit tests in `crates/agent-core/src/exec/background.rs`

- [x] **Step 1: Add EventMsg granularity tests**

Retain `run_multi_turn_stream_with_chat_fn` as the existing public, `#[doc(hidden)]` integration-test seam, but change it to accept `Arc<Session>`, `Arc<TurnContext>`, and a `ChatOverride`; it must invoke the same inner engine as `RegularTask`. Add this concrete test to `streaming_test.rs`, using the existing `scripted_chat` helper and the `new_thread()` helper from Task 7 (move `new_thread` to a private `tests/common/mod.rs` shared only by integration tests):

```rust
#[tokio::test]
async fn scripted_tool_turn_emits_item_lifecycle_and_one_terminal() {
    let (_dir, session, thread, _recorder, _path) = common::new_thread().await;
    let turn_id = "scripted-tool-turn";
    let turn_context = session.create_turn_context(turn_id.into()).await;
    let chat = scripted_chat(vec![
        vec![
            StreamChunk::ToolCallStart {
                index: 0,
                id: "call-1".into(),
                name: "terminal".into(),
            },
            StreamChunk::ToolCallDelta {
                index: 0,
                arguments: r#"{"command":"pwd"}"#.into(),
            },
            StreamChunk::Done { finish_reason: "tool_calls".into() },
        ],
        vec![
            StreamChunk::Text("done".into()),
            StreamChunk::Done { finish_reason: "stop".into() },
        ],
    ]);
    let run = tokio::spawn(run_multi_turn_stream_with_chat_fn(
        Arc::clone(&session),
        turn_context,
        vec![TurnInput::UserInput {
            content: "run pwd".into(),
            image_data_urls: Vec::new(),
        }],
        chat,
    ));
    let events = common::collect_through_terminal(&thread, turn_id).await;
    run.await.unwrap().unwrap();
    assert!(events.iter().any(|event| matches!(
        &event.msg,
        EventMsg::ItemStarted(item) if item.item.id() == "call-1"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.msg,
        EventMsg::ItemCompleted(item) if item.item.id() == "call-1"
    )));
    assert!(events.iter().any(|event| matches!(
        &event.msg,
        EventMsg::AgentMessageContentDelta(delta) if delta.delta == "done"
    )));
    assert_eq!(events.iter().filter(|event| event.msg.is_terminal()).count(), 1);
}
```

Append this helper to `crates/agent-core/tests/common/mod.rs` and import the module via `mod common;`:

```rust
pub(crate) async fn collect_through_terminal(
    thread: &AstroThread,
    turn_id: &str,
) -> Vec<agent_protocol::Event> {
    let mut events = Vec::new();
    loop {
        let event = thread.next_event().await.unwrap();
        if event.id != turn_id {
            continue;
        }
        let terminal = event.msg.is_terminal();
        events.push(event);
        if terminal {
            return events;
        }
    }
}
```

Change the background collector to consume `async_channel::Receiver<Event>` and return this private structure:

```rust
struct BackgroundCollected {
    usage: Usage,
    event_kinds: Vec<&'static str>,
    terminal_kind: &'static str,
}
```

Add a background collector unit test with a real channel, not a server fixture:

```rust
#[tokio::test]
async fn background_collector_observes_tool_and_terminal_events() {
    let (tx, rx) = async_channel::unbounded();
    let item = TurnItem::CommandExecution(CommandExecutionItem {
        id: "call-1".into(),
        command: "pwd".into(),
        output: None,
        exit_code: None,
    });
    for msg in [
        EventMsg::ItemStarted(ItemEvent { turn_id: "turn-1".into(), item: item.clone() }),
        EventMsg::ItemCompleted(ItemEvent { turn_id: "turn-1".into(), item }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".into(),
            last_agent_message: Some("done".into()),
            error: None,
        }),
    ] {
        tx.send(Event { id: "turn-1".into(), msg }).await.unwrap();
    }
    drop(tx);
    let result = collect_background_events(rx, "turn-1").await.unwrap();
    assert!(result.event_kinds.contains(&"item_started"));
    assert!(result.event_kinds.contains(&"item_completed"));
    assert_eq!(result.terminal_kind, "turn_complete");
}
```

- [x] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p agent scripted_tool_turn_emits_item_lifecycle_and_one_terminal --test streaming_test
cargo test -p agent background_collector_observes_tool_and_terminal_events --lib
```

Expected: FAIL because the loop still emits `MultiTurnStreamItem` and the background collector discards lifecycle events.

- [x] **Step 3: Replace the streaming emitter**

Replace `streaming::lifecycle::emit` with:

```rust
pub(crate) async fn emit(
    session: &Session,
    turn_context: &TurnContext,
    msg: agent_protocol::EventMsg,
) {
    session.send_event(turn_context.sub_id(), msg).await;
}
```

Remove the per-run `mpsc::Sender<MultiTurnStreamItem>` from `RunTurnArgs`. `RegularTask` must call the existing model/tool loop with `Arc<Session>`, `Arc<TurnContext>`, and `CancellationToken` only; provider targets and config come from the Session/StepContext snapshot. `run_multi_turn_stream_with_chat_fn` builds a `ProviderStreamer::with_chat_override` and then calls that same inner function; it may not contain a second loop implementation.

- [x] **Step 4: Apply the exact legacy-to-EventMsg mapping**

Replace each old emission using this table:

| Old item | New EventMsg |
|---|---|
| `RunStarted` | `TurnStarted` |
| assistant text token | `AgentMessageContentDelta` |
| reasoning token | `ReasoningContentDelta` |
| tool-call argument delta | `DynamicToolCallRequest` or the tool-specific delta payload |
| `ToolStarted` | `ItemStarted(TurnItem::DynamicToolCall/McpToolCall/CommandExecution)` |
| command stdout | `ExecCommandOutputDelta` |
| `ToolResult` | `ItemCompleted` with final output and status |
| `MemoryUpdate` | `ItemCompleted(TurnItem::Extension { namespace: "astro.memory" })` |
| `ContextUsage` | `TokenCount` |
| hook UI event | `HookStarted` / `HookCompleted` |
| HITL confirmation | approval/request event with stable request id |
| Subagent activity | `SubAgentActivity` |
| `RunFinished` | no direct emitter; task lifecycle emits the terminal event |
| `Error` | `Error`, followed by task-owned terminal event |
| `Done` | removed |

Use one stable `item_id` for begin/delta/completed events. Tool calls use provider `call_id`; assistant messages use a UUID allocated before their first delta.

- [x] **Step 5: Record completed assistant and tool items before the next action**

Immediately after recording an assistant response, emit:

```rust
session
    .send_event(
        turn_context.sub_id(),
        EventMsg::ItemCompleted(ItemEvent {
            turn_id: turn_context.sub_id().to_string(),
            item: TurnItem::AgentMessage(TextItem {
                id: assistant_item_id,
                content: assistant_text.clone(),
            }),
        }),
    )
    .await;
```

For tool calls, emit `ItemStarted` before dispatch and `ItemCompleted` after `record_tool_result_with_id(...).await` succeeds. This preserves the existing “assistant with tool_calls before tool execution” invariant while making rollout authoritative.

- [x] **Step 6: Replace background event dropping with a complete collector**

Replace `collect_background_events` with a collector over `AstroThread::next_event()`:

```rust
async fn collect_background_events(
    thread: &AstroThread,
    turn_id: &str,
) -> anyhow::Result<BackgroundTurnResult> {
    let mut usage = providers::Usage::default();
    let mut event_kinds = Vec::new();
    loop {
        let event = thread.next_event().await?;
        if event.id != turn_id {
            continue;
        }
        match event.msg {
            EventMsg::ItemStarted(_) => event_kinds.push("item_started".into()),
            EventMsg::ItemCompleted(_) => event_kinds.push("item_completed".into()),
            EventMsg::TokenCount(tokens) => {
                usage.input_tokens = u32::try_from(tokens.input_tokens).unwrap_or(u32::MAX);
                usage.output_tokens = u32::try_from(tokens.output_tokens).unwrap_or(u32::MAX);
            }
            EventMsg::TurnComplete(event) => {
                if let Some(error) = event.error {
                    anyhow::bail!(error.message);
                }
                return Ok(BackgroundTurnResult {
                    usage,
                    event_kinds,
                    terminal_kind: "turn_complete".into(),
                });
            }
            EventMsg::TurnAborted(event) => {
                anyhow::bail!("background turn aborted: {:?}", event.reason);
            }
            EventMsg::Error(_)
            | EventMsg::Warning(_)
            | EventMsg::StreamError(_)
            | EventMsg::TurnStarted(_)
            | EventMsg::AgentMessageContentDelta(_)
            | EventMsg::PlanDelta(_)
            | EventMsg::ReasoningContentDelta(_)
            | EventMsg::ExecCommandOutputDelta(_)
            | EventMsg::PatchApplyUpdated(_)
            | EventMsg::ExecApprovalRequest(_)
            | EventMsg::ApplyPatchApprovalRequest(_)
            | EventMsg::RequestPermissions(_)
            | EventMsg::RequestUserInput(_)
            | EventMsg::ElicitationRequest(_)
            | EventMsg::DynamicToolCallRequest(_)
            | EventMsg::DynamicToolCallResponse(_)
            | EventMsg::McpToolCallBegin(_)
            | EventMsg::McpToolCallEnd(_)
            | EventMsg::HookStarted(_)
            | EventMsg::HookCompleted(_)
            | EventMsg::SubAgentActivity(_)
            | EventMsg::ContextCompacted(_)
            | EventMsg::LegacyUserMessage(_)
            | EventMsg::LegacyAgentMessage(_)
            | EventMsg::LegacyReasoning(_)
            | EventMsg::LegacyMcpToolCallEnd(_)
            | EventMsg::LegacyPatchApplyEnd(_)
            | EventMsg::LegacyContextCompacted(_)
            | EventMsg::LegacySubAgentActivity(_)
            | EventMsg::ThreadSettingsApplied(_)
            | EventMsg::ThreadRolledBack(_)
            | EventMsg::ShutdownComplete => {}
        }
    }
}
```

Keep cache/read/write/reasoning counters from the existing runtime usage accumulator; the `TokenCount` event supplies the input/output totals shown above.

- [x] **Step 7: Run Core streaming and background tests**

Run:

```bash
cargo test -p agent --test streaming_test
cargo test -p agent exec::background::tests --lib
cargo check -p agent --all-targets
```

Expected: PASS and no Core execution path emits `Done` or `RunFinished`.

- [x] **Step 8: Commit**

```bash
git add crates/agent-core/src/streaming crates/agent-core/src/tasks crates/agent-core/src/exec/background.rs crates/agent-core/tests/streaming_test.rs
git commit -m "refactor(agent): emit unified foreground and background events"
```

## Batch C — app-server listener, subscribers, and resume

### Task 9: Add the gRPC Thread protocol

**Files:**
- Modify: `crates/agent-proto/proto/astro.proto`
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Test: generated types via `cargo check -p proto`

- [x] **Step 1: Add RPC signatures**

Add to `service AstroService` after `ChatControl`:

```protobuf
  // One long-lived outbound stream per client connection.
  rpc SubscribeThreadEvents(SubscribeThreadEventsRequest)
      returns (stream ThreadEvent);
  // Submit one user input operation to a loaded or newly created Thread.
  rpc SubmitTurn(SubmitTurnRequest) returns (SubmitTurnResponse);
  // Rebuild durable history, merge an active snapshot, and subscribe the connection.
  rpc ResumeThread(ResumeThreadRequest) returns (ResumeThreadResponse);
  // Stop routing one Thread to a connection without stopping the Thread.
  rpc UnsubscribeThread(UnsubscribeThreadRequest) returns (Empty);
```

- [x] **Step 2: Add request, snapshot, item, and event messages**

Add this protocol block near the existing Chat messages:

```protobuf
message SubscribeThreadEventsRequest {
  string connection_id = 1;
}

message SubmitTurnRequest {
  string connection_id = 1;
  ChatRequest chat = 2;
  string mode = 3;             // start_or_steer | start_if_idle | steer
  string expected_turn_id = 4; // required only for steer
}

message SubmitTurnResponse {
  string submission_id = 1;
  string turn_id = 2;
  string disposition = 3; // started | steered | not_submitted
  string reason = 4;
}

message ResumeThreadRequest {
  string connection_id = 1;
  string thread_id = 2;
  bool include_turns = 3;
}

message ResumeThreadResponse {
  ThreadSnapshot thread = 1;
}

message UnsubscribeThreadRequest {
  string connection_id = 1;
  string thread_id = 2;
}

message ThreadSnapshot {
  string thread_id = 1;
  string status = 2; // idle | running | errored | shutdown
  repeated ThreadTurn turns = 3;
  ThreadTurn active_turn = 4;
  bool has_active_turn = 5;
}

message ThreadTurn {
  string id = 1;
  string status = 2; // in_progress | completed | failed | aborted
  repeated ThreadItem items = 3;
  string last_agent_message = 4;
  ThreadError error = 5;
  bool has_error = 6;
}

message ThreadItem {
  string id = 1;
  string item_type = 2;
  string status = 3; // in_progress | completed | failed
  string payload_json = 4;
}

message ThreadError {
  string message = 1;
  string error_type = 2;
}

message ThreadEvent {
  string thread_id = 1;
  string turn_id = 2;
  oneof payload {
    ThreadTurnStarted turn_started = 10;
    ThreadItemEvent item_started = 11;
    ThreadItemEvent item_completed = 12;
    ThreadDelta agent_message_delta = 13;
    ThreadDelta plan_delta = 14;
    ThreadDelta reasoning_delta = 15;
    ThreadDelta exec_output_delta = 16;
    ThreadDelta patch_delta = 17;
    ThreadControlRequest control_request = 18;
    ThreadTokenCount token_count = 19;
    ThreadError error = 20;
    ThreadError warning = 21;
    ThreadTurnComplete turn_complete = 22;
    ThreadTurnAborted turn_aborted = 23;
    ThreadExtension extension = 24;
    bool shutdown_complete = 25;
  }
}

message ThreadTurnStarted {
  string turn_id = 1;
}

message ThreadItemEvent {
  ThreadItem item = 1;
}

message ThreadDelta {
  string item_id = 1;
  string delta = 2;
}

message ThreadControlRequest {
  string kind = 1;
  string item_id = 2;
  string request_id = 3;
  string payload_json = 4;
}

message ThreadTokenCount {
  uint64 input_tokens = 1;
  uint64 output_tokens = 2;
  uint64 total_tokens = 3;
}

message ThreadTurnComplete {
  string last_agent_message = 1;
  ThreadError error = 2;
  bool has_error = 3;
}

message ThreadTurnAborted {
  string reason = 1;
}

message ThreadExtension {
  string item_id = 1;
  string namespace = 2;
  string payload_json = 3;
}
```

The new protocol intentionally has no `event_id`, `stream_id`, or `after_event_id`; recovery is snapshot based.

- [x] **Step 3: Add temporary compiling RPC stubs**

In the `impl AstroService for AstroServiceImpl` block, add:

```rust
type SubscribeThreadEventsStream =
    Pin<Box<dyn futures::Stream<Item = Result<proto::ThreadEvent, Status>> + Send>>;

async fn subscribe_thread_events(
    &self,
    _request: Request<proto::SubscribeThreadEventsRequest>,
) -> Result<Response<Self::SubscribeThreadEventsStream>, Status> {
    Err(Status::unimplemented("thread event transport lands in Task 12"))
}

async fn submit_turn(
    &self,
    _request: Request<proto::SubmitTurnRequest>,
) -> Result<Response<proto::SubmitTurnResponse>, Status> {
    Err(Status::unimplemented("thread submission lands in Task 12"))
}

async fn resume_thread(
    &self,
    _request: Request<proto::ResumeThreadRequest>,
) -> Result<Response<proto::ResumeThreadResponse>, Status> {
    Err(Status::unimplemented("thread resume lands in Task 12"))
}

async fn unsubscribe_thread(
    &self,
    _request: Request<proto::UnsubscribeThreadRequest>,
) -> Result<Response<proto::Empty>, Status> {
    Err(Status::unimplemented("thread unsubscribe lands in Task 12"))
}
```

These stubs keep every intermediate commit buildable and are deleted when Task 12 delegates to `grpc::thread_service`.

- [x] **Step 4: Regenerate and compile**

Run:

```bash
cargo check -p proto
cargo check -p server
```

Expected: both commands PASS. Do not hand-edit generated files.

- [x] **Step 5: Commit**

```bash
git add crates/agent-proto/proto/astro.proto crates/agent-server/src/grpc/astro_service.rs
git commit -m "feat(proto): add thread submit resume and event APIs"
```

### Task 10: Implement ThreadHistoryBuilder and subscription state

**Files:**
- Modify: `crates/agent-server/Cargo.toml`
- Create: `crates/agent-server/src/thread_state.rs`
- Modify: `crates/agent-server/src/lib.rs`
- Test: unit tests in `thread_state.rs`

- [x] **Step 1: Write active snapshot reconstruction tests**

Add to `thread_state.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_protocol::{
        Event, EventMsg, ItemEvent, TextItem, TurnCompleteEvent, TurnItem, TurnStartedEvent,
    };

    #[test]
    fn builder_tracks_active_item_then_completes_turn() {
        let mut builder = ThreadHistoryBuilder::default();
        builder.track(&Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnStarted(TurnStartedEvent {
                turn_id: "turn-1".into(),
            }),
        });
        builder.track(&Event {
            id: "turn-1".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "turn-1".into(),
                item: TurnItem::AgentMessage(TextItem {
                    id: "item-1".into(),
                    content: "done".into(),
                }),
            }),
        });
        assert_eq!(builder.active_turn_snapshot().unwrap().items.len(), 1);
        builder.track(&Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: "turn-1".into(),
                last_agent_message: Some("done".into()),
                error: None,
            }),
        });
        assert!(builder.active_turn_snapshot().is_none());
        assert_eq!(builder.completed_turns().len(), 1);
    }
}
```

- [x] **Step 2: Run the test and verify failure**

Run `cargo test -p server thread_state::tests --lib`.

Expected: FAIL because `ThreadHistoryBuilder` is not defined.

- [x] **Step 3: Add server dependencies**

Add to `crates/agent-server/Cargo.toml`:

```toml
agent-protocol = { path = "../agent-protocol" }
agent-rollout = { path = "../agent-rollout" }
tokio-util = { workspace = true }
```

- [x] **Step 4: Implement snapshots and the history builder**

Create `thread_state.rs` with:

```rust
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use agent_protocol::{Event, EventMsg, TurnItem};
use tokio::sync::{mpsc, oneshot, Mutex, RwLock};

pub type ConnectionId = String;

#[derive(Debug, Clone, PartialEq)]
pub struct ItemSnapshot {
    pub id: String,
    pub status: String,
    pub item: TurnItem,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TurnSnapshot {
    pub id: String,
    pub status: String,
    pub items: Vec<ItemSnapshot>,
    pub last_agent_message: Option<String>,
    pub error: Option<agent_protocol::ErrorEvent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ThreadSnapshot {
    pub thread_id: String,
    pub status: String,
    pub turns: Vec<TurnSnapshot>,
    pub active_turn: Option<TurnSnapshot>,
}

#[derive(Default)]
pub struct ThreadHistoryBuilder {
    active: Option<TurnSnapshot>,
    completed: Vec<TurnSnapshot>,
}

impl ThreadHistoryBuilder {
    pub fn track(&mut self, event: &Event) {
        match &event.msg {
            EventMsg::TurnStarted(started) => {
                self.active = Some(TurnSnapshot {
                    id: started.turn_id.clone(),
                    status: "in_progress".into(),
                    items: Vec::new(),
                    last_agent_message: None,
                    error: None,
                });
            }
            EventMsg::ItemStarted(item) => self.upsert_item(&item.item, "in_progress"),
            EventMsg::ItemCompleted(item) => self.upsert_item(&item.item, "completed"),
            EventMsg::TurnComplete(completed) => {
                if let Some(mut turn) = self.active.take() {
                    turn.status = if completed.error.is_some() {
                        "failed".into()
                    } else {
                        "completed".into()
                    };
                    turn.last_agent_message = completed.last_agent_message.clone();
                    turn.error = completed.error.clone();
                    self.completed.push(turn);
                }
            }
            EventMsg::TurnAborted(_) => {
                if let Some(mut turn) = self.active.take() {
                    turn.status = "aborted".into();
                    self.completed.push(turn);
                }
            }
            _ => {}
        }
    }

    fn upsert_item(&mut self, item: &TurnItem, status: &str) {
        let Some(turn) = self.active.as_mut() else {
            return;
        };
        if let Some(existing) = turn.items.iter_mut().find(|entry| entry.id == item.id()) {
            existing.status = status.into();
            existing.item = item.clone();
        } else {
            turn.items.push(ItemSnapshot {
                id: item.id().into(),
                status: status.into(),
                item: item.clone(),
            });
        }
    }

    pub fn active_turn_snapshot(&self) -> Option<TurnSnapshot> {
        self.active.clone()
    }

    pub fn completed_turns(&self) -> &[TurnSnapshot] {
        &self.completed
    }
}

pub enum ListenerCommand {
    CoreEvent(Event),
    Resume {
        connection_id: ConnectionId,
        include_turns: bool,
        reply: oneshot::Sender<ThreadSnapshot>,
    },
    Unsubscribe {
        connection_id: ConnectionId,
    },
    Stop,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadActivity {
    pub status: String,
    pub has_subscribers: bool,
}

pub struct ThreadState {
    pub status: String,
    pub history: ThreadHistoryBuilder,
    pub subscribers: HashSet<ConnectionId>,
    pub listener_command_tx: mpsc::UnboundedSender<ListenerCommand>,
    pub activity_tx: tokio::sync::watch::Sender<ThreadActivity>,
}

#[derive(Clone, Default)]
pub struct ThreadStateManager {
    states: Arc<RwLock<HashMap<String, Arc<Mutex<ThreadState>>>>>,
}
```

Implement `insert`, `get`, `remove`, `subscribed_connection_ids`, `unsubscribe`, and `has_subscribers` with short RwLock scopes. `CoreEvent`, `Resume`, and `Unsubscribe` are executed by the serialized listener in Task 12, never directly by an RPC.

- [x] **Step 5: Run tests**

Run:

```bash
cargo test -p server thread_state::tests --lib
cargo check -p server --all-targets
```

Expected: PASS with the Task 9 RPC stubs still present.

- [x] **Step 6: Commit**

```bash
git add crates/agent-server/Cargo.toml crates/agent-server/src/thread_state.rs crates/agent-server/src/lib.rs Cargo.lock
git commit -m "feat(server): track thread history and subscriptions"
```

### Task 11: Add capacity-128 connection transport and slow-consumer isolation

**Files:**
- Create: `crates/agent-server/src/transport.rs`
- Modify: `crates/agent-server/src/lib.rs`
- Test: unit tests in `transport.rs`

- [x] **Step 1: Write the slow-consumer test**

Add to `transport.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn event(thread_id: &str, turn_id: &str) -> proto::ThreadEvent {
        proto::ThreadEvent {
            thread_id: thread_id.into(),
            turn_id: turn_id.into(),
            payload: Some(proto::thread_event::Payload::TurnStarted(
                proto::ThreadTurnStarted {
                    turn_id: turn_id.into(),
                },
            )),
        }
    }

    #[tokio::test]
    async fn slow_connection_does_not_block_fast_connection() {
        let registry = ConnectionRegistry::with_capacity(1);
        let (_slow_rx, slow_cancel) = registry.register("slow".into()).await;
        let (mut fast_rx, fast_cancel) = registry.register("fast".into()).await;
        registry.send_to("slow", event("thread", "first")).await;
        registry.send_to("slow", event("thread", "overflow")).await;
        registry.send_to("fast", event("thread", "first")).await;
        assert!(slow_cancel.is_cancelled());
        assert!(!fast_cancel.is_cancelled());
        assert_eq!(fast_rx.recv().await.unwrap().turn_id, "first");
    }
}
```

- [x] **Step 2: Run the test and verify failure**

Run `cargo test -p server slow_connection_does_not_block_fast_connection --lib`.

Expected: FAIL because `ConnectionRegistry` is not defined.

- [x] **Step 3: Implement the registry**

Create `transport.rs`:

```rust
use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

pub const CHANNEL_CAPACITY: usize = 128;

struct ConnectionEntry {
    tx: mpsc::Sender<proto::ThreadEvent>,
    cancel: CancellationToken,
}

#[derive(Clone)]
pub struct ConnectionRegistry {
    capacity: usize,
    entries: Arc<RwLock<HashMap<String, ConnectionEntry>>>,
}

impl Default for ConnectionRegistry {
    fn default() -> Self {
        Self::with_capacity(CHANNEL_CAPACITY)
    }
}

impl ConnectionRegistry {
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity,
            entries: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(
        &self,
        connection_id: String,
    ) -> (
        mpsc::Receiver<proto::ThreadEvent>,
        CancellationToken,
    ) {
        let (tx, rx) = mpsc::channel(self.capacity);
        let cancel = CancellationToken::new();
        let replaced = self.entries.write().await.insert(
            connection_id,
            ConnectionEntry {
                tx,
                cancel: cancel.clone(),
            },
        );
        if let Some(replaced) = replaced {
            replaced.cancel.cancel();
        }
        (rx, cancel)
    }

    pub async fn send_to(&self, connection_id: &str, event: proto::ThreadEvent) -> bool {
        let result = {
            let entries = self.entries.read().await;
            let Some(entry) = entries.get(connection_id) else {
                return false;
            };
            entry.tx.try_send(event)
        };
        match result {
            Ok(()) => true,
            Err(mpsc::error::TrySendError::Full(_)) => {
                if let Some(entry) = self.entries.write().await.remove(connection_id) {
                    entry.cancel.cancel();
                }
                false
            }
            Err(mpsc::error::TrySendError::Closed(_)) => {
                self.entries.write().await.remove(connection_id);
                false
            }
        }
    }

    pub async fn remove(&self, connection_id: &str) {
        if let Some(entry) = self.entries.write().await.remove(connection_id) {
            entry.cancel.cancel();
        }
    }
}
```

- [x] **Step 4: Run tests**

Run:

```bash
cargo test -p server transport::tests --lib
cargo check -p server --all-targets
```

Expected: PASS; fast receiver gets its event after slow receiver is disconnected.

- [x] **Step 5: Commit**

```bash
git add crates/agent-server/src/transport.rs crates/agent-server/src/lib.rs
git commit -m "feat(server): isolate slow thread-event consumers"
```

### Task 12: Implement the single listener, resume serialization, and Thread RPCs

**Files:**
- Create: `crates/agent-server/src/thread_listener.rs`
- Create: `crates/agent-server/src/thread_manager.rs`
- Create: `crates/agent-server/src/grpc/thread_service.rs`
- Modify: `crates/agent-server/src/grpc/mod.rs`
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Modify: `crates/agent-server/src/lib.rs`
- Test: `crates/agent-server/tests/thread_events_test.rs`

- [x] **Step 1: Write multi-subscriber and resume-gap integration tests**

Create `crates/agent-server/tests/thread_events_test.rs`:

```rust
use std::sync::Arc;

use agent_protocol::{Event, EventMsg, TurnCompleteEvent, TurnStartedEvent};
use server::{
    run_listener_commands, ConnectionRegistry, ListenerCommand, ThreadHistoryBuilder,
    ThreadState,
};
use tokio::sync::{mpsc, oneshot, Mutex};

async fn start_listener() -> (
    ConnectionRegistry,
    mpsc::UnboundedSender<ListenerCommand>,
) {
    let connections = ConnectionRegistry::default();
    let (commands, command_rx) = mpsc::unbounded_channel();
    let (activity_tx, _activity_rx) = tokio::sync::watch::channel(ThreadActivity {
        status: "idle".into(),
        has_subscribers: false,
    });
    let state = Arc::new(Mutex::new(ThreadState {
        status: "idle".into(),
        history: ThreadHistoryBuilder::default(),
        subscribers: Default::default(),
        listener_command_tx: commands.clone(),
        activity_tx,
    }));
    tokio::spawn(run_listener_commands(
        "thread-1".into(),
        state,
        command_rx,
        connections.clone(),
    ));
    (connections, commands)
}

async fn resume(
    commands: &mpsc::UnboundedSender<ListenerCommand>,
    connection_id: &str,
    include_turns: bool,
) -> server::ThreadSnapshot {
    let (reply, recv) = oneshot::channel();
    commands
        .send(ListenerCommand::Resume {
            connection_id: connection_id.into(),
            include_turns,
            reply,
        })
        .unwrap();
    recv.await.unwrap()
}

#[tokio::test]
async fn two_connections_receive_the_same_thread_event_order() {
    let (connections, commands) = start_listener().await;
    let (mut first, _) = connections.register("first".into()).await;
    let (mut second, _) = connections.register("second".into()).await;
    resume(&commands, "first", false).await;
    resume(&commands, "second", false).await;
    for msg in [
        EventMsg::TurnStarted(TurnStartedEvent { turn_id: "turn-1".into() }),
        EventMsg::TurnComplete(TurnCompleteEvent {
            turn_id: "turn-1".into(),
            last_agent_message: Some("hello".into()),
            error: None,
        }),
    ] {
        commands
            .send(ListenerCommand::CoreEvent(Event { id: "turn-1".into(), msg }))
            .unwrap();
    }
    for _ in 0..2 {
        assert_eq!(first.recv().await.unwrap(), second.recv().await.unwrap());
    }
}

#[tokio::test]
async fn running_resume_has_no_snapshot_to_live_gap() {
    let (connections, commands) = start_listener().await;
    let (mut connection, _) = connections.register("resume".into()).await;
    commands
        .send(ListenerCommand::CoreEvent(Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnStarted(TurnStartedEvent { turn_id: "turn-1".into() }),
        }))
        .unwrap();
    let snapshot = resume(&commands, "resume", true).await;
    assert_eq!(snapshot.active_turn.unwrap().id, "turn-1");
    commands
        .send(ListenerCommand::CoreEvent(Event {
            id: "turn-1".into(),
            msg: EventMsg::TurnComplete(TurnCompleteEvent {
                turn_id: "turn-1".into(),
                last_agent_message: Some("done".into()),
                error: None,
            }),
        }))
        .unwrap();
    let live = connection.recv().await.unwrap();
    assert!(matches!(
        live.payload,
        Some(proto::thread_event::Payload::TurnComplete(_))
    ));
}
```

- [x] **Step 2: Run tests and verify failure**

Run `cargo test -p server --test thread_events_test`.

Expected: FAIL because there is no Thread listener or RPC implementation.

- [x] **Step 3: Implement Core-to-proto mapping**

In `thread_listener.rs`, add these helpers:

```rust
fn item_type(item: &agent_protocol::TurnItem) -> &'static str {
    use agent_protocol::TurnItem;
    match item {
        TurnItem::UserMessage(_) => "user_message",
        TurnItem::HookPrompt(_) => "hook_prompt",
        TurnItem::AgentMessage(_) => "agent_message",
        TurnItem::Plan(_) => "plan",
        TurnItem::Reasoning(_) => "reasoning",
        TurnItem::CommandExecution(_) => "command_execution",
        TurnItem::DynamicToolCall(_) => "dynamic_tool_call",
        TurnItem::McpToolCall(_) => "mcp_tool_call",
        TurnItem::CollabAgentToolCall(_) => "collab_agent_tool_call",
        TurnItem::SubAgentActivity(_) => "subagent_activity",
        TurnItem::WebSearch(_) => "web_search",
        TurnItem::ImageView(_) => "image_view",
        TurnItem::ImageGeneration(_) => "image_generation",
        TurnItem::FileChange(_) => "file_change",
        TurnItem::ContextCompaction(_) => "context_compaction",
        TurnItem::EnteredReviewMode(_) => "entered_review_mode",
        TurnItem::ExitedReviewMode(_) => "exited_review_mode",
        TurnItem::Extension(_) => "extension",
    }
}

fn item_to_proto(item: &agent_protocol::TurnItem, status: &str) -> proto::ThreadItem {
    proto::ThreadItem {
        id: item.id().into(),
        item_type: item_type(item).into(),
        status: status.into(),
        payload_json: serde_json::to_string(item)
            .unwrap_or_else(|error| serde_json::json!({"serialization_error":error.to_string()}).to_string()),
    }
}
```

Implement `event_to_proto(thread_id, event)` as an exhaustive `match`. Use:

- `ItemStarted` → `thread_event::Payload::ItemStarted` with status `in_progress`;
- `ItemCompleted(TurnItem::Extension)` → `ThreadExtension`; every other `ItemCompleted` → `ItemCompleted` with status `completed`;
- delta variants → the matching `ThreadDelta` payload;
- all approval/user-input/permission/elicitation variants → `ThreadControlRequest` with a stable `kind`;
- MCP/Hook/Subagent/context item events → item started/completed notifications;
- legacy message/reasoning/completion events → the corresponding typed item-completed notification;
- settings/rollback → `ThreadExtension` namespace `astro.thread_settings` / `astro.thread_rollback`;
- terminal and error variants → their typed protobuf payload;
- `ShutdownComplete` → `shutdown_complete = true`.

This function must return one proto event for every Core `EventMsg` variant; no variant may map to `None`.

- [x] **Step 4: Implement one serialized listener command loop**

Use one command queue for Core events and subscriber mutations. `run_thread_listener` owns the only `AstroThread::next_event` pump; the pump translates each event into `ListenerCommand::CoreEvent`, while `run_listener_commands` is the only code allowed to mutate `ThreadState`:

```rust
pub async fn run_thread_listener(
    thread_id: String,
    thread: Arc<agent::AstroThread>,
    state: Arc<tokio::sync::Mutex<ThreadState>>,
    commands: tokio::sync::mpsc::UnboundedSender<ListenerCommand>,
    command_rx: tokio::sync::mpsc::UnboundedReceiver<ListenerCommand>,
    connections: ConnectionRegistry,
) {
    let pump_commands = commands.clone();
    let pump = tokio::spawn(async move {
        while let Ok(event) = thread.next_event().await {
            if pump_commands.send(ListenerCommand::CoreEvent(event)).is_err() {
                break;
            }
        }
    });
    run_listener_commands(thread_id, state, command_rx, connections).await;
    pump.abort();
}

pub async fn run_listener_commands(
    thread_id: String,
    state: Arc<tokio::sync::Mutex<ThreadState>>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<ListenerCommand>,
    connections: ConnectionRegistry,
) {
    while let Some(command) = commands.recv().await {
        match command {
            ListenerCommand::CoreEvent(event) => {
                let (subscribers, outbound) = {
                    let mut state = state.lock().await;
                    state.history.track(&event);
                    state.status = match &event.msg {
                        EventMsg::TurnStarted(_) => "running".into(),
                        EventMsg::TurnComplete(completed) if completed.error.is_some() => {
                            "errored".into()
                        }
                        EventMsg::TurnComplete(_) | EventMsg::TurnAborted(_) => "idle".into(),
                        EventMsg::ShutdownComplete => "shutdown".into(),
                        _ => state.status.clone(),
                    };
                    let _ = state.activity_tx.send(ThreadActivity {
                        status: state.status.clone(),
                        has_subscribers: !state.subscribers.is_empty(),
                    });
                    (
                        state.subscribers.iter().cloned().collect::<Vec<_>>(),
                        event_to_proto(&thread_id, &event),
                    )
                };
                for connection_id in subscribers {
                    connections.send_to(&connection_id, outbound.clone()).await;
                }
            }
            ListenerCommand::Resume { connection_id, include_turns, reply } => {
                let snapshot = {
                    let mut state = state.lock().await;
                    state.subscribers.insert(connection_id);
                    let _ = state.activity_tx.send(ThreadActivity {
                        status: state.status.clone(),
                        has_subscribers: true,
                    });
                    ThreadSnapshot {
                        thread_id: thread_id.clone(),
                        status: state.status.clone(),
                        turns: if include_turns {
                            state.history.completed_turns().to_vec()
                        } else {
                            Vec::new()
                        },
                        active_turn: state.history.active_turn_snapshot(),
                    }
                };
                let _ = reply.send(snapshot);
            }
            ListenerCommand::Unsubscribe { connection_id } => {
                let mut state = state.lock().await;
                state.subscribers.remove(&connection_id);
                let _ = state.activity_tx.send(ThreadActivity {
                    status: state.status.clone(),
                    has_subscribers: !state.subscribers.is_empty(),
                });
            }
            ListenerCommand::Stop => break,
        }
    }
}
```

The listener queue establishes the recovery boundary: a Core event is either processed before `Resume` and included in its snapshot, or processed afterward and delivered live to the newly inserted subscriber. It cannot fall between the two.

Re-export `run_listener_commands`, `ConnectionRegistry`, `ListenerCommand`, `ThreadHistoryBuilder`, `ThreadSnapshot`, and `ThreadState` from `server::lib` for the integration test. Keep `event_to_proto` private.

- [x] **Step 5: Replace the Session map with a Thread manager**

In `astro_service.rs`, replace:

```rust
sessions: Arc<RwLock<HashMap<String, SessionHandle>>>,
```

with:

```rust
threads: ThreadManager,
thread_states: ThreadStateManager,
connections: ConnectionRegistry,
```

Create `thread_manager.rs` with these ownership types:

```rust
pub struct ManagedThread {
    pub runtime: Arc<agent::AstroThread>,
    pub commands: tokio::sync::mpsc::UnboundedSender<ListenerCommand>,
    pub activity_rx: tokio::sync::watch::Receiver<ThreadActivity>,
    pub listener: tokio::task::JoinHandle<()>,
}

#[derive(Clone, Default)]
pub struct ThreadManager {
    entries: Arc<RwLock<HashMap<String, Arc<ManagedThread>>>>,
    creation_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}
```

Implement `get`, `insert_if_absent`, `remove`, `contains`, and `creation_lock(thread_id)`. `insert_if_absent` returns the existing value on collision so the caller can shut down the unused candidate. `AstroServiceImpl::get_or_create_thread` acquires `creation_lock(thread_id)` and performs the following sequence while holding only that per-thread lock:

`AstroServiceImpl::get_or_create_thread` must:

1. return an existing `Arc<AstroThread>` when loaded;
2. build the Session with `AgentBuilder` when absent;
3. create or locate the rollout path under `~/.astro/sessions/rollouts/YYYY/MM/DD`;
4. open `RolloutRecorder` in `Paginated` mode;
5. spawn `AstroThread`;
6. seed `ThreadHistoryBuilder` from existing rollout items;
7. start exactly one listener task;
8. insert the Thread only after all prior steps succeed.

The method returns `Arc<ManagedThread>`. Never hold the global entries lock while awaiting Session construction, rollout I/O, or listener startup.

- [x] **Step 6: Implement the four RPC methods**

In `grpc/thread_service.rs`, implement:

```rust
pub(crate) async fn subscribe_thread_events(
    service: &AstroServiceImpl,
    request: tonic::Request<proto::SubscribeThreadEventsRequest>,
) -> Result<tonic::Response<ThreadEventsStream>, tonic::Status>;

pub(crate) async fn submit_turn(
    service: &AstroServiceImpl,
    request: tonic::Request<proto::SubmitTurnRequest>,
) -> Result<tonic::Response<proto::SubmitTurnResponse>, tonic::Status>;

pub(crate) async fn resume_thread(
    service: &AstroServiceImpl,
    request: tonic::Request<proto::ResumeThreadRequest>,
) -> Result<tonic::Response<proto::ResumeThreadResponse>, tonic::Status>;

pub(crate) async fn unsubscribe_thread(
    service: &AstroServiceImpl,
    request: tonic::Request<proto::UnsubscribeThreadRequest>,
) -> Result<tonic::Response<proto::Empty>, tonic::Status>;
```

Required ordering:

- `SubscribeThreadEvents` registers the connection before returning the stream.
- `SubmitTurn` verifies the connection exists, sends a listener `Resume` command with `include_turns=false`, waits for its reply, submits `ThreadSettings`, then calls `AstroThread::submit_turn`.
- `ResumeThread` sends one listener `Resume` command and converts the returned snapshot.
- `UnsubscribeThread` removes only the connection id from the Thread subscriber set.

The outbound stream bridges the registry receiver to `Result<ThreadEvent, Status>`. On cancellation, return `Status::resource_exhausted("slow thread-event consumer")` once, then close.

- [x] **Step 7: Convert existing Chat into a compatibility adapter**

Add two private, exhaustive adapter helpers in `grpc/thread_service.rs`:

```rust
fn turn_request_from_chat(
    request: &proto::ChatRequest,
) -> Result<agent_protocol::TurnInputRequest, tonic::Status>;

fn thread_event_to_chat_events(event: proto::ThreadEvent) -> Vec<proto::ChatEvent>;
```

`turn_request_from_chat` reuses the current Chat request parser for text, images, interaction mode, model, provider, tools, and HITL configuration; it must return `invalid_argument` instead of dropping an unsupported field. `thread_event_to_chat_events` follows the Task 8 mapping in reverse. It returns zero events only for thread settings/rollback notifications, one event for deltas/items/control/usage/errors, and `RunFinished` followed by legacy `Done` for `turn_complete` or `turn_aborted`.

The `chat` RPC must no longer call `stream_multi_turn_with_hitl`. Its adapter flow is:

```rust
let connection_id = format!("chat-{}", uuid::Uuid::new_v4());
let (mut event_rx, cancel) = self.connections.register(connection_id.clone()).await;
let managed = self.get_or_create_thread(&session_id).await?;
let (reply, recv) = tokio::sync::oneshot::channel();
managed.commands.send(ListenerCommand::Resume {
    connection_id: connection_id.clone(),
    include_turns: false,
    reply,
}).map_err(|_| tonic::Status::unavailable("thread listener stopped"))?;
recv.await.map_err(|_| tonic::Status::unavailable("thread listener stopped"))?;
let (submission_id, submission) = thread
    .submit_turn(turn_request_from_chat(&req)?, TurnInputMode::StartOrSteer)
    .await?;
let turn_id = submission
    .turn_id()
    .ok_or_else(|| tonic::Status::failed_precondition("turn input was not submitted"))?
    .to_string();
```

Here `thread` is `Arc::clone(&managed.runtime)`. Spawn a compatibility mapping task that filters by `turn_id`, calls `thread_event_to_chat_events`, and closes after `turn_complete` or `turn_aborted`. The adapter may emit legacy `Done` only after the terminal event; Core must not.

- [x] **Step 8: Add 30-minute idle unload**

Clone `ManagedThread::activity_rx` into one unload task per loaded Thread. Start the 30-minute timer only when `status == "idle" && !has_subscribers`; any watch change cancels and recomputes the timer:

```rust
loop {
    let idle = {
        let activity = activity_rx.borrow().clone();
        activity.status == "idle" && !activity.has_subscribers
    };
    if !idle {
        if activity_rx.changed().await.is_err() { break; }
        continue;
    }
    tokio::select! {
        changed = activity_rx.changed() => {
            if changed.is_err() { break; }
        }
        _ = tokio::time::sleep(std::time::Duration::from_secs(30 * 60)) => {
            let activity = activity_rx.borrow().clone();
            if activity.status == "idle" && !activity.has_subscribers {
                managed.runtime.submit(agent_protocol::Op::Shutdown).await?;
                managed.runtime.flush_rollout().await?;
                thread_manager.remove(&thread_id).await;
                thread_state_manager.remove(&thread_id).await;
                break;
            }
        }
    }
}
```

Add a paused-time test using `#[tokio::test(start_paused = true)]` that advances 29 minutes and asserts loaded, then advances one more minute and asserts unloaded.

- [x] **Step 9: Run server tests**

Run:

```bash
cargo test -p server --test thread_events_test
cargo test -p server thread_state::tests --lib
cargo test -p server transport::tests --lib
cargo check -p server --all-targets
```

Expected: PASS; no generated AstroService trait method is missing.

- [x] **Step 10: Commit**

```bash
git add crates/agent-server/src crates/agent-server/tests/thread_events_test.rs
git commit -m "feat(server): add codex-style thread listener and resume"
```

## Batch D — Tauri migration and legacy removal

### Task 13: Migrate Tauri to one connection event stream

**Files:**
- Create: `apps/desktop/src-tauri/src/infra/thread_events.rs`
- Modify: `apps/desktop/src-tauri/src/infra/mod.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/commands/chat.rs`
- Test: unit tests in `apps/desktop/src-tauri/src/infra/thread_events.rs`

- [x] **Step 1: Write proto-to-UI mapping tests**

Add to `thread_events.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_thread_event_maps_to_run_finished_then_done() {
        let mapped = map_thread_event(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::TurnComplete(
                proto::ThreadTurnComplete {
                    last_agent_message: "done".into(),
                    error: None,
                    has_error: false,
                },
            )),
        });
        assert!(matches!(mapped.as_slice(), [
            ChatStreamEvent::RunFinished { outcome_type, .. },
            ChatStreamEvent::Done,
        ] if outcome_type == "success"));
    }

    #[test]
    fn memory_extension_maps_to_existing_session_event_shape() {
        let event = proto::ThreadExtension {
            item_id: "memory-1".into(),
            namespace: "astro.memory".into(),
            payload_json: serde_json::json!({
                "source":"review",
                "target":"memory",
                "summary":"updated",
                "live_written":true
            })
            .to_string(),
        };
        assert!(extension_to_session_event("session-1", event).is_some());
    }
}
```

- [x] **Step 2: Run tests and verify failure**

Run `cargo test -p astro-agent thread_events::tests`.

Expected: FAIL because the bridge does not exist.

- [x] **Step 3: Implement the connection bridge**

Create `thread_events.rs` with one managed state:

```rust
pub struct ThreadEventsBridge {
    connection_id: String,
    ready: tokio::sync::Notify,
    active_threads: tokio::sync::RwLock<std::collections::HashSet<String>>,
}

impl ThreadEventsBridge {
    pub fn new() -> Self {
        Self {
            connection_id: uuid::Uuid::new_v4().to_string(),
            ready: tokio::sync::Notify::new(),
            active_threads: tokio::sync::RwLock::new(std::collections::HashSet::new()),
        }
    }
}
```

At app setup, open `SubscribeThreadEvents(connection_id)` and set ready only after the server accepts the stream. For each incoming event:

- map message/reasoning/tool/usage/terminal payloads to existing `ChatStreamEvent`;
- emit to `format!("chat_stream_{}", event.thread_id)`;
- map `astro.memory`, `astro.pending`, and `astro.session_metadata` extensions to the existing `session_event` UI event;
- remove a thread from `active_threads` after its terminal event.

On stream failure, reconnect with exponential backoff from 500 ms to 15 seconds. After the new stream is ready, call `ResumeThread(include_turns=true)` for every id in `active_threads`; emit the returned snapshot as `thread_snapshot` before accepting live events. If the snapshot shows the formerly active Turn as completed/failed/aborted, synthesize its terminal UI projection and remove the Thread from `active_threads`; otherwise retain it for live delivery.

- [x] **Step 4: Change start_chat to SubmitTurn**

Replace the task that calls the streaming `Chat` RPC with:

```rust
let bridge = app
    .state::<std::sync::Arc<ThreadEventsBridge>>()
    .inner()
    .clone();
bridge.wait_ready().await;
bridge.active_threads.write().await.insert(session_id.clone());
let mut client = AstroServiceClient::connect(endpoint).await.map_err(|e| e.to_string())?;
let response = client
    .submit_turn(proto::SubmitTurnRequest {
        connection_id: bridge.connection_id().to_string(),
        chat: Some(chat_request),
        mode: "start_or_steer".into(),
        expected_turn_id: String::new(),
    })
    .await
    .map_err(|e| e.to_string())?
    .into_inner();
if response.disposition == "not_submitted" {
    return Err(response.reason);
}
```

Do not spawn a second gRPC stream per chat. `chat_control(cancel)` becomes `Op::Interrupt` through the server Thread manager; pause/resume remains a Thread control operation until the pause protocol is migrated.

- [x] **Step 5: Run Tauri checks**

Run:

```bash
cargo test -p astro-agent thread_events::tests
cargo check -p astro-agent --all-targets
cd apps/desktop && npx tsc --noEmit
```

Expected: PASS; start_chat has no direct `.chat(...).into_inner()` stream loop.

- [x] **Step 6: Commit**

```bash
git add apps/desktop/src-tauri apps/desktop/src
git commit -m "refactor(desktop): consume shared thread event stream"
```

### Task 14: Migrate session side effects and remove duplicate event systems

**Files:**
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Modify: `crates/agent-server/src/lib.rs`
- Delete: `crates/agent-server/src/session_events.rs`
- Modify: `crates/agent-core/src/lib.rs`
- Delete: `crates/agent-core/src/event_bus.rs`
- Modify: `crates/agent-core/src/streaming/mod.rs`
- Modify: `crates/agent-core/src/streaming/types.rs`
- Modify: `crates/agent-proto/proto/astro.proto`
- Delete: `apps/desktop/src-tauri/src/infra/session_events.rs`
- Modify: `apps/desktop/src-tauri/src/infra/mod.rs`
- Test: `crates/agent-server/tests/thread_events_test.rs`

- [x] **Step 1: Add Extension migration tests**

Add server tests that assert:

```rust
#[tokio::test]
async fn background_review_emits_durable_memory_extension() {
    let (connections, commands) = start_listener().await;
    let (mut connection, _) = connections.register("desktop".into()).await;
    resume(&commands, "desktop", false).await;
    commands
        .send(ListenerCommand::CoreEvent(Event {
            id: "background-review".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "background-review".into(),
                item: TurnItem::Extension(ExtensionItem {
                    id: "memory-1".into(),
                    namespace: "astro.memory".into(),
                    payload: serde_json::json!({"summary":"memory updated"}),
                }),
            }),
        }))
        .unwrap();
    let event = connection.recv().await.unwrap();
    let Some(proto::thread_event::Payload::Extension(extension)) = event.payload else {
        panic!("expected extension");
    };
    let payload: serde_json::Value = serde_json::from_str(&extension.payload_json).unwrap();
    assert_eq!(payload["summary"], "memory updated");
}

#[tokio::test]
async fn workspace_pending_uses_the_workspace_event_thread() {
    let (connections, commands) = start_listener_for(WORKSPACE_EVENT_THREAD_ID).await;
    let (mut connection, _) = connections.register("desktop".into()).await;
    resume(&commands, "desktop", false).await;
    commands
        .send(ListenerCommand::CoreEvent(Event {
            id: "pending-1".into(),
            msg: EventMsg::ItemCompleted(ItemEvent {
                turn_id: "pending-1".into(),
                item: TurnItem::Extension(ExtensionItem {
                    id: "pending-1".into(),
                    namespace: "astro.pending".into(),
                    payload: serde_json::json!({"pending_count":2}),
                }),
            }),
        }))
        .unwrap();
    let event = connection.recv().await.unwrap();
    let Some(proto::thread_event::Payload::Extension(extension)) = event.payload else {
        panic!("expected extension");
    };
    let payload: serde_json::Value = serde_json::from_str(&extension.payload_json).unwrap();
    assert_eq!(payload["pending_count"], 2);
}
```

Generalize the Task 12 test helper to `start_listener_for(thread_id: &str)` and keep `start_listener()` as a one-line call for `"thread-1"`. Add `ItemEvent`, `TurnItem`, `ExtensionItem`, and `WORKSPACE_EVENT_THREAD_ID` to the imports.

- [x] **Step 2: Run tests and verify failure**

Run:

```bash
cargo test -p server background_review_emits_durable_memory_extension --test thread_events_test
cargo test -p server workspace_pending_uses_the_workspace_event_thread --test thread_events_test
```

Expected: FAIL while emitters still publish through `SessionEventHub`.

- [x] **Step 3: Route all side effects through Op::EmitExtension**

Use these namespaces and payload keys:

```text
astro.memory          source,target,summary,live_written
astro.pending         pending_count,reason
astro.session_metadata title
```

For session-scoped review/title events, call:

```rust
thread
    .submit(Op::EmitExtension {
        item: ExtensionItem {
            id: uuid::Uuid::new_v4().to_string(),
            namespace: "astro.memory".into(),
            payload: serde_json::json!({
                "source": "review",
                "target": "memory",
                "summary": summary,
                "live_written": true,
            }),
        },
    })
    .await?;
```

Define `pub const WORKSPACE_EVENT_THREAD_ID: &str = "astro-workspace-events";`. Tauri always resumes that Thread after opening its connection. Global pending changes submit `EmitExtension` to this Thread.

- [x] **Step 4: Remove legacy event implementations**

Delete:

- `SessionEventHub`, `SequencedSessionEvent`, cursor replay, and `SubscribeSessionEvents` RPC;
- Tauri `session_events` reconnect loop and its `stream_id/after_event_id` state;
- unused `EventBus` and `AgentEvent`;
- `MultiTurnStreamItem`, `RunFinished`, and `Done` as Core types;
- `multi_turn_to_chat_event` from the server.

Keep `ChatEvent.Done` only in the compatibility wire adapter and `ChatStreamEvent::Done` only in the desktop UI adapter.
Keep `StreamedAssistantContent`, `AssistantContentStream`, and `map_new_provider_stream` in `streaming/types.rs`; only remove the multi-turn wrapper enum and alias.

- [x] **Step 5: Regenerate and run migration tests**

Run:

```bash
cargo test -p server --test thread_events_test
cargo test -p agent --all-targets
cargo check -p server --all-targets
cargo check -p astro-agent --all-targets
```

Expected: PASS and `rg -n "SessionEventHub|MultiTurnStreamItem|pub struct EventBus|after_event_id|stream_id" crates/agent-core crates/agent-server apps/desktop/src-tauri` returns no runtime implementation hits.

- [x] **Step 6: Commit**

```bash
git add -A crates/agent-core crates/agent-server crates/agent-proto apps/desktop/src-tauri
git commit -m "refactor(events): remove duplicate session event paths"
```

## Batch E — Recovery projection, documentation, and release verification

### Task 15: Rebuild SQLite projection from rollout

**Files:**
- Modify: `crates/agent-session/src/store/messages.rs`
- Create: `crates/agent-session/src/store/rollout_projection.rs`
- Modify: `crates/agent-session/src/store/mod.rs`
- Modify: `crates/agent-session/Cargo.toml`
- Test: `crates/agent-session/tests/rollout_projection_test.rs`

- [x] **Step 1: Write projection rebuild test**

Create `rollout_projection_test.rs`:

```rust
#[test]
fn fresh_message_projection_rebuilds_from_rollout() {
    let dir = tempfile::tempdir().unwrap();
    let store = session::SessionStore::open(&dir.path().join("state.db")).unwrap();
    let items = vec![
        agent_rollout::RolloutItem::ResponseItem(types::message::Message::user("hello")),
        agent_rollout::RolloutItem::ResponseItem(types::message::Message::assistant("world")),
    ];
    session::store::rebuild_messages_from_rollout(&store, "thread-1", &items).unwrap();
    session::store::rebuild_messages_from_rollout(&store, "thread-1", &items).unwrap();
    let messages = store.get_messages("thread-1").unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content.as_deref(), Some("hello"));
    assert_eq!(messages[1].content.as_deref(), Some("world"));
}
```

- [x] **Step 2: Run test and verify failure**

Run `cargo test -p session --test rollout_projection_test`.

Expected: FAIL because the projection rebuilder does not exist.

- [x] **Step 3: Implement deterministic projection**

Add `agent-protocol` and `agent-rollout` dependencies to `agent-session`. Implement:

```rust
pub fn rebuild_messages_from_rollout(
    store: &SessionStore,
    session_id: &str,
    items: &[agent_rollout::RolloutItem],
) -> anyhow::Result<()> {
    store.ensure_session(session_id, "rollout")?;
    for item in items {
        if let agent_rollout::RolloutItem::ResponseItem(message) = item {
            append_runtime_message(store, session_id, message)?;
        }
    }
    Ok(())
}
```

`append_runtime_message` maps user/assistant/tool roles, media, tool_call_id, tool_calls, reasoning, reasoning_details, and compressed_content into `NewMessage`. Before rebuilding, delete only rows for the target session inside one transaction; never delete the whole database.

- [x] **Step 4: Run projection tests**

Run:

```bash
cargo test -p session --test rollout_projection_test
cargo test -p session --all-targets
```

Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/agent-session Cargo.lock
git commit -m "feat(session): rebuild message projection from rollout"
```

### Task 16: Update architecture docs and run the full verification matrix

**Files:**
- Modify: `docs/superpowers/specs/2026-08-18-agent-loop-codex-alignment-design.md`
- Modify: `docs/04-详细设计阶段/01-核心引擎层/07-Agent生命周期详细设计.md`
- Create: `docs/04-详细设计阶段/01-核心引擎层/12-Agent事件与恢复详细设计.md`

- [x] **Step 1: Update canonical diagrams and invariants**

Document this exact chain:

```text
AstroThread::submit(Op)
  → bounded(512)
  → Session::submission_loop
  → SessionTask::run_turn
  → EventMsg
  → rollout policy + append
  → Core event queue
  → one Server listener
  → ThreadHistoryBuilder
  → connection queues bounded(128)
  → Tauri / exec / compatibility Chat
```

Mark the spec status `已实现` only after all tests below pass. State explicitly that recovery is snapshot + live stream and transient deltas are not replayed.

- [x] **Step 2: Run formatting and focused tests**

Run:

```bash
cargo fmt --all -- --check
cargo test -p agent-protocol
cargo test -p agent-rollout
cargo test -p agent --test thread_event_lifecycle_test
cargo test -p agent --test streaming_test
cargo test -p server --test thread_events_test
cargo test -p session --test rollout_projection_test
```

Expected: all PASS.

- [x] **Step 3: Run workspace and desktop verification**

Run:

```bash
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cd apps/desktop && npx tsc --noEmit
cd apps/desktop && npm run build
```

Expected: all commands exit 0. If a known baseline failure remains, record the exact command and error in the design document; do not describe the alignment as complete until all new focused tests pass.

- [x] **Step 4: Check architectural deletion and event coverage**

Run:

```bash
rg -n "SessionEventHub|MultiTurnStreamItem|pub struct EventBus|after_event_id|stream_id" crates/agent-core crates/agent-server apps/desktop/src-tauri
rg -n "TurnStarted|ItemStarted|ItemCompleted|TurnComplete|TurnAborted|ExecCommandOutputDelta|HookStarted|McpToolCallBegin|SubAgentActivity" crates/agent-core crates/agent-server
git diff --check
```

Expected: the first command has no runtime implementation hits; the second command finds emitters and mapping tests for every required event family; `git diff --check` is silent.

- [x] **Step 5: Commit documentation and verification record**

```bash
git add docs
git commit -m "docs: record codex-aligned agent loop architecture"
```

## Final acceptance checklist

- [x] Every Thread owns one long-lived Session and one 512-capacity submission queue.
- [x] Every foreground and background Turn emits the same EventMsg lifecycle.
- [x] Every Turn has exactly one `TurnComplete` or `TurnAborted`.
- [x] Durable events are appended to rollout before Core delivery.
- [x] Multiple subscribers observe the same order.
- [x] A full 128-entry connection queue disconnects only that slow connection.
- [x] Resume returns durable turns plus an active snapshot with no snapshot/live gap.
- [x] Transient token/reasoning/stdout deltas are not promised during recovery.
- [x] SQLite messages can be rebuilt from rollout.
- [x] `SessionEventHub`, Core `EventBus`, Core `MultiTurnStreamItem`, and cursor replay are removed.
- [x] app-server, Tauri, exec, Cron, MCP, Hook, approval, Subagent, patch, and compaction paths use the unified protocol.
