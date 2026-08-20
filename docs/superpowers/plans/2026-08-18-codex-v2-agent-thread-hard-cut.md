# Codex V2 Agent Thread Hard Cut Implementation Plan

> **历史实施记录：** 本文保留 TDD 任务和当时的预期输出，不是当前 runtime 契约。其中 `subagents.db`、旧工具/参数和 `.astro/agents` 均是已被替换或用于负向验证的历史内容；当前真值以 `docs/subagents.md` 与 `subagents-v2.db` 实现为准。

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace Astro's compatibility-based subagent implementation with a single Codex V2 Agent Thread contract, including durable tree/mailbox state, real runner lifecycle events, desktop-only read/close control, and event-driven frontend activity.

**Architecture:** One `AgentControl` is created per root session and shared by all descendants through session-specific dispatch handles. `subagents.db` owns graph, mailbox, status-event, and migration metadata; SessionStore remains the only conversation/tool timeline. Model tools expose only the six Codex V2 operations, while Tauri retains read and recursive close as desktop control-plane commands.

**Tech Stack:** Rust 2021, Tokio, rusqlite/WAL, serde/schemars, Tonic/Protobuf, Tauri 2, React/TypeScript, Vitest.

**Design:** `docs/superpowers/specs/2026-08-18-codex-v2-agent-thread-design.md`

**Codex reference:** `/Users/iswm/CodeRope/codex/codex-rs`, especially `core/src/agent/control.rs`, `core/src/agent/control/spawn.rs`, `core/src/agent/control/residency.rs`, and `core/src/tools/handlers/multi_agents_v2/`.

---

## File Structure

### Agent Thread domain and persistence

- Create `crates/agent-subagents/src/path.rs`: canonical `/root/<segments>` path validation and resolution.
- Create `crates/agent-subagents/src/mailbox.rs`: durable mailbox value types and delivery semantics.
- Create `crates/agent-subagents/src/registry.rs`: root-scoped tree index, spawn reservations, identity and execution quotas.
- Create `crates/agent-subagents/src/activity.rs`: activity sequence, subscription, wait outcomes, and UI event payloads.
- Create `crates/agent-subagents/src/migration.rs`: idempotent v1 historical archive migration and v2 schema setup.
- Rewrite `crates/agent-subagents/src/model.rs`: V2 statuses, thread metadata, strict request/result types.
- Rewrite `crates/agent-subagents/src/store.rs`: graph/mailbox/status-event transactions and snapshot queries.
- Rewrite `crates/agent-subagents/src/control.rs`: one shared root control coordinating registry, store, activity, and runtime handles.
- Modify `crates/agent-subagents/src/config.rs`: load only `.codex` agent/config layers.
- Modify `crates/agent-subagents/src/lib.rs`: export only V2 domain/control APIs and desktop control types.

### Runtime and model tools

- Create `crates/agent-core/src/exec/agent_control_directory.rs`: return one shared control per root session to runtime and desktop callers.
- Create `crates/agent-core/src/exec/agent_runtime.rs`: active-turn runtime manager, lazy Session restore, interrupt/termination acknowledgements.
- Rewrite `crates/agent-core/src/exec/dispatch.rs`: session-bound V2 dispatch and desktop control-plane adapter.
- Rewrite `crates/agent-core/src/exec/subagents.rs`: one-turn runner driven by mailbox and runner events.
- Modify `crates/agent-core/src/exec/mod.rs`: register the runtime manager module.
- Modify `crates/agent-core/src/runtime/session_services.rs`: store the shared `AgentControl` and current `AgentPath`.
- Modify `crates/agent-core/src/runtime/mod.rs`: construct root controls and pass the same control to child sessions.
- Modify `crates/agent-core/src/runtime/turn_lifecycle.rs`: notify Agent Thread waiters when the main turn is steered.
- Modify `crates/agent-core/src/streaming/maintenance.rs`: inject ordered mailbox messages at safe iteration boundaries.
- Modify `crates/agent-session/src/store/messages.rs`: fork recent turns as structured message/tool rows.
- Rewrite `crates/agent-tools/src/engine/execution.rs`: six V2 model operations only.
- Modify `crates/agent-tools/src/engine/context.rs`: expose the session-bound dispatch without legacy ownership arguments.
- Rewrite `crates/agent-tools/src/builtin/agents/subagent.rs`: strict schemas with `deny_unknown_fields` and six registrations.
- Modify `crates/agent-tools/src/lib.rs` and `crates/agent-tools/tests/tools_test.rs`: V2-only tool catalog and regression assertions.

### Backend events, desktop control, and frontend

- Modify `crates/agent-proto/proto/astro.proto`: add Agent Thread activity payload to `SessionEvent`.
- Modify `crates/agent-server/src/session_events.rs`: publish, filter, replay, and serialize Agent Thread events.
- Modify `crates/agent-server/src/grpc/astro_service.rs`: attach one activity watcher per root session.
- Rewrite `apps/desktop/src-tauri/src/commands/subagents.rs`: snapshot, real Session read, follow-up, interrupt, recursive close.
- Modify `apps/desktop/src-tauri/src/infra/session_events.rs`: bridge Agent Thread event payloads to Tauri.
- Create `apps/desktop/src/hooks/chat/subagentTree.ts`: pure snapshot/event reducer.
- Create `apps/desktop/src/hooks/chat/subagentTree.test.ts`: tree, ordering, reconnect, and unread tests.
- Rewrite `apps/desktop/src/hooks/chat/useSubagentThreads.ts`: initial snapshot plus `session_event` updates; no timer.
- Modify `apps/desktop/src/components/chat/SubagentActivityBar.tsx`: hierarchical live activity.
- Modify `apps/desktop/src/components/chat/SubagentsPanel.tsx`: real Session timeline and desktop operations.
- Modify `apps/desktop/src/components/chat/ChatView.tsx`: event-driven props and selection behavior.
- Modify `apps/desktop/src/styles/features/chat/subagents.css`: tree indentation and V2 status styling.
- Modify `apps/desktop/src/i18n/messages.ts`: V2 status/control labels and removal of legacy wording.

### Cleanup and documentation

- Modify `crates/agent-core/src/prompt/interaction_mode.rs`, `crates/agent-core/src/prompt/context_usage.rs`, and any tool guidance files returned by the static scan: remove old tool names.
- Modify `docs/subagents.md`: document the V2-only contract and desktop control plane.
- Modify `AGENTS.md`: replace mixed lifecycle and `.astro/agents` compatibility statements.

---

### Task 1: Hard-cut custom-agent configuration to `.codex`

**Files:**
- Modify: `crates/agent-subagents/src/config.rs`
- Test: `crates/agent-subagents/src/config.rs`

- [ ] **Step 1: Write failing tests proving Astro compatibility paths are ignored**

Add tests that create conflicting files under the old and canonical locations:

```rust
#[test]
fn astro_agent_directories_are_not_configuration_inputs() {
    let memory = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(memory.path().join("agents")).unwrap();
    std::fs::create_dir_all(project.path().join(".astro/agents")).unwrap();
    std::fs::write(
        memory.path().join("agents/legacy.toml"),
        "name = \"legacy\"\ndescription = \"legacy\"\ndeveloper_instructions = \"legacy\"\n",
    ).unwrap();
    std::fs::write(
        project.path().join(".astro/agents/project_legacy.toml"),
        "name = \"project_legacy\"\ndescription = \"legacy\"\ndeveloper_instructions = \"legacy\"\n",
    ).unwrap();

    let catalog = load_agent_catalog(memory.path(), Some(project.path()));

    assert!(!catalog.agents.contains_key("legacy"));
    assert!(!catalog.agents.contains_key("project_legacy"));
}

#[test]
fn astro_config_toml_does_not_override_codex_agents_settings() {
    let memory = tempfile::tempdir().unwrap();
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".astro")).unwrap();
    std::fs::create_dir_all(project.path().join(".codex")).unwrap();
    std::fs::write(project.path().join(".astro/config.toml"), "[agents]\nenabled = false\n").unwrap();
    std::fs::write(project.path().join(".codex/config.toml"), "[agents]\nenabled = true\n").unwrap();

    assert!(load_agents_settings(memory.path(), Some(project.path())).enabled);
}
```

- [ ] **Step 2: Run the tests and verify the old paths still affect results**

Run:

```bash
cargo test -p subagents astro_agent_directories_are_not_configuration_inputs -- --nocapture
cargo test -p subagents astro_config_toml_does_not_override_codex_agents_settings -- --nocapture
```

Expected: at least the directory test fails because `memory_dir/agents` and `<project>/.astro/agents` are currently loaded.

- [ ] **Step 3: Remove the compatibility loaders**

Make the load order explicit and canonical:

```rust
pub fn load_agents_settings(memory_dir: &Path, project_root: Option<&Path>) -> AgentsSettings {
    let mut settings = AgentsSettings::default();
    apply_settings_file(&mut settings, &codex_home(memory_dir).join("config.toml"));
    if let Some(root) = project_root {
        apply_settings_file(&mut settings, &root.join(".codex/config.toml"));
    }
    settings
}

pub fn load_agent_catalog(memory_dir: &Path, project_root: Option<&Path>) -> AgentCatalog {
    let mut catalog = AgentCatalog::default();
    for definition in builtin_agents() {
        catalog.agents.insert(definition.name.clone(), definition);
    }
    load_agent_dir(&codex_home(memory_dir).join("agents"), &mut catalog);
    if let Some(root) = project_root {
        load_agent_dir(&root.join(".codex/agents"), &mut catalog);
    }
    catalog
}
```

Update the existing precedence test so personal `.codex/agents` is overridden by project `.codex/agents`, without creating any legacy file.

- [ ] **Step 4: Run the focused config suite**

Run: `cargo test -p subagents config::tests -- --nocapture`

Expected: all config tests pass; tests prove old Astro paths have no effect and project Codex definitions win.

- [ ] **Step 5: Commit the configuration hard cut**

```bash
git add crates/agent-subagents/src/config.rs
git commit -m "refactor(subagents): hard cut agent config to codex paths"
```

### Task 2: Introduce V2 paths, statuses, and strict request types

**Files:**
- Create: `crates/agent-subagents/src/path.rs`
- Rewrite: `crates/agent-subagents/src/model.rs`
- Modify: `crates/agent-subagents/src/lib.rs`
- Test: `crates/agent-subagents/src/path.rs`
- Test: `crates/agent-subagents/src/model.rs`

- [ ] **Step 1: Write failing AgentPath tests**

```rust
#[test]
fn resolves_relative_and_absolute_agent_paths() {
    let parent = AgentPath::parse("/root/research").unwrap();
    assert_eq!(parent.child("citations").unwrap().as_str(), "/root/research/citations");
    assert_eq!(parent.resolve("/root/review").unwrap().as_str(), "/root/review");
    assert_eq!(parent.resolve("citations").unwrap().as_str(), "/root/research/citations");
}

#[test]
fn rejects_invalid_task_segments() {
    for value in ["", "UPPER", "has-dash", "../escape", "two/parts"] {
        assert!(AgentPath::root().child(value).is_err(), "accepted {value}");
    }
}
```

- [ ] **Step 2: Run the path tests and verify they fail to compile**

Run: `cargo test -p subagents path::tests -- --nocapture`

Expected: compile failure because `AgentPath` and `path` module do not exist.

- [ ] **Step 3: Implement the V2 domain types**

Implement the path API without adding a regex dependency:

```rust
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AgentPath(String);

impl AgentPath {
    pub fn root() -> Self { Self("/root".into()) }
    pub fn parse(value: &str) -> anyhow::Result<Self> {
        let parts = value.split('/').collect::<Vec<_>>();
        if parts.len() < 2 || parts[0] != "" || parts[1] != "root"
            || parts[2..].iter().any(|segment| !is_valid_segment(segment))
        {
            anyhow::bail!("invalid agent path: {value}");
        }
        Ok(Self(value.to_string()))
    }
    pub fn child(&self, task_name: &str) -> anyhow::Result<Self> {
        if !is_valid_segment(task_name) {
            anyhow::bail!("invalid task_name: {task_name}");
        }
        Self::parse(&format!("{}/{task_name}", self.0))
    }
    pub fn resolve(&self, target: &str) -> anyhow::Result<Self> {
        if target.starts_with('/') {
            Self::parse(target)
        } else {
            self.child(target)
        }
    }
    pub fn parent(&self) -> Option<Self> {
        self.0.rsplit_once('/').and_then(|(parent, _)| {
            (!parent.is_empty()).then(|| Self(parent.to_string()))
        })
    }
    pub fn starts_with(&self, prefix: &Self) -> bool {
        self == prefix || self.0.strip_prefix(prefix.as_str()).is_some_and(|rest| rest.starts_with('/'))
    }
    pub fn as_str(&self) -> &str { &self.0 }
}

fn is_valid_segment(value: &str) -> bool {
    !value.is_empty()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}
```

Replace legacy status/request shapes with strict V2 types:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "payload", rename_all = "snake_case")]
pub enum AgentStatus {
    PendingInit,
    Running,
    Interrupted,
    Completed { last_message: String },
    Errored { message: String },
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentStatusKind { PendingInit, Running, Interrupted, Completed, Errored, Shutdown }

impl AgentStatus {
    pub fn kind(&self) -> AgentStatusKind {
        match self {
            Self::PendingInit => AgentStatusKind::PendingInit,
            Self::Running => AgentStatusKind::Running,
            Self::Interrupted => AgentStatusKind::Interrupted,
            Self::Completed { .. } => AgentStatusKind::Completed,
            Self::Errored { .. } => AgentStatusKind::Errored,
            Self::Shutdown => AgentStatusKind::Shutdown,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentThread {
    pub thread_id: String,
    pub root_thread_id: String,
    pub parent_thread_id: Option<String>,
    pub canonical_path: AgentPath,
    pub task_name: String,
    pub agent_type: String,
    pub session_id: String,
    pub status: AgentStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct ThreadReservation {
    pub thread_id: String,
    pub root_thread_id: String,
    pub parent_thread_id: String,
    pub canonical_path: AgentPath,
    pub task_name: String,
    pub agent_type: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RunnerEvent {
    TurnStarted { turn_id: String },
    TurnCompleted { turn_id: String, last_message: String },
    TurnInterrupted { turn_id: String, reason: String },
    TurnErrored { turn_id: String, message: String },
    RuntimeTerminated,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTreeSnapshot {
    pub root_thread_id: String,
    pub activity_sequence: u64,
    pub threads: Vec<AgentThread>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpawnAgentResult { pub thread: AgentThread }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageAgentResult { pub message_id: String, pub queued: bool, pub turn_triggered: bool }

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WaitAgentResult { pub message: String, pub timed_out: bool }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterruptAgentResult { pub thread: AgentThread, pub previous_status: AgentStatus }

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpawnAgentRequest {
    pub task_name: String,
    pub message: String,
    pub agent_type: Option<String>,
    pub model: Option<String>,
    pub reasoning_effort: Option<String>,
    pub fork_turns: Option<String>,
}

#[derive(Clone)]
pub struct SpawnRuntimeRequest {
    pub model_request: SpawnAgentRequest,
    pub parent_thread_id: String,
    pub parent_path: AgentPath,
    pub developer_instructions: String,
    pub chat_targets: Vec<types::ChatTarget>,
    pub project_root: Option<PathBuf>,
    pub hook_bus: Option<Arc<hooks::PluginHookBus>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListAgentsRequest { pub path_prefix: Option<String> }

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageAgentRequest { pub target: String, pub message: String }

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WaitAgentRequest { pub timeout_ms: Option<i64> }

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InterruptAgentRequest { pub target: String }
```

Keep credentials and hook handles only in an in-memory `SpawnRuntimeRequest`; never add them to a serializable stored thread.

- [ ] **Step 4: Add strict deserialization tests and run the crate suite**

```rust
#[test]
fn legacy_arguments_are_rejected() {
    assert!(serde_json::from_value::<ListAgentsRequest>(serde_json::json!({"include_closed": true})).is_err());
    assert!(serde_json::from_value::<MessageAgentRequest>(serde_json::json!({"thread_id": "x", "message": "m"})).is_err());
    assert!(serde_json::from_value::<WaitAgentRequest>(serde_json::json!({"thread_ids": ["x"]})).is_err());
}
```

Run: `cargo test -p subagents -- --nocapture`

Expected: all path/model/config tests pass after fixing imports and old test fixtures.

- [ ] **Step 5: Commit the V2 domain model**

```bash
git add crates/agent-subagents/src/path.rs crates/agent-subagents/src/model.rs crates/agent-subagents/src/lib.rs
git commit -m "feat(subagents): add codex v2 agent thread domain"
```

### Task 3: Migrate `subagents.db` to graph, mailbox, and status-event storage

**Files:**
- Create: `crates/agent-subagents/src/migration.rs`
- Create: `crates/agent-subagents/src/mailbox.rs`
- Rewrite: `crates/agent-subagents/src/store.rs`
- Modify: `crates/agent-subagents/src/lib.rs`
- Test: `crates/agent-subagents/src/migration.rs`
- Test: `crates/agent-subagents/src/store.rs`

- [ ] **Step 1: Write failing migration and transaction tests**

Create a v1 fixture with the existing `agent_threads` and `agent_thread_messages` tables, then assert:

```rust
#[test]
fn v1_rows_are_archived_once_and_never_enter_the_v2_tree() {
    let dir = tempfile::tempdir().unwrap();
    let db = dir.path().join("subagents.db");
    create_v1_database(&db);
    let store = AgentGraphStore::open(db.clone()).unwrap();
    assert_eq!(store.schema_version().unwrap(), 2);
    assert_eq!(store.list_historical().unwrap().len(), 1);
    assert!(store.snapshot("legacy-parent").unwrap().threads.is_empty());

    drop(store);
    let reopened = AgentGraphStore::open(db).unwrap();
    assert_eq!(reopened.list_historical().unwrap().len(), 1);
}

#[test]
fn status_event_and_thread_projection_commit_together() {
    let dir = tempfile::tempdir().unwrap();
    let store = AgentGraphStore::open(dir.path().join("subagents.db")).unwrap();
    let thread = store.reserve_thread(&ThreadReservation {
        thread_id: "child".into(),
        root_thread_id: "root".into(),
        parent_thread_id: "root".into(),
        canonical_path: AgentPath::parse("/root/worker").unwrap(),
        task_name: "worker".into(),
        agent_type: "default".into(),
        session_id: "child-session".into(),
    }).unwrap();
    store.apply_status_event(&thread.thread_id, RunnerEvent::TurnStarted { turn_id: "t1".into() }).unwrap();
    assert_eq!(store.get_thread(&thread.thread_id).unwrap().unwrap().status, AgentStatus::Running);
    assert_eq!(store.status_events(&thread.thread_id).unwrap().len(), 1);
}

fn create_v1_database(path: &Path) {
    let conn = types::open_wal(path).unwrap();
    conn.execute_batch(V1_TEST_DDL).unwrap();
    conn.execute(
        "INSERT INTO agent_threads (
            id, parent_session_id, parent_agent_id, agent_name, task, status,
            created_at, updated_at
         ) VALUES ('legacy', 'legacy-parent', 'astro', 'worker', 'old task',
                   'completed', '2026-08-18T00:00:00Z', '2026-08-18T00:00:00Z')",
        [],
    ).unwrap();
}
```

- [ ] **Step 2: Run the focused tests and verify missing APIs fail**

Run:

```bash
cargo test -p subagents migration::tests -- --nocapture
cargo test -p subagents store::tests -- --nocapture
```

Expected: compile failure because `AgentGraphStore`, schema v2, and mailbox APIs are absent.

- [ ] **Step 3: Implement idempotent schema v2 migration**

Use one SQLite transaction to:

```sql
CREATE TABLE IF NOT EXISTS schema_meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

ALTER TABLE agent_threads RENAME TO historical_agent_threads_v1;
ALTER TABLE agent_thread_messages RENAME TO historical_agent_messages_v1;

CREATE TABLE agent_threads (
    thread_id TEXT PRIMARY KEY,
    root_thread_id TEXT NOT NULL,
    parent_thread_id TEXT,
    canonical_path TEXT NOT NULL,
    task_name TEXT NOT NULL,
    agent_type TEXT NOT NULL,
    session_id TEXT NOT NULL,
    status_kind TEXT NOT NULL,
    status_payload TEXT NOT NULL,
    last_status_sequence INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(root_thread_id, canonical_path)
);

CREATE TABLE agent_spawn_edges (
    parent_thread_id TEXT NOT NULL,
    child_thread_id TEXT PRIMARY KEY,
    edge_state TEXT NOT NULL CHECK(edge_state IN ('open', 'closed')),
    created_at TEXT NOT NULL,
    closed_at TEXT
);

CREATE TABLE agent_mailbox (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    message_id TEXT NOT NULL UNIQUE,
    idempotency_key TEXT NOT NULL UNIQUE,
    sender_thread_id TEXT NOT NULL,
    recipient_thread_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    trigger_turn INTEGER NOT NULL,
    delivery_state TEXT NOT NULL,
    created_at TEXT NOT NULL,
    delivered_at TEXT
);

CREATE TABLE agent_status_events (
    sequence INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    payload TEXT NOT NULL,
    source_turn_id TEXT,
    created_at TEXT NOT NULL
);
```

Only run `ALTER TABLE` when the v1 table exists and the v2 schema marker is absent. Record `schema_version=2` in `schema_meta` in the same transaction.

- [ ] **Step 4: Implement graph and mailbox transactions**

Expose focused methods:

```rust
impl AgentGraphStore {
    pub fn reserve_thread(&self, reservation: &ThreadReservation) -> anyhow::Result<AgentThread>;
    pub fn rollback_pending_thread(&self, thread_id: &str) -> anyhow::Result<()>;
    pub fn apply_status_event(&self, thread_id: &str, event: RunnerEvent) -> anyhow::Result<AgentThread>;
    pub fn enqueue(&self, message: &NewMailboxMessage) -> anyhow::Result<MailboxMessage>;
    pub fn pending_for(&self, recipient: &str, after: i64) -> anyhow::Result<Vec<MailboxMessage>>;
    pub fn mark_delivered(&self, recipient: &str, through_sequence: i64) -> anyhow::Result<()>;
    pub fn snapshot(&self, root_thread_id: &str) -> anyhow::Result<AgentTreeSnapshot>;
    pub fn close_edge(&self, child_thread_id: &str) -> anyhow::Result<()>;
}
```

`enqueue` must return the existing row when an idempotency key is retried. `apply_status_event` must append the event and update the thread projection in one transaction.

Define the mailbox values in `mailbox.rs` and serialize only JSON-safe payloads:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MailboxKind { Message, Followup, Result, Status }

#[derive(Debug, Clone)]
pub struct NewMailboxMessage {
    pub message_id: String,
    pub idempotency_key: String,
    pub sender_thread_id: String,
    pub recipient_thread_id: String,
    pub kind: MailboxKind,
    pub payload: String,
    pub trigger_turn: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MailboxMessage {
    pub sequence: i64,
    pub message_id: String,
    pub sender_thread_id: String,
    pub recipient_thread_id: String,
    pub kind: MailboxKind,
    pub payload: String,
    pub trigger_turn: bool,
}
```

- [ ] **Step 5: Run store tests and commit**

Run: `cargo test -p subagents -- --nocapture`

Expected: migration, idempotency, ordering, projection, and config tests pass.

```bash
git add crates/agent-subagents/src/migration.rs crates/agent-subagents/src/mailbox.rs crates/agent-subagents/src/store.rs crates/agent-subagents/src/lib.rs
git commit -m "feat(subagents): persist agent graph mailbox and status events"
```

### Task 4: Build root-scoped registry, reservations, activity, and shared control

**Files:**
- Create: `crates/agent-subagents/src/registry.rs`
- Create: `crates/agent-subagents/src/activity.rs`
- Rewrite: `crates/agent-subagents/src/control.rs`
- Modify: `crates/agent-subagents/src/lib.rs`
- Test: `crates/agent-subagents/src/registry.rs`
- Test: `crates/agent-subagents/src/control.rs`

- [ ] **Step 1: Write failing reservation, quota, and activity tests**

```rust
#[test]
fn failed_spawn_reservation_releases_path_and_identity_slot() {
    let registry = AgentRegistry::new("root", Limits { max_threads: 4, max_depth: 3, max_running: 1 });
    {
        let _reservation = registry.reserve_spawn(&AgentPath::root(), "worker").unwrap();
    }
    assert!(registry.reserve_spawn(&AgentPath::root(), "worker").is_ok());
}

#[test]
fn completed_threads_do_not_consume_execution_slots() {
    let registry = AgentRegistry::new("root", Limits { max_threads: 4, max_depth: 3, max_running: 1 });
    let permit = registry.reserve_execution("child-a").unwrap();
    drop(permit);
    assert!(registry.reserve_execution("child-b").is_ok());
}

#[tokio::test]
async fn wait_wakes_for_mailbox_activity_without_target_ids() {
    let control = test_control();
    let waiter = tokio::spawn({
        let control = control.clone();
        async move { control.wait_activity(Duration::from_secs(1)).await }
    });
    control.publish_activity(AgentActivityKind::Mailbox { thread_id: "child".into() });
    assert_eq!(waiter.await.unwrap(), WaitOutcome::MailboxActivity);
}

#[test]
fn list_returns_root_and_nested_descendants_in_path_order() {
    let control = test_control();
    insert_thread(&control, "/root/research");
    insert_thread(&control, "/root/research/citations");
    let paths = control
        .list_agents(&AgentPath::root(), None)
        .unwrap()
        .into_iter()
        .map(|thread| thread.canonical_path.to_string())
        .collect::<Vec<_>>();
    assert_eq!(paths, vec!["/root", "/root/research", "/root/research/citations"]);
}
```

- [ ] **Step 2: Run the focused tests and verify missing types fail**

Run:

```bash
cargo test -p subagents registry::tests -- --nocapture
cargo test -p subagents control::tests -- --nocapture
```

Expected: compile failure because the registry/reservation/activity APIs do not exist.

- [ ] **Step 3: Implement RAII reservations and execution permits**

Use guards that commit explicitly and roll back on `Drop`:

```rust
pub struct SpawnReservation<'a> {
    registry: &'a AgentRegistry,
    path: AgentPath,
    committed: bool,
}

impl SpawnReservation<'_> {
    pub fn commit(mut self, thread: AgentThread) -> anyhow::Result<()> {
        self.registry.commit_thread(self.path.clone(), thread)?;
        self.committed = true;
        Ok(())
    }
}

impl Drop for SpawnReservation<'_> {
    fn drop(&mut self) {
        if !self.committed {
            self.registry.release_reserved_path(&self.path);
        }
    }
}
```

Keep identity count and active execution count separate. A completed/interrupted/errored thread stays in the path map but releases its `ExecutionPermit`.

Define the activity API before implementing `AgentControl`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityCursor(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentActivityKind {
    Spawned { thread_id: String },
    Mailbox { thread_id: String },
    StatusChanged { thread_id: String },
    EdgeClosed { thread_id: String },
    MainSteer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WaitOutcome { MailboxActivity, Steered, TimedOut }

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentActivity {
    pub sequence: u64,
    pub kind: AgentActivityKind,
    pub thread: Option<AgentThread>,
}

pub struct ActivityBus {
    sequence: AtomicU64,
    tx: watch::Sender<ActivityCursor>,
}

impl ActivityBus {
    pub fn cursor(&self) -> ActivityCursor;
    pub fn publish(&self, kind: AgentActivityKind) -> AgentActivity;
    pub async fn wait_after(&self, cursor: ActivityCursor, timeout: Duration) -> WaitOutcome;
}
```

- [ ] **Step 4: Implement `AgentControl` and activity cursors**

```rust
pub struct AgentControl {
    root_thread_id: String,
    store: AgentGraphStore,
    registry: AgentRegistry,
    activity: ActivityBus,
    runtimes: RuntimeHandleRegistry,
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub max_threads: usize,
    pub max_depth: usize,
    pub max_running: usize,
}

#[derive(Clone)]
pub struct AgentRuntimeHandle {
    pub interrupt: Arc<dyn Fn() + Send + Sync>,
    pub terminate: Arc<dyn Fn() + Send + Sync>,
}

#[derive(Default)]
pub struct RuntimeHandleRegistry {
    handles: Mutex<HashMap<String, AgentRuntimeHandle>>,
}

impl AgentControl {
    pub fn open(root_thread_id: String, store: AgentGraphStore, limits: Limits) -> anyhow::Result<Arc<Self>>;
}

impl AgentControl {
    pub fn reserve_spawn(&self, parent: &AgentPath, task_name: &str) -> anyhow::Result<SpawnReservation<'_>>;
    pub fn resolve_target(&self, current: &AgentPath, target: &str) -> anyhow::Result<AgentThread>;
    pub fn list_agents(&self, current: &AgentPath, prefix: Option<&str>) -> anyhow::Result<Vec<AgentThread>>;
    pub fn enqueue_message(&self, sender: &AgentPath, request: MessageAgentRequest, trigger_turn: bool) -> anyhow::Result<MailboxMessage>;
    pub fn record_runner_event(&self, thread_id: &str, event: RunnerEvent) -> anyhow::Result<AgentThread>;
    pub async fn wait_activity(&self, cursor: ActivityCursor, timeout: Duration) -> WaitAgentResult;
    pub fn notify_main_steer(&self);
}
```

Activity notifications are wakeups only. Callers retrieve durable mailbox/status rows after the cursor; notifications never carry the only copy of a message.

`AgentControl::open` ensures one root row with canonical path `/root`, parent `None`, and `session_id=root_thread_id`. Root appears in model `list_agents` results but cannot be overwritten by spawn, targeted by self-interrupt, or closed through model tools.

- [ ] **Step 5: Run the crate tests and commit**

Run: `cargo test -p subagents -- --nocapture`

Expected: path, quota, rollback, target resolution, list prefix, activity wakeup, persistence, and migration tests pass.

```bash
git add crates/agent-subagents/src/registry.rs crates/agent-subagents/src/activity.rs crates/agent-subagents/src/control.rs crates/agent-subagents/src/lib.rs
git commit -m "feat(subagents): add shared codex v2 agent control"
```

### Task 5: Replace the resident legacy runner with event-driven active-turn runtimes

**Files:**
- Create: `crates/agent-core/src/exec/agent_control_directory.rs`
- Create: `crates/agent-core/src/exec/agent_runtime.rs`
- Rewrite: `crates/agent-core/src/exec/subagents.rs`
- Modify: `crates/agent-core/src/exec/mod.rs`
- Modify: `crates/agent-core/src/runtime/session_services.rs`
- Modify: `crates/agent-core/src/runtime/mod.rs`
- Modify: `crates/agent-core/src/runtime/turn_lifecycle.rs`
- Modify: `crates/agent-core/src/streaming/maintenance.rs`
- Modify: `crates/agent-session/src/store/messages.rs`
- Test: `crates/agent-session/src/store/messages.rs`
- Test: `crates/agent-core/src/exec/agent_runtime.rs`
- Test: `crates/agent-core/src/exec/subagents.rs`

- [ ] **Step 1: Write failing runtime lifecycle tests**

```rust
#[tokio::test]
async fn completed_turn_releases_execution_slot_but_keeps_thread_addressable() {
    let harness = RuntimeHarness::new(1);
    let child = harness.spawn("worker", scripted_chat(&["done"])).await.unwrap();
    harness.wait_for_status(&child, AgentStatusKind::Completed).await;
    assert_eq!(harness.running_count(), 0);
    assert!(harness.control().resolve_target(&AgentPath::root(), "/root/worker").is_ok());
}

#[tokio::test]
async fn followup_after_interrupt_restores_session_history() {
    let harness = RuntimeHarness::new(1);
    let child = harness.spawn("worker", blocking_chat()).await.unwrap();
    harness.interrupt(&child).await.unwrap();
    harness.followup(&child, "continue", scripted_chat(&["resumed"])).await.unwrap();
    assert_eq!(harness.session_roles(&child), vec!["user", "assistant", "user", "assistant"]);
}

#[tokio::test]
async fn runtime_creation_failure_records_error_and_releases_permit() {
    let harness = RuntimeHarness::with_runtime_failure();
    let error = harness.spawn("worker", scripted_chat(&[])).await.unwrap_err();
    assert!(error.to_string().contains("runtime"));
    assert_eq!(harness.running_count(), 0);
    assert_eq!(harness.thread("/root/worker").status.kind(), AgentStatusKind::Errored);
}

#[test]
fn directory_returns_one_control_per_root_session() {
    let directory = AgentControlDirectory::new(test_store_factory());
    let first = directory.open_root("root-session").unwrap();
    let second = directory.open_root("root-session").unwrap();
    assert!(Arc::ptr_eq(&first, &second));
}

#[test]
fn fork_turns_copies_structured_recent_history_with_tool_rows() {
    let store = session_store_with_two_turns_and_a_tool_result();
    store.fork_session_recent_turns("parent", "child", Some(1)).unwrap();
    let messages = store.get_messages("child").unwrap();
    assert_eq!(messages.iter().map(|message| message.role.as_str()).collect::<Vec<_>>(), vec!["user", "assistant", "tool", "assistant"]);
    assert!(messages.iter().any(|message| message.tool_call_id.is_some()));
}
```

- [ ] **Step 2: Run tests and verify the legacy runner behavior fails**

Run:

```bash
cargo test -p agent agent_runtime::tests -- --nocapture
cargo test -p agent exec::subagents::tests -- --nocapture
```

Expected: compile failure for the new harness and lifecycle APIs; existing runner keeps completed threads resident.

- [ ] **Step 3: Implement structured `fork_turns` in SessionStore**

Add a recent-turn fork API that copies complete stored rows, including tool metadata:

```rust
impl SessionStore {
    pub fn fork_session_recent_turns(
        &self,
        source_id: &str,
        new_id: &str,
        recent_turns: Option<usize>,
    ) -> anyhow::Result<()> {
        let messages = self.get_messages(source_id)?;
        let start = match recent_turns {
            None => 0,
            Some(0) => messages.len(),
            Some(count) => messages
                .iter()
                .enumerate()
                .rev()
                .filter(|(_, message)| message.role == "user")
                .nth(count.saturating_sub(1))
                .map(|(index, _)| index)
                .unwrap_or(0),
        };
        self.copy_session_message_rows(source_id, new_id, &messages[start..])
    }
}
```

`fork_turns = "all"` maps to `None`, `"none"` maps to `Some(0)`, and a positive integer maps to `Some(value)`. Reject zero strings and malformed values at the tool schema boundary. The copied rows retain `tool_calls`, `tool_call_id`, reasoning, compressed content, Codex items, media, and parent session lineage.

- [ ] **Step 4: Implement the root control directory and active-turn runtime manager**

The process directory lets backend sessions and Tauri commands find the same control without creating a second root controller:

```rust
pub struct AgentControlDirectory {
    controls: Mutex<HashMap<String, Weak<subagents::AgentControl>>>,
    store_factory: Arc<dyn Fn() -> anyhow::Result<subagents::AgentGraphStore> + Send + Sync>,
}

impl AgentControlDirectory {
    pub fn global() -> &'static Self;
    pub fn open_root(&self, root_session_id: &str) -> anyhow::Result<Arc<subagents::AgentControl>>;
    pub fn get(&self, root_session_id: &str) -> Option<Arc<subagents::AgentControl>>;
}
```

The manager stores only active turns and acknowledgements:

```rust
pub struct AgentRuntimeManager {
    active: Mutex<HashMap<String, ActiveAgentTurn>>,
}

struct ActiveAgentTurn {
    interrupt: Arc<AgentThreadControl>,
    terminated: oneshot::Receiver<RunnerTermination>,
}

impl AgentRuntimeManager {
    pub async fn start_turn(&self, request: RunAgentTurnRequest) -> anyhow::Result<()>;
    pub async fn interrupt(&self, thread_id: &str) -> anyhow::Result<AgentStatus>;
    pub async fn terminate(&self, thread_id: &str) -> anyhow::Result<()>;
    pub fn is_running(&self, thread_id: &str) -> bool;
}
```

Each turn builds a Session from `state.db`, runs one model/tool turn, emits `TurnStarted`, then exactly one of `TurnCompleted`, `TurnInterrupted`, or `TurnErrored`, releases the execution permit, and drops the resident runtime.

- [ ] **Step 5: Share the root control through Session services**

Add root and child constructors that pass the same control while binding a different current path:

```rust
pub(crate) struct SessionServices {
    pub(crate) sessions: SharedConversationStore,
    pub(crate) compression_policy: Mutex<Box<dyn CompressionPolicy>>,
    pub(crate) agent_control: Arc<subagents::AgentControl>,
    pub(crate) agent_path: subagents::AgentPath,
}

impl Session {
    fn with_session_id_for_agent_thread(
        config: Config,
        session_id: String,
        agent_id: &str,
        control: Arc<subagents::AgentControl>,
        path: subagents::AgentPath,
    ) -> anyhow::Result<Self>;
}
```

Do not overwrite the concurrent SessionState refactor in `runtime/mod.rs`; integrate through the existing `clone_history`, `record_items`, and `replace_history` APIs present at execution time.

- [ ] **Step 6: Deliver follow-ups at safe boundaries and wake waits on steer**

At the start of each streaming iteration, drain ordered mailbox rows for the current thread and queue one structured context block. Mark rows delivered only after the Session history accepts them:

```rust
let mailbox_items = agent.services.agent_control.drain_mailbox(&agent.services.agent_path)?;
if !mailbox_items.is_empty() {
    agent.record_items(mailbox_items_to_messages(&mailbox_items)).await;
    agent.services.agent_control.ack_mailbox(&agent.services.agent_path, mailbox_items.last().unwrap().sequence)?;
}
```

When `start_or_steer_turn` steers an active main turn, call `agent_control.notify_main_steer()` after the steer input is durably accepted.

- [ ] **Step 7: Run focused runtime tests and commit**

Run:

```bash
cargo test -p agent agent_runtime::tests -- --nocapture
cargo test -p agent exec::subagents::tests -- --nocapture
cargo test -p agent runtime::tests -- --nocapture
cargo test -p session fork_session_recent_turns -- --nocapture
```

Expected: event-derived statuses, slot release, interruption recovery, lazy Session history, and safe-boundary delivery pass.

```bash
git add crates/agent-core/src/exec/agent_control_directory.rs crates/agent-core/src/exec/agent_runtime.rs crates/agent-core/src/exec/subagents.rs crates/agent-core/src/exec/mod.rs crates/agent-core/src/runtime/session_services.rs crates/agent-core/src/runtime/mod.rs crates/agent-core/src/runtime/turn_lifecycle.rs crates/agent-core/src/streaming/maintenance.rs crates/agent-session/src/store/messages.rs
git commit -m "refactor(agent): run codex v2 agent turns from shared control"
```

### Task 6: Expose exactly six strict Codex V2 model tools

**Files:**
- Rewrite: `crates/agent-tools/src/engine/execution.rs`
- Rewrite: `crates/agent-tools/src/builtin/agents/subagent.rs`
- Modify: `crates/agent-tools/src/lib.rs`
- Modify: `crates/agent-tools/tests/tools_test.rs`
- Rewrite: `crates/agent-core/src/exec/dispatch.rs`
- Test: `crates/agent-tools/src/builtin/agents/subagent.rs`
- Test: `crates/agent-core/src/exec/dispatch.rs`

- [ ] **Step 1: Write failing catalog and schema tests**

```rust
#[test]
fn registers_only_codex_v2_agent_tools() {
    let registry = registry_with_all_tools();
    let names = registry.toolset("subagents").unwrap().items.iter().map(|item| item.id.as_str()).collect::<Vec<_>>();
    assert_eq!(names, vec![
        "spawn_agent", "list_agents", "send_message",
        "followup_task", "wait_agent", "interrupt_agent",
    ]);
    for removed in ["read_agent", "close_agent", "send_message_to_agent", "wait_agents"] {
        assert!(registry.get(removed).is_none(), "legacy tool remains: {removed}");
    }
}

#[test]
fn v2_tool_arguments_reject_legacy_aliases() {
    assert!(parse_args::<SpawnAgentArgs>(json!({"task": "x"})).is_err());
    assert!(parse_args::<TargetArgs>(json!({"thread_id": "x"})).is_err());
    assert!(parse_args::<WaitAgentArgs>(json!({"thread_ids": ["x"]})).is_err());
}

#[tokio::test]
async fn spawn_inherits_custom_agent_layers_without_expanding_parent_permissions() {
    let harness = DispatchHarness::with_parent_permission("workspace-write");
    harness.write_project_agent(
        "reviewer",
        r#"name = "reviewer"
description = "review"
developer_instructions = "review carefully"
sandbox_mode = "danger-full-access"

[mcp_servers.docs]
url = "https://example.invalid/mcp"

[[skills.config]]
path = "skills/review/SKILL.md"
enabled = true
"#,
    );
    let spawned = harness.spawn("review", "review the patch", Some("reviewer")).await.unwrap();
    assert_eq!(spawned.permission_profile, "workspace-write");
    assert_eq!(spawned.mcp_server_ids, vec!["docs"]);
    assert_eq!(spawned.enabled_skill_paths.len(), 1);
}
```

- [ ] **Step 2: Run focused tests and verify legacy tools make them fail**

Run:

```bash
cargo test -p tools registers_only_codex_v2_agent_tools -- --nocapture
cargo test -p tools v2_tool_arguments_reject_legacy_aliases -- --nocapture
```

Expected: failures show ten registered tools and accepted aliases.

- [ ] **Step 3: Replace the dispatch trait with V2 operations**

```rust
#[async_trait]
pub trait AgentThreadDispatch: Send + Sync {
    async fn spawn_agent(&self, request: SpawnAgentRequest) -> anyhow::Result<SpawnAgentResult>;
    async fn list_agents(&self, request: ListAgentsRequest) -> anyhow::Result<Vec<AgentThread>>;
    async fn send_message(&self, request: MessageAgentRequest) -> anyhow::Result<MessageAgentResult>;
    async fn followup_task(&self, request: MessageAgentRequest) -> anyhow::Result<MessageAgentResult>;
    async fn wait_agent(&self, request: WaitAgentRequest) -> anyhow::Result<WaitAgentResult>;
    async fn interrupt_agent(&self, request: InterruptAgentRequest) -> anyhow::Result<InterruptAgentResult>;
    fn notify_main_steer(&self);
}
```

Desktop read/close must not appear on this trait. Put them on a separate `DesktopAgentThreadControl` implemented in `agent-core/src/exec/dispatch.rs`.

- [ ] **Step 4: Implement strict schemas and semantics**

Use `#[serde(deny_unknown_fields)]` on every input:

```rust
struct SpawnAgentArgs {
    task_name: String,
    message: String,
    agent_type: Option<String>,
    model: Option<String>,
    reasoning_effort: Option<String>,
    fork_turns: Option<String>,
}

struct ListAgentsArgs { path_prefix: Option<String> }
struct MessageArgs { target: String, message: String }
struct WaitAgentArgs { timeout_ms: Option<i64> }
struct InterruptAgentArgs { target: String }
```

Match each tool to its own dispatch method. `send_message` calls queue-only; `followup_task` calls trigger-turn. Use the current Codex V2 defaults `min=10_000ms`, `default=30_000ms`, and `max=3_600_000ms`: reject values above max, clamp lower values to min, and return only `{ message, timed_out }`.

- [ ] **Step 5: Implement session-bound dispatch behavior**

`DefaultAgentThreadDispatch` contains `Arc<AgentControl>`, current `AgentPath`, current thread id, and the runtime manager. `list_agents` queries the entire root tree; `interrupt_agent` rejects root/self and returns the previous status; `spawn_agent` commits its reservation only after Session/runtime setup succeeds. Resolve custom-agent model, reasoning, sandbox, MCP, and skill layers before creating the child Session; pass those values through `SpawnRuntimeRequest`, and enforce sandbox narrowing against the parent profile.

- [ ] **Step 6: Run tools/core tests and commit**

Run:

```bash
cargo test -p tools -- --nocapture
cargo test -p agent exec::dispatch::tests -- --nocapture
```

Expected: only six tools are visible; old names are absent; old args fail; send/followup/wait/interrupt behavior passes.

```bash
git add crates/agent-tools/src/engine/execution.rs crates/agent-tools/src/builtin/agents/subagent.rs crates/agent-tools/src/lib.rs crates/agent-tools/tests/tools_test.rs crates/agent-core/src/exec/dispatch.rs
git commit -m "refactor(tools): make codex v2 the only agent tool contract"
```

### Task 7: Add restart recovery, recursive close, and real Session reads

**Files:**
- Modify: `crates/agent-subagents/src/control.rs`
- Modify: `crates/agent-subagents/src/store.rs`
- Modify: `crates/agent-core/src/exec/agent_runtime.rs`
- Modify: `crates/agent-core/src/exec/dispatch.rs`
- Rewrite: `apps/desktop/src-tauri/src/commands/subagents.rs`
- Test: `crates/agent-subagents/src/control.rs`
- Test: `crates/agent-core/src/exec/dispatch.rs`

- [ ] **Step 1: Write failing recovery and desktop-control tests**

```rust
#[test]
fn restart_converts_running_turn_to_interrupted_event() {
    let store = store_with_running_thread();
    let control = AgentControl::recover(store.clone(), "root").unwrap();
    let thread = control.resolve_target(&AgentPath::root(), "/root/worker").unwrap();
    assert_eq!(thread.status, AgentStatus::Interrupted);
    assert_eq!(store.status_events(&thread.thread_id).unwrap().last().unwrap().event_kind, "turn_interrupted");
}

#[tokio::test]
async fn close_subtree_waits_for_children_and_is_idempotent() {
    let harness = nested_runtime_harness();
    harness.desktop.close_subtree("/root/parent").await.unwrap();
    harness.desktop.close_subtree("/root/parent").await.unwrap();
    assert!(harness.subtree("/root/parent").iter().all(|thread| thread.status == AgentStatus::Shutdown));
    assert_eq!(harness.active_runtime_count(), 0);
}

#[test]
fn desktop_read_returns_session_store_tool_timeline() {
    let harness = desktop_harness_with_tool_call();
    let detail = harness.desktop.read_thread("/root/worker").unwrap();
    assert!(detail.messages.iter().any(|message| message.role == "tool"));
}
```

- [ ] **Step 2: Run focused tests and verify they fail**

Run:

```bash
cargo test -p subagents restart_converts_running_turn_to_interrupted_event -- --nocapture
cargo test -p agent close_subtree_waits_for_children_and_is_idempotent -- --nocapture
cargo test -p agent desktop_read_returns_session_store_tool_timeline -- --nocapture
```

Expected: failures expose optimistic status writes, non-recursive close, and simplified transcript reads.

- [ ] **Step 3: Implement recovery and desktop-only operations**

Recovery appends a durable `turn_interrupted` event for each stored `Running` thread and never restarts an LLM/tool call automatically.

Desktop control methods:

```rust
#[async_trait]
pub trait DesktopAgentThreadControl {
    async fn snapshot(&self, root_session_id: &str) -> anyhow::Result<AgentTreeSnapshot>;
    async fn read_thread(&self, root_session_id: &str, target: &str) -> anyhow::Result<AgentThreadDetail>;
    async fn followup(&self, root_session_id: &str, target: &str, message: String) -> anyhow::Result<AgentThread>;
    async fn interrupt(&self, root_session_id: &str, target: &str) -> anyhow::Result<InterruptAgentResult>;
    async fn close_subtree(&self, root_session_id: &str, target: &str) -> anyhow::Result<AgentTreeSnapshot>;
}
```

Read messages via `SessionStore::open_sessions_dir(&memory_dir.join("sessions"))?.get_messages(&thread.session_id)`. Close descendants leaf-first, await termination acknowledgements, then write closed edges and `Shutdown` events.

- [ ] **Step 4: Rewrite Tauri commands around root session and canonical target**

Use DTO inputs `{ rootSessionId, target }`; remove `includeClosed` and direct `threadId` compatibility arguments. Preserve command names used by the desktop shell, but their payload contract is desktop-only and canonical-path based.

- [ ] **Step 5: Run focused crates and commit**

Run:

```bash
cargo test -p subagents -- --nocapture
cargo test -p agent exec::dispatch::tests -- --nocapture
cargo check -p astro-agent
```

Expected: recovery, idempotent close, real timeline read, and Tauri compilation pass.

```bash
git add crates/agent-subagents/src/control.rs crates/agent-subagents/src/store.rs crates/agent-core/src/exec/agent_runtime.rs crates/agent-core/src/exec/dispatch.rs apps/desktop/src-tauri/src/commands/subagents.rs
git commit -m "feat(desktop): add v2 agent thread control plane"
```

### Task 8: Stream Agent Thread activity through Session Events

**Files:**
- Modify: `crates/agent-proto/proto/astro.proto`
- Modify: `crates/agent-server/src/session_events.rs`
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Modify: `apps/desktop/src-tauri/src/infra/session_events.rs`
- Test: `crates/agent-server/src/session_events.rs`
- Test: `crates/agent-server/tests/agent_thread_events.rs`

- [ ] **Step 1: Write failing replay/filter tests**

```rust
#[tokio::test]
async fn agent_thread_events_replay_after_cursor_in_order() {
    let hub = SessionEventHub::new(16);
    hub.publish(agent_thread_event("root", 7, "/root/a", "running"));
    hub.publish(agent_thread_event("root", 8, "/root/a", "completed"));
    let mut rx = hub.subscribe(session_filter("root"), hub.stream_id(), 1);
    let event = rx.recv().await.unwrap();
    assert_eq!(event.event.agent_thread_changed.as_ref().unwrap().activity_sequence, 8);
}

#[tokio::test]
async fn one_root_watcher_publishes_runner_status_changes() {
    let harness = service_harness();
    harness.emit_agent_status("root", "/root/worker", AgentStatus::Running);
    let event = harness.next_session_event("root").await;
    assert_eq!(event.agent_thread_changed.unwrap().canonical_path, "/root/worker");
}
```

- [ ] **Step 2: Run the server tests and verify the payload is absent**

Run:

```bash
cargo test -p server agent_thread_events_replay_after_cursor_in_order -- --nocapture
cargo test -p server one_root_watcher_publishes_runner_status_changes -- --nocapture
```

Expected: compile failure because the proto and event hub lack the Agent Thread payload.

- [ ] **Step 3: Extend the proto and internal event message**

Add a payload that contains one complete thread projection so frontend reducers never infer missing state:

```proto
message AgentThreadChangedEvent {
  uint64 activity_sequence = 1;
  string root_thread_id = 2;
  string thread_id = 3;
  string parent_thread_id = 4;
  string canonical_path = 5;
  string task_name = 6;
  string agent_type = 7;
  string session_id = 8;
  string status_kind = 9;
  string status_payload_json = 10;
  string activity_kind = 11;
}
```

Add `agent_thread_changed = 13` to `SessionEvent.payload` and the matching internal payload/DTO.

- [ ] **Step 4: Attach one activity watcher per root**

When AstroService creates or resumes a root Session, subscribe once to its `AgentControl` activity bus. Publish each projection to `SessionEventHub` with `session_id=root_thread_id`. Track attached roots in a mutex-protected set and remove them when the watcher ends.

- [ ] **Step 5: Bridge the payload to the desktop and run tests**

Extend `SessionEventDto` with `agent_thread_changed: Option<AgentThreadChangedDto>` and map the proto payload in `proto_to_dto`.

Run:

```bash
cargo test -p server session_events -- --nocapture
cargo test -p server --test agent_thread_events -- --nocapture
cargo check -p astro-agent
```

Expected: sequence replay, filtering, watcher deduplication, proto mapping, and desktop bridge compile pass.

- [ ] **Step 6: Commit the event stream**

```bash
git add crates/agent-proto/proto/astro.proto crates/agent-server/src/session_events.rs crates/agent-server/src/grpc/astro_service.rs crates/agent-server/tests/agent_thread_events.rs apps/desktop/src-tauri/src/infra/session_events.rs
git commit -m "feat(events): stream codex v2 agent thread activity"
```

### Task 9: Replace frontend polling with snapshot plus incremental Agent Tree events

**Files:**
- Create: `apps/desktop/src/hooks/chat/subagentTree.ts`
- Create: `apps/desktop/src/hooks/chat/subagentTree.test.ts`
- Rewrite: `apps/desktop/src/hooks/chat/useSubagentThreads.ts`
- Modify: `apps/desktop/src/components/chat/SubagentActivityBar.tsx`
- Modify: `apps/desktop/src/components/chat/SubagentsPanel.tsx`
- Modify: `apps/desktop/src/components/chat/ChatView.tsx`
- Modify: `apps/desktop/src/styles/features/chat/subagents.css`
- Modify: `apps/desktop/src/i18n/messages.ts`

- [ ] **Step 1: Write failing pure reducer tests**

```typescript
it("builds a stable nested tree from a snapshot", () => {
  const state = fromSnapshot(snapshot([
    thread("/root/research", "running"),
    thread("/root/research/citations", "completed"),
  ]));
  expect(state.roots[0].children[0].thread.canonicalPath).toBe("/root/research/citations");
});

it("ignores duplicate and out-of-order activity sequences", () => {
  const initial = fromSnapshot(snapshot([thread("/root/a", "running")], 10));
  const completed = reduceAgentThreadEvent(initial, changed("/root/a", "completed", 11));
  expect(reduceAgentThreadEvent(completed, changed("/root/a", "running", 10))).toBe(completed);
});

it("marks mailbox and final activity unread until the thread is opened", () => {
  const initial = fromSnapshot(snapshot([thread("/root/a", "running")]));
  const changedState = reduceAgentThreadEvent(initial, changed("/root/a", "completed", 2, "result"));
  expect(changedState.byPath["/root/a"].unread).toBe(true);
  expect(markThreadRead(changedState, "/root/a").byPath["/root/a"].unread).toBe(false);
});
```

- [ ] **Step 2: Run the reducer test and verify it fails**

Run: `cd apps/desktop && npm test -- src/hooks/chat/subagentTree.test.ts`

Expected: failure because the reducer module does not exist.

- [ ] **Step 3: Implement the pure tree projection**

Define camelCase frontend DTOs and pure functions:

```typescript
export type AgentThreadStatus =
  | { kind: "pending_init" }
  | { kind: "running" }
  | { kind: "interrupted" }
  | { kind: "completed"; payload: { lastMessage: string } }
  | { kind: "errored"; payload: { message: string } }
  | { kind: "shutdown" };

export function fromSnapshot(snapshot: AgentTreeSnapshot): AgentTreeState;
export function reduceAgentThreadEvent(state: AgentTreeState, event: AgentThreadChanged): AgentTreeState;
export function markThreadRead(state: AgentTreeState, path: string): AgentTreeState;
```

Sort siblings by `canonicalPath`; derive nesting only from `parentThreadId`/path and never from arrival order.

- [ ] **Step 4: Rewrite the hook without timers**

On root session change:

1. invoke `list_subagent_threads` once for a snapshot;
2. listen to `session_event` immediately;
3. buffer matching events until the snapshot resolves;
4. apply buffered events with sequence greater than the snapshot cursor;
5. on stream-id change, request a new snapshot before applying new-generation events.

The resulting hook returns `{ state, threads, roots, error, refresh, markRead }`. Remove both 1.5-second intervals from the hook and detail panel.

- [ ] **Step 5: Render hierarchy and real Session details**

Activity bar:

- render nodes recursively with `depth` CSS variable;
- count only `status.kind === "running"` as running;
- show unread activity;
- interrupt active turns through the desktop command.

Panel:

- request `read_subagent_thread` only when selection changes or a matching event arrives;
- render user/assistant/tool messages from SessionStore;
- route composer submission to desktop follow-up;
- show interrupt for Running and close for every non-Shutdown V2 thread;
- render historical archive rows read-only.

- [ ] **Step 6: Run frontend tests, typecheck, and commit**

Run:

```bash
cd apps/desktop && npm test -- src/hooks/chat/subagentTree.test.ts
cd apps/desktop && npm test
cd apps/desktop && npx tsc --noEmit
```

Expected: reducer and existing frontend tests pass; TypeScript reports no errors; no `setInterval` remains in Subagent files.

```bash
git add apps/desktop/src/hooks/chat/subagentTree.ts apps/desktop/src/hooks/chat/subagentTree.test.ts apps/desktop/src/hooks/chat/useSubagentThreads.ts apps/desktop/src/components/chat/SubagentActivityBar.tsx apps/desktop/src/components/chat/SubagentsPanel.tsx apps/desktop/src/components/chat/ChatView.tsx apps/desktop/src/styles/features/chat/subagents.css apps/desktop/src/i18n/messages.ts
git commit -m "feat(desktop): render live codex v2 agent tree"
```

### Task 10: Remove every legacy tool/config reference and update documentation

**Files:**
- Modify: `crates/agent-core/src/prompt/interaction_mode.rs`
- Modify: `crates/agent-core/src/prompt/context_usage.rs`
- Modify: files returned by the legacy-name scan under `crates/agent-core`, `crates/agent-tools`, and `apps/desktop/src`
- Rewrite: `docs/subagents.md`
- Modify: `AGENTS.md`
- Test: `crates/agent-tools/tests/tools_test.rs`

- [ ] **Step 1: Add a failing source-level legacy scan test**

Extend the tools integration test with the complete removed set and add a repository check command:

```rust
for removed in ["read_agent", "close_agent", "send_message_to_agent", "wait_agents"] {
    assert!(!tool_names.contains(&removed), "legacy tool registered: {removed}");
}
```

Run:

```bash
rg -n 'read_agent|close_agent|send_message_to_agent|wait_agents|include_closed|thread_ids|\.astro/agents' \
  crates/agent-core crates/agent-subagents crates/agent-tools apps/desktop/src apps/desktop/src-tauri/src docs/subagents.md AGENTS.md
```

Expected: the scan returns legacy production, prompt, UI, documentation, and old-test references.

- [ ] **Step 2: Remove production and prompt references**

Delete old match arms, structs, trait methods, fallback catalog items, context-usage grouping, and guidance text. Do not replace removed model tools with deprecated wrappers.

Keep `read_subagent_thread` and `close_subagent_thread` only as explicit Tauri desktop command names. Their DTOs must use V2 root/target fields and their Rust implementations must call `DesktopAgentThreadControl`, never the model dispatch trait.

- [ ] **Step 3: Rewrite the canonical project documentation**

`docs/subagents.md` and `AGENTS.md` must state:

- six model tools only;
- read/close are desktop control-plane operations;
- statuses are PendingInit/Running/Interrupted/Completed/Errored/Shutdown;
- `send_message` is queue-only and `followup_task` triggers/resumes turns;
- `wait_agent` waits for any mailbox/final/steer activity;
- only `.codex/agents` and `.codex/config.toml` configure custom agents;
- `~/.astro/subagents.db` remains Astro runtime storage.

- [ ] **Step 4: Verify the legacy scan has only intentional history/design mentions**

Run the same `rg` command. Expected remaining matches:

- migration code referring to v1 column/table names;
- the approved design and implementation plan explaining what was removed;
- Tauri desktop command identifiers for read/close.

No model registry, prompt, schema, runtime branch, compatibility loader, or frontend fallback catalog match may remain.

- [ ] **Step 5: Run focused tests and commit**

Run:

```bash
cargo test -p tools -- --nocapture
cargo test -p subagents -- --nocapture
cd apps/desktop && npx tsc --noEmit
```

Expected: all pass.

```bash
git add crates/agent-core crates/agent-subagents crates/agent-tools apps/desktop/src apps/desktop/src-tauri/src docs/subagents.md AGENTS.md
git commit -m "docs(subagents): remove legacy agent thread contract"
```

Before this commit, inspect `git diff --cached --name-only` and unstage every unrelated concurrent file. Stage explicit paths when the workspace contains user changes.

### Task 11: Run end-to-end recovery and contract verification

**Files:**
- Create: `crates/agent-subagents/tests/v2_lifecycle.rs`
- Create: `crates/agent-server/tests/agent_thread_recovery.rs`
- Modify: implementation files only when a failing acceptance test exposes a defect

- [ ] **Step 1: Write the cross-layer lifecycle acceptance test**

Cover this exact sequence:

```rust
#[tokio::test]
async fn nested_agent_tree_survives_interrupt_restart_resume_and_recursive_close() {
    let app = TestApp::new();
    let research = app.spawn("/root", "research", "collect sources").await.unwrap();
    let citations = app.spawn("/root/research", "citations", "check citations").await.unwrap();
    app.send_message("/root/research/citations", "queued context").await.unwrap();
    assert!(!app.is_running(&citations));
    app.followup("/root/research/citations", "continue").await.unwrap();
    app.interrupt("/root/research/citations").await.unwrap();
    app.restart().await;
    assert_eq!(app.status("/root/research/citations"), AgentStatusKind::Interrupted);
    app.followup("/root/research/citations", "resume after restart").await.unwrap();
    app.wait_for_status("/root/research/citations", AgentStatusKind::Completed).await;
    app.close_subtree("/root/research").await.unwrap();
    assert_eq!(app.status(&research), AgentStatusKind::Shutdown);
    assert_eq!(app.status(&citations), AgentStatusKind::Shutdown);
}
```

- [ ] **Step 2: Run the acceptance test and fix defects through red-green cycles**

Run: `cargo test -p subagents --test v2_lifecycle -- --nocapture`

Expected before fixes: any remaining cross-module defect produces a focused assertion failure. For each defect, add the smallest reproducing test beside the owning module, observe failure, implement the fix, and rerun both tests.

- [ ] **Step 3: Verify backend event reconnect behavior**

Run: `cargo test -p server --test agent_thread_recovery -- --nocapture`

Expected: after simulated disconnect, snapshot cursor plus replayed events yield the same final tree as an uninterrupted subscription.

- [ ] **Step 4: Run the complete verification matrix**

```bash
cargo fmt --all --check
cargo test -p subagents
cargo test -p tools
cargo test -p agent
cargo test -p server
cargo check --workspace --all-targets
cargo test --workspace
cd apps/desktop && npm test
cd apps/desktop && npx tsc --noEmit
cd apps/desktop && npm run build
git diff --check
```

Expected: every command exits zero. If a pre-existing failure appears, capture its exact command/output and prove it reproduces on the worktree base before classifying it as baseline.

- [ ] **Step 5: Commit acceptance coverage**

```bash
git add crates/agent-subagents/tests/v2_lifecycle.rs crates/agent-server/tests/agent_thread_recovery.rs
git commit -m "test(subagents): cover codex v2 thread lifecycle"
```

### Task 12: Final contract audit and handoff

**Files:**
- Verify only; modify the owning file when the audit finds a mismatch

- [ ] **Step 1: Compare the six schemas to local Codex source**

Inspect:

```bash
sed -n '1130,1195p' /Users/iswm/CodeRope/codex/codex-rs/core/src/tools/spec_plan.rs
sed -n '1,220p' /Users/iswm/CodeRope/codex/codex-rs/core/src/tools/handlers/multi_agents_v2/message_tool.rs
sed -n '1,230p' /Users/iswm/CodeRope/codex/codex-rs/core/src/tools/handlers/multi_agents_v2/wait.rs
sed -n '430,670p' /Users/iswm/CodeRope/codex/codex-rs/core/src/agent/control.rs
```

Verify names, required fields, unknown-field rejection, queue-only versus trigger-turn behavior, list prefix resolution, wait outcome, interrupt previous-status return, and root/self restrictions.

- [ ] **Step 2: Audit resource and status invariants**

Confirm from tests and source:

- no API writes `Completed`, `Interrupted`, `Errored`, or `Shutdown` before a runner/runtime acknowledgement;
- every spawn failure releases path and identity reservations;
- every turn exit releases its execution permit;
- completed/interrupted/errored threads remain addressable;
- close waits for termination and is idempotent;
- mailbox acknowledgment occurs after Session history accepts messages.

- [ ] **Step 3: Audit frontend event ownership**

Run:

```bash
rg -n 'setInterval|setTimeout' apps/desktop/src/hooks/chat/useSubagentThreads.ts apps/desktop/src/components/chat/SubagentActivityBar.tsx apps/desktop/src/components/chat/SubagentsPanel.tsx
```

Expected: no polling timer. Confirm a single hook owns snapshot/event state and both UI surfaces consume the same projection.

- [ ] **Step 4: Inspect final history and worktree status**

```bash
git log --oneline --decorate -15
git status --short
git diff --check HEAD~12..HEAD
```

Expected: focused commits, clean implementation worktree, no unrelated user files, no whitespace errors.

- [ ] **Step 5: Present merge and cleanup choices**

Report the worktree path, branch, commits, verification results, baseline exceptions, and any retained historical migration data. Ask the user whether to merge the branch and remove the worktree; do not merge or delete the worktree before approval.
