# Codex Hooks B1 Unified Dispatch and Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route Astro Agent lifecycle hooks through one canonical `HookRuntime::dispatch` path and align the main-turn `SessionStart`, `UserPromptSubmit`, `Stop`, and `AgentEnd` behavior with the approved Codex contract.

**Architecture:** `Session` owns an `Arc<HookRuntime>` instead of only a Plugin bus, enriches every **Session-owned** payload with stable session/model/cwd/permission fields, and delegates those events to one dispatcher that reaches Plugin, Gateway, and Shell exactly once. `SubagentStart` / `SubagentStop` remain documented Plugin-only direct-fire exceptions. The main lifecycle retries blocked `SessionStart` admission with the same source, consumes that source once on its first non-`Block` outcome, evaluates prompt hooks before persistence, evaluates Stop for every terminal candidate with a bounded continuation loop, and emits one AgentEnd from `RegularTask` on both success and failure.

**Tech Stack:** Rust 2021 workspace, Tokio, `agent-hooks`, `agent-core`, `agent-server`, Cargo integration tests.

---

## Scope boundary

This plan implements B1 only:

- Session-owned lifecycle events use unified Plugin/Gateway/Shell dispatch;
- Session ownership of the shared runtime and common payload enrichment;
- `SessionStart(source=startup|resume)` once per in-memory Session after its first non-`Block` admission; a `Block` retains the source and retries;
- `UserPromptSubmit` block/context behavior before persistence;
- `Stop` on every terminal candidate, independent of disk writes;
- one authoritative `AgentEnd` per `RegularTask` run;
- removal of duplicate server-side SessionStart/AgentEnd/reset/finalize delivery;
- accurate B1 documentation.

The following remain separate B2/B3/C work:

- `SessionEnd`, clear/compact SessionStart sources, `PreCompact`, and `PostCompact`;
- Gateway/Shell transport for Plugin-only `SubagentStart` / `SubagentStop`, plus `SubagentStop` `KeepGoing` continuation;
- Codex command-hook schema, matcher aggregation, JSON stdin/stdout, timeout, trust, and discovery.

## File map

- `crates/agent-hooks/src/lib.rs`: authoritative normalization and three-transport dispatch.
- `crates/agent-core/src/runtime/mod.rs`: Session runtime ownership, compatibility accessors, and common HookInput enrichment.
- `crates/agent-core/src/runtime/session_state.rs`: one-shot startup/resume source state.
- `crates/agent-core/src/runtime/turn_lifecycle.rs`: SessionStart and UserPromptSubmit ordering/control flow.
- `crates/agent-core/src/tasks/regular.rs`: authoritative AgentEnd finalization.
- `crates/agent-core/src/streaming/multi_turn.rs`: Stop semantics and removal of the old AgentEnd fire point.
- `crates/agent-server/src/grpc/astro_service.rs`: inject the full runtime before steering and remove duplicate lifecycle dispatch.
- `crates/agent-core/tests/rig_agent_test.rs`: startup/resume, prompt block, prompt context, and payload tests.
- `crates/agent-core/tests/streaming_test.rs`: steering prompt hook, Stop continuation, and AgentEnd success/error tests.
- `docs/hooks.md`: current B1 runtime truth and remaining gaps.
- `docs/examples/hooks/{README.md,config.yaml.snippet,telemetry-webhook.sh}`: runnable Shell configuration and custom-webhook examples.

### Task 1: Add one canonical HookRuntime dispatcher

**Files:**
- Modify: `crates/agent-hooks/src/lib.rs:27-195`

- [ ] **Step 1: Write a failing dispatcher test**

Add a test that registers the same legacy-named event across Plugin, Gateway, and Shell, dispatches once, and proves that both observable buses receive the canonical name while the Plugin outcome is returned:

```rust
#[tokio::test]
async fn dispatch_normalizes_once_and_reaches_all_transports() {
    let dir = tempfile::tempdir().unwrap();
    let hook_dir = dir.path().join("hooks").join("audit");
    std::fs::create_dir_all(&hook_dir).unwrap();
    std::fs::write(
        hook_dir.join("HOOK.yaml"),
        "name: audit\nevents:\n  - pre_tool_call\n",
    )
    .unwrap();

    let rt = HookRuntime::new();
    rt.gateway.discover(dir.path()).unwrap();
    let plugin_hits = Arc::new(AtomicUsize::new(0));
    let plugin_counter = Arc::clone(&plugin_hits);
    rt.plugin.register(PRE_TOOL_USE, move |input| {
        assert_eq!(input.hook_event_name, PRE_TOOL_USE);
        plugin_counter.fetch_add(1, Ordering::SeqCst);
        HookOutcome::Block("blocked".into())
    });
    let gateway_hits = Arc::new(AtomicUsize::new(0));
    let gateway_counter = Arc::clone(&gateway_hits);
    rt.gateway.register_handler("audit", move |event, input| {
        assert_eq!(event, PRE_TOOL_USE);
        assert_eq!(input.hook_event_name, PRE_TOOL_USE);
        gateway_counter.fetch_add(1, Ordering::SeqCst);
    });
    *rt.shell.lock().unwrap() = ShellHookRunner::new(std::collections::HashMap::from([(
        "pre_tool_call".to_string(),
        "true".to_string(),
    )]));

    let outcome = rt.dispatch("pre_tool_call", &HookInput::default());

    assert!(matches!(outcome, HookOutcome::Block(ref reason) if reason == "blocked"));
    assert_eq!(plugin_hits.load(Ordering::SeqCst), 1);
    assert_eq!(gateway_hits.load(Ordering::SeqCst), 1);
    let shell_schedule = rt.shell.lock().unwrap().scheduled();
    assert_eq!(shell_schedule.len(), 1);
    assert_eq!(shell_schedule[0].0, PRE_TOOL_USE);
    assert_eq!(shell_schedule[0].1.hook_event_name, PRE_TOOL_USE);
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test -p hooks tests::dispatch_normalizes_once_and_reaches_all_transports -- --exact
```

Expected: compile failure because `HookRuntime::dispatch` does not exist.

- [ ] **Step 3: Implement the dispatcher and compatibility constructor**

Refactor construction and dispatch to this shape:

```rust
impl HookRuntime {
    pub fn new() -> Self {
        Self::with_plugin_bus(Arc::new(PluginHookBus::new()))
    }

    pub fn with_plugin_bus(plugin: Arc<PluginHookBus>) -> Self {
        let ui_slot = UiTimelineSlot::new();
        ui_slot.install(&plugin);
        Self {
            plugin,
            gateway: Arc::new(GatewayHookRegistry::default()),
            shell: Arc::new(std::sync::Mutex::new(ShellHookRunner::default())),
            ui_slot,
        }
    }

    pub fn dispatch(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        let name = normalize_hook_event_name(name);
        let payload = payload.normalized_for_event(name.as_ref());
        let outcome = self.plugin.fire(name.as_ref(), &payload);
        self.gateway.fire(name.as_ref(), &payload);
        if let Ok(shell) = self.shell.lock() {
            shell.fire_async(name.as_ref(), &payload);
        }
        outcome
    }

    pub fn fire_plugin(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        self.dispatch(name, payload)
    }

    pub fn fire_gateway(&self, name: &str, payload: &HookPayload) {
        let _ = self.dispatch(name, payload);
    }
}
```

Gateway and Shell remain observers in B1; the Plugin outcome remains the controlling result until the multi-handler aggregator lands in Batch C. New callers that need the Plugin outcome should call `dispatch` directly; `fire_gateway` retains its historical `()` return type for compatibility.

- [ ] **Step 4: Run the focused and crate tests**

Run:

```bash
cargo test -p hooks tests::dispatch_normalizes_once_and_reaches_all_transports -- --exact
cargo test -p hooks
```

Expected: the focused test passes and the Hooks crate reports all tests passing.

- [ ] **Step 5: Commit Task 1**

```bash
git add crates/agent-hooks/src/lib.rs
git commit -m "refactor(hooks): unify runtime dispatch"
```

### Task 2: Make Session own and enrich the shared HookRuntime

**Files:**
- Modify: `crates/agent-core/src/runtime/mod.rs:118-138,350-375,1350-1410`
- Test: `crates/agent-core/tests/rig_agent_test.rs`

- [ ] **Step 1: Write a failing common-payload test**

Add this integration test, including `std::sync::Mutex` in the imports if it is not already present:

```rust
#[tokio::test]
async fn session_fire_hook_uses_shared_runtime_and_common_payload() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    let runtime = Arc::new(::hooks::HookRuntime::new());
    let seen = Arc::new(std::sync::Mutex::new(None));
    let capture = Arc::clone(&seen);
    runtime.plugin.register(::hooks::USER_PROMPT_SUBMIT, move |input| {
        *capture.lock().unwrap() = Some(input.clone());
        ::hooks::HookOutcome::Continue
    });
    agent.set_hook_runtime(Arc::clone(&runtime));
    agent.set_project_root(Some(dir.path().join("workspace")));
    agent.set_permission_profile(Some("workspace-write".into()));
    agent.set_chat_credentials("openai", "gpt-5.6-sol", "key", "https://example.com");

    let _ = agent.fire_hook(
        ::hooks::USER_PROMPT_SUBMIT,
        ::hooks::HookPayload {
            prompt: Some("hello".into()),
            ..Default::default()
        },
    );

    let input = seen.lock().unwrap().clone().unwrap();
    assert_eq!(input.session_id, agent.session_id());
    assert_eq!(input.cwd, dir.path().join("workspace").display().to_string());
    assert_eq!(input.model, "openai/gpt-5.6-sol");
    assert_eq!(input.permission_mode.as_deref(), Some("workspace-write"));
    assert_eq!(input.hook_event_name, ::hooks::USER_PROMPT_SUBMIT);
    assert!(Arc::ptr_eq(&agent.hook_runtime(), &runtime));
}
```

- [ ] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test -p agent session_fire_hook_uses_shared_runtime_and_common_payload -- --exact
```

Expected: compile failure because `set_hook_runtime` and `hook_runtime` do not exist.

- [ ] **Step 3: Replace the SessionConfiguration Plugin bus with HookRuntime**

Change the field and default:

```rust
pub(crate) hook_runtime: Arc<::hooks::HookRuntime>,
```

```rust
hook_runtime: Arc::new(::hooks::HookRuntime::new()),
```

Add the full-runtime API while retaining Plugin-only compatibility:

```rust
pub fn set_hook_runtime(&self, runtime: Arc<::hooks::HookRuntime>) {
    self.session_configuration_mut().hook_runtime = runtime;
}

pub fn hook_runtime(&self) -> Arc<::hooks::HookRuntime> {
    Arc::clone(&self.session_configuration().hook_runtime)
}

pub fn set_hook_bus(&self, bus: Arc<::hooks::PluginHookBus>) {
    self.session_configuration_mut().replace_hook_bus(bus);
}

pub fn hook_bus(&self) -> Arc<::hooks::PluginHookBus> {
    Arc::clone(&self.session_configuration().hook_runtime.plugin)
}
```

`SessionConfiguration::replace_hook_bus(&mut self, bus)` performs the
idempotent compatibility replacement inside the single configuration write
guard: it swaps only the plugin bus while preserving the current Gateway,
Shell, and UI slot transports. It must not call `ui_slot.install`; callers that
create a standalone `HookRuntime` continue to use `HookRuntime::new` or
`with_plugin_bus`.

Add payload enrichment and route `fire_hook` through the dispatcher:

```rust
fn enrich_hook_payload(&self, mut payload: ::hooks::HookPayload) -> ::hooks::HookPayload {
    let configuration = self.session_configuration();
    if payload.session_id.is_empty() {
        payload.session_id = self.session_id.clone();
    }
    if payload.cwd.is_empty() {
        payload.cwd = configuration
            .project_root
            .as_deref()
            .unwrap_or(self.memory_dir())
            .display()
            .to_string();
    }
    if payload.model.is_empty() {
        let target = configuration.model_ctx.primary_chat_target();
        payload.model = match (target.backend_id.trim(), target.model.trim()) {
            ("", model) => model.to_string(),
            (backend, "") => backend.to_string(),
            (backend, model) => format!("{backend}/{model}"),
        };
    }
    if payload.permission_mode.is_none() {
        payload.permission_mode = configuration.permission_profile.clone();
    }
    payload
}

pub fn fire_hook(&self, name: &str, payload: ::hooks::HookPayload) -> ::hooks::HookOutcome {
    self.hook_runtime()
        .dispatch(name, &self.enrich_hook_payload(payload))
}
```

Update the compile-only Arc API tests to exercise `set_hook_runtime` and `hook_runtime` as owned snapshots.

- [ ] **Step 4: Run focused tests and the Agent library tests**

Run:

```bash
cargo test -p agent session_fire_hook_uses_shared_runtime_and_common_payload -- --exact
cargo test -p agent --lib
```

Expected: the focused test and all Agent library tests pass.

- [ ] **Step 5: Commit Task 2**

```bash
git add crates/agent-core/src/runtime/mod.rs crates/agent-core/tests/rig_agent_test.rs
git commit -m "refactor(agent): own shared hook runtime"
```

### Task 3: Align SessionStart and UserPromptSubmit lifecycle

**Files:**
- Modify: `crates/agent-core/src/runtime/mod.rs`
- Modify: `crates/agent-core/src/runtime/session_state.rs:14-63`
- Modify: `crates/agent-core/src/runtime/turn_context.rs`
- Modify: `crates/agent-core/src/runtime/turn_lifecycle.rs:69-220`
- Modify: `crates/agent-core/src/runtime/system_prompt.rs`
- Modify: `crates/agent-core/src/streaming/multi_turn.rs`
- Test: `crates/agent-core/tests/rig_agent_test.rs`
- Test: `crates/agent-core/tests/streaming_test.rs:110-205`
- Modify: `crates/agent-server/src/grpc/astro_service.rs` (adapt the steering `Result`, inject the full runtime before steering, and remove the duplicate server SessionStart path)

The quality-review correction is authoritative for this task:

- steering uses an RAII `TurnInputReservation`; terminal close waits for in-flight admissions, so a fired hook cannot race with queue closure;
- initial and pending input slices are coalesced into one logical user message (`\n\n` text join plus stable image flattening) before their single persistence write;
- initial SessionStart/UserPromptSubmit contexts are staged locally, discarded on any later Block, and enter `assemble_system_layers` through its budgeted inject layer;
- steering InjectContext remains a next-sampling message-side context and is committed to Session state before its input reservation wakes terminal close;
- a session-scoped FIFO `admission_lock` serializes the complete initial SessionStart/UserPromptSubmit admission sequence and each steer admission. Steering clones the active TurnContext first, then acquires this lock before reserving; a queue closed while waiting returns `Ok(None)` without firing a hook;
- the same admission lock makes pending SessionStart consumption strictly one-shot under concurrent compatibility admission. It is deliberately released before begin/reload/persistence and never substitutes for the conversation write lock;
- `run_turn` does not drain pending input at the first loop top. `has_sampled` becomes true only after `run_sampling_request` succeeds, so an early steer stays queued until the first assistant is recorded; the no-tool terminal branch then records it as the next user turn. Later tool-loop iterations may drain pending input at the top because assistant/tool history already separates the roles;
- a blocked SessionStart retains its pending `startup`/`resume` source for retry; any non-Block outcome consumes the expected source. A later prompt Block or infrastructure/reload failure does not restore SessionStart after it has successfully fired;
- server full-runtime injection and duplicate SessionStart removal move forward from Task 5; AgentEnd/reset/finalize cleanup remains Task 5.

- [ ] **Step 1: Write failing startup, resume, control, context, and steering tests**

Replace `test_prompt_hooks_on_run_turn` with a test that captures exact ordering and one-shot behavior:

```rust
#[tokio::test]
async fn session_start_fires_once_before_each_user_prompt() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    let events = Arc::new(std::sync::Mutex::new(Vec::new()));
    let session_events = Arc::clone(&events);
    agent.hook_bus().register(::hooks::SESSION_START, move |input| {
        session_events
            .lock()
            .unwrap()
            .push(format!("{}:{}", input.hook_event_name, input.source.as_deref().unwrap_or("")));
        ::hooks::HookOutcome::Continue
    });
    let prompt_events = Arc::clone(&events);
    agent.hook_bus().register(::hooks::USER_PROMPT_SUBMIT, move |input| {
        prompt_events
            .lock()
            .unwrap()
            .push(format!("{}:{}", input.hook_event_name, input.prompt.as_deref().unwrap_or("")));
        ::hooks::HookOutcome::Continue
    });

    agent.start_or_steer_turn("first", "t1").await.unwrap();
    agent.record_assistant_message("first answer").await.unwrap();
    agent.start_or_steer_turn("second", "t2").await.unwrap();

    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["SessionStart:startup", "UserPromptSubmit:first", "UserPromptSubmit:second"]
    );
}
```

Add a cold-resume test using the existing persisted-session pattern:

```rust
#[tokio::test]
async fn hydrated_session_starts_with_resume_source() {
    let dir = TempDir::new().unwrap();
    let root = dir.path().to_path_buf();
    {
        let agent = AgentLoop::with_session_id(
            AgentConfig::with_defaults(root.clone()),
            "resume-hooks".into(),
        )
        .unwrap();
        agent.ensure_session("test").unwrap();
        agent.start_or_steer_turn("first", "t1").await.unwrap();
        agent.record_assistant_message("answer").await.unwrap();
    }
    let agent = AgentLoop::with_session_id(
        AgentConfig::with_defaults(root),
        "resume-hooks".into(),
    )
    .unwrap();
    let source = Arc::new(std::sync::Mutex::new(None));
    let capture = Arc::clone(&source);
    agent.hook_bus().register(::hooks::SESSION_START, move |input| {
        *capture.lock().unwrap() = input.source.clone();
        ::hooks::HookOutcome::Continue
    });

    agent.start_or_steer_turn("second", "t2").await.unwrap();

    assert_eq!(source.lock().unwrap().as_deref(), Some("resume"));
}
```

Add prompt control-flow tests:

```rust
#[tokio::test]
async fn user_prompt_submit_block_prevents_persistence() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    agent.hook_bus().register(::hooks::USER_PROMPT_SUBMIT, |_| {
        ::hooks::HookOutcome::Block("policy".into())
    });

    let error = agent.start_or_steer_turn("blocked", "t1").await.unwrap_err();

    assert!(error.to_string().contains("policy"));
    assert!(agent.clone_history().await.is_empty());
}

#[tokio::test]
async fn user_prompt_submit_context_enters_initial_system_prompt() {
    let dir = TempDir::new().unwrap();
    let (agent, _) = AgentBuilder::new(dir.path()).build().unwrap();
    agent.hook_bus().register(::hooks::USER_PROMPT_SUBMIT, |_| {
        ::hooks::HookOutcome::InjectContext("PROMPT_HOOK_CONTEXT".into())
    });

    let result = agent.start_or_steer_turn("hello", "t1").await.unwrap();

    let TurnResult::Continue { system_prompt, .. } = result else {
        panic!("expected Continue");
    };
    assert!(system_prompt.contains("PROMPT_HOOK_CONTEXT"));
}
```

Also add these approved control cases:

- `session_start_block_prevents_prompt_and_persistence`: the error contains the reason, `UserPromptSubmit` hit count remains zero, and history remains empty.
- `session_and_prompt_contexts_enter_initial_system_prompt_in_order`: both contexts occur in the initial system prompt in event order, joined with `\n\n`.
- `prompt_skip_is_not_treated_as_block`: `Skip` is specific to `PreGatewayDispatch`; only `Block` blocks SessionStart or UserPromptSubmit admission.
- `session_start_block_retries_same_source`: the blocked source remains pending and the retry fires the same startup/resume source.
- `initial_inputs_are_persisted_as_one_logical_user_message` and `later_prompt_block_discards_staged_context_and_all_input`: multi-input admission is atomic and role-safe.
- `admission_context_respects_system_prompt_budget`: a long admission context cannot bypass `context_budget_chars`.
- reservation unit tests prove terminal close waits for commit and resumes on commit or Drop; the steering integration test uses a hook barrier and proves one hook fire plus model-visible follow-up.
- `steer_during_initial_prompt_preparation_preserves_role_order`: a barrier pauses the initial `UserPromptSubmit`, admits a follow-up steer, and proves one follow-up hook plus provider/history order `user initial -> assistant first -> user follow-up -> assistant second`.
- `concurrent_session_start_admission_fires_once`: a barrier holds the first callback while a second admission queues, then proves SessionStart fires exactly once.
- `concurrent_steers_preserve_submission_order`: the first steer callback is held while the second queues; before release the second hook has not fired, and afterward hook context, coalesced input, provider view, and history all preserve first-then-second order.
- `chat_delegates_session_start_to_the_shared_session_runtime`: production server source has no direct SessionStart dispatch or Plugin-only replacement.

Extend `steered_input_is_consumed_by_the_active_regular_task` to register `USER_PROMPT_SUBMIT`, capture both `prompt` and `turn_id`, call `.await.unwrap().expect(...)`, and assert the hook has already seen `follow up` plus the non-empty active turn id before releasing the first provider call.

- [ ] **Step 2: Run the new tests and verify RED**

Run:

```bash
cargo test -p agent --test rig_agent_test session_start_fires_once_before_each_user_prompt -- --exact
cargo test -p agent --test streaming_test hydrated_session_starts_with_resume_source -- --exact
cargo test -p agent --test rig_agent_test user_prompt_submit_block_prevents_persistence -- --exact
cargo test -p agent --test rig_agent_test user_prompt_submit_context_enters_initial_system_prompt -- --exact
cargo test -p agent --test rig_agent_test session_start_block_prevents_prompt_and_persistence -- --exact
cargo test -p agent --test rig_agent_test session_and_prompt_contexts_enter_initial_system_prompt_in_order -- --exact
cargo test -p agent --test rig_agent_test prompt_skip_is_not_treated_as_block -- --exact
cargo test -p agent --test streaming_test steered_input_is_consumed_by_the_active_regular_task -- --exact
cargo test -p agent --test streaming_test steer_during_initial_prompt_preparation_preserves_role_order -- --exact
cargo test -p agent --test streaming_test concurrent_steers_preserve_submission_order -- --exact
cargo test -p agent --lib runtime::turn_context::tests::reservation_blocks_terminal_close_until_commit -- --exact
cargo test -p agent --lib runtime::turn_context::tests::dropping_reservation_unblocks_terminal_close_and_closes_queue -- --exact
cargo test -p agent --lib runtime::turn_lifecycle::tests::initial_inputs_are_persisted_as_one_logical_user_message -- --exact
cargo test -p agent --lib runtime::turn_lifecycle::tests::later_prompt_block_discards_staged_context_and_all_input -- --exact
cargo test -p agent --lib runtime::turn_lifecycle::tests::session_start_block_retries_same_source -- --exact
cargo test -p agent --lib runtime::turn_lifecycle::tests::concurrent_session_start_admission_fires_once -- --exact
cargo test -p agent --lib runtime::turn_lifecycle::tests::admission_context_respects_system_prompt_budget -- --exact
cargo test -p server --lib grpc::astro_service::tests::chat_delegates_session_start_to_the_shared_session_runtime -- --exact
```

Expected: each behavioral test reports `running 1 test` when its wished-for API already compiles; reservation tests initially fail to compile until the new API exists. Runtime RED must expose the old per-input writes, leaked staged context, consumed blocked SessionStart, unbudgeted prompt growth, steering close race, and direct server SessionStart. A zero-test run is not accepted as RED evidence.

- [ ] **Step 3: Add one-shot SessionStart state**

Add the field and initialize it from the hydrated history:

```rust
pub(crate) pending_session_start_source: Option<String>,
```

```rust
let source = if history.is_empty() { "startup" } else { "resume" };
Self {
    history,
    pending_session_start_source: Some(source.to_string()),
    compression: compression_state::CompressionState::default(),
    turn: turn_budget::TurnState::default(),
    pending_inject_context: None,
    pending_learning_nudge: None,
    interaction_mode: types::InteractionMode::Agent,
    current_turn_context: None,
    current_step_context: None,
}
```

Peek the source before firing. A Block returns without consuming it; every non-Block result conditionally consumes only the same expected source:

```rust
let source = self.state.lock().await.pending_session_start_source.clone();
let outcome = self.fire_hook(::hooks::SESSION_START, payload);
let context = apply_admission_outcome(::hooks::SESSION_START, outcome)?;
let mut state = self.state.lock().await;
if state.pending_session_start_source.as_deref() == source.as_deref() {
    state.pending_session_start_source = None;
}
```

- [ ] **Step 4: Add prompt admission helper and lifecycle ordering**

Admission outcomes return staged context instead of mutating Session state:

```rust
fn apply_admission_outcome(
    event_name: &str,
    outcome: ::hooks::HookOutcome,
) -> anyhow::Result<Option<String>> {
    match outcome {
        ::hooks::HookOutcome::Block(reason) => {
            anyhow::bail!("{event_name} blocked by hook: {reason}")
        }
        ::hooks::HookOutcome::InjectContext(context) => Ok(Some(context)),
        _ => Ok(None),
    }
}
```

In `prepare_turn`, acquire the session FIFO admission lock and stage SessionStart plus every prompt context locally before `begin_user_turn`, reload, or persistence. Release the admission lock after the complete hook sequence. If every prompt passes, join contexts in event order and coalesce all `TurnInput` values into one logical user message. Persist exactly once, then pass the staged context into `build_system_prompt_with_inject` so it shares the configured prompt budget:

```rust
let mut contexts = Vec::new();
// SessionStart, then one UserPromptSubmit per input item; collect Option<String>.
// A Block returns before begin/reset/reload/write and drops all staged contexts.
let coalesced = coalesce_turn_inputs(input.iter().cloned()).unwrap();
self.begin_user_turn().await;
self.reload_tools_and_mcp().await?;
self.record_turn_input(coalesced).await?;
let context = (!contexts.is_empty()).then(|| contexts.join("\n\n"));
let system_prompt = self.build_system_prompt_with_inject(context.as_deref()).await;
```

`build_system_prompt()` delegates to the same internal builder with `None`. Do not manually append unbudgeted context after prompt assembly. Pending steers use the same coalescing helper before a single persistence write, but do not fire their hooks again.

At the streaming loop top, gate pending-input persistence until a provider sampling request has successfully returned. This preserves the role invariant for steers admitted while the initial prompt is still being prepared without changing normal tool-loop steering:

```rust
let mut has_sampled = false;
loop {
    if has_sampled {
        record_pending_input(&session, turn_context.take_pending_input()).await?;
    }
    // maintenance and request setup
    let raw_stream = run_sampling_request(/* ... */).await?;
    has_sampled = true;
    // consume the stream, then persist the assistant before terminal pending input
}
```

Do not set `has_sampled` before `run_sampling_request` succeeds: cancellation or provider setup failure must not pretend a sampling occurred or consume the early steer out of role order.

For steering, clone the active regular task's `Arc<TurnContext>` under `active_turn`, release that lock, acquire the FIFO admission lock, and only then reserve input before firing the hook. Terminal close asynchronously waits while reservations exist. Block/error/Drop cancels the reservation; success commits exactly once. Do not hold `active_turn` or Session state while invoking the callback:

```rust
pub async fn steer_input(
    &self,
    user_message: &str,
    image_data_urls: &[String],
) -> anyhow::Result<Option<String>> {
    if user_message.trim().is_empty() && image_data_urls.is_empty() {
        return Ok(None);
    }
    let running = {
        let active_turn = self.active_turn.lock().await;
        let Some(running) = active_turn.as_ref().and_then(|turn| turn.task.as_ref()) else {
            return Ok(None);
        };
        (running.kind, Arc::clone(&running.turn_context))
    };
    if running.0 != TaskKind::Regular {
        return Ok(None);
    }
    let _admission_guard = self.admission_lock.lock().await;
    let turn_id = running.1.sub_id().to_string();
    let Some(reservation) = running.1.reserve_input() else {
        return Ok(None);
    };
    let context = self.admit_user_prompt(user_message, Some(turn_id.clone()))?;
    let input = TurnInput::UserInput {
        content: user_message.to_string(),
        image_data_urls: image_data_urls.to_vec(),
    };
    if let Some(context) = context {
        let mut state = self.state.lock().await;
        append_context(&mut state.pending_inject_context, context);
        reservation.commit(input); // synchronous; notify happens after context write
    } else {
        reservation.commit(input);
    }
    Ok(Some(turn_id))
}
```

Immediately after server `get_session`, install the full runtime before steering:

```rust
session.set_hook_runtime(Arc::clone(&self.hook_runtime));
sess.steer_input(&content, &image_data_urls)
    .await
    .map_err(|error| Status::failed_precondition(error.to_string()))?
```

Remove `is_new_session`, direct server Gateway SessionStart, and later `set_hook_bus`. Preserve AgentEnd/reset/finalize lifecycle work for Task 5.

**Boundary: active-task best-effort steering**

B1 keeps steering scoped to the active `TurnContext`; `Done(true)` after a steer acknowledges enqueue, not durable persistence or eventual model consumption. The pre-Task-3 baseline (`916e95bf^`) already stored pending input only in `TurnContext`, drained it from the running loop, and returned directly on provider error/cancellation, so unconsumed steering could be lost when that active task ended. The reservation protocol preserves this baseline: a hook Block does not enqueue, Drop cancels an unfinished admission, and commit is equivalent to the former successful `push_input`. A durable receipt plus session-level recovery queue is a separate runtime reliability project and is not part of the Hooks B1 contract.

Initial admission context is present in the real budgeted system prompt. `system_prompt_layer_breakdown`, however, independently reconstructs static/dynamic layers for the pre-sampling `ContextUsage` estimate and has no request-level inject argument. Counting it would require plumbing the actual request prompt into the emitter or adding mutable last-prompt state; this batch does neither, so the estimate retains that existing limitation.

- [ ] **Step 5: Run lifecycle tests and Agent tests**

Run:

```bash
cargo test -p agent --test rig_agent_test
cargo test -p agent --test streaming_test
cargo test -p agent --lib
cargo test -p server --lib
cargo check --workspace --all-targets
cargo fmt --all -- --check
git diff --check
```

Expected: all focused tests previously ran exactly one test, and both integration suites, Agent library tests, server check, formatting, and diff checks pass. If `streaming_test` hits the known HITL race, report the first raw failure without retrying to hide it.

- [ ] **Step 6: Commit Task 3**

```bash
git add crates/agent-core/src/runtime/turn_context.rs crates/agent-core/src/runtime/turn_lifecycle.rs crates/agent-core/src/runtime/system_prompt.rs crates/agent-core/src/streaming/multi_turn.rs crates/agent-core/tests/streaming_test.rs crates/agent-server/src/grpc/astro_service.rs docs/superpowers/plans/2026-08-19-codex-hooks-b1-unified-dispatch-lifecycle.md
git commit -m "fix(agent): make prompt admission race safe"
```

### Task 4: Fire Stop for every terminal candidate

**Files:**
- Modify: `crates/agent-core/src/streaming/multi_turn.rs:560-635`
- Test: `crates/agent-core/tests/streaming_test.rs:596-818`

- [x] **Step 1: Rewrite the no-write test for the Codex behavior**

Rename it to `stop_fires_without_disk_write` and capture the Stop fields supplied by the runtime:

```rust
let stop_inputs = Arc::new(std::sync::Mutex::new(Vec::new()));
let inputs = Arc::clone(&stop_inputs);
agent.hook_bus().register(::hooks::STOP, move |input| {
    inputs.lock().unwrap().push((
        input.stop_hook_active,
        input.last_assistant_message.clone(),
        input.turn_id.clone(),
    ));
    ::hooks::HookOutcome::Continue
});
```

Replace the old negative assertion with:

```rust
let stop_inputs = stop_inputs.lock().unwrap().clone();
assert_eq!(stop_inputs.len(), 1);
assert_eq!(stop_inputs[0].0, Some(false));
assert_eq!(stop_inputs[0].1.as_deref(), Some("hi there"));
assert!(stop_inputs[0].2.is_some());
assert_eq!(
    events.iter().filter(|event| event.as_str() == ::hooks::STOP).count(),
    1
);
```

Remove the write-tool round from `stop_keep_going_retries_capped_at_two`, use three text-only provider rounds, capture `stop_hook_active`, and assert:

```rust
assert_eq!(stop_flags.lock().unwrap().as_slice(), [Some(false), Some(true)]);
assert_eq!(api_request_count, 3);
assert_eq!(post_llm_count, 1);
```

- [x] **Step 2: Run both tests and verify RED**

Run:

```bash
cargo test -p agent stop_fires_without_disk_write -- --exact
cargo test -p agent stop_keep_going_retries_capped_at_two -- --exact
```

Expected: the first test reports zero Stop calls and the text-only KeepGoing test finishes without the expected two continuations.

- [x] **Step 3: Remove the disk-write gate and set stop_hook_active**

Replace the Stop gate with:

```rust
let verify_outcome = {
    let agent = session.as_ref();
    let sid = agent.session_id().to_string();
    let turn_id = agent.current_turn_id().await;
    agent.fire_hook(
        ::hooks::STOP,
        ::hooks::HookPayload {
            session_id: sid,
            turn_id,
            stop_hook_active: Some(verify_attempt > 0),
            last_assistant_message: Some(full_response.clone()),
            detail: format!("attempt={}", verify_attempt + 1),
            ..Default::default()
        },
    )
};
if verify_attempt < MAX_VERIFY_ATTEMPTS {
    if let ::hooks::HookOutcome::KeepGoing(prompt) = verify_outcome {
    verify_attempt += 1;
    let agent = session.as_ref();
    let details = types::message::merge_google_thought_signature(
        Some(timeline.reasoning_details_snapshot()),
        thought_signature.as_deref(),
    );
    if let Err(err) = agent
        .record_assistant_with_calls(
            &full_response,
            &[],
            (!full_reasoning.is_empty()).then_some(full_reasoning.as_str()),
            details,
        )
        .await
    {
        finish_error(
            &session,
            &streamer,
            &tx,
            err.to_string(),
            saw_usage.then_some(total_usage),
            &run_id,
        )
        .await;
        return;
    }
    if let Err(err) = agent
        .record_user_message(&format!("[astro:hook-context]\n{prompt}"))
        .await
    {
        finish_error(
            &session,
            &streamer,
            &tx,
            err.to_string(),
            saw_usage.then_some(total_usage),
            &run_id,
        )
        .await;
        return;
    }
    defer_pending_input_after_stop = true;
    continue;
    }
}
```

Keep `MAX_VERIFY_ATTEMPTS = 2`: it limits accepted `KeepGoing` continuations, never Stop
dispatch. A terminal candidate after the quota still fires Stop with `stop_hook_active=true`,
then ignores `KeepGoing` and closes normally. Do not remove `turn_wrote_disk`; it remains
unrelated runtime state.

- [x] **Step 4: Run Stop and streaming tests**

Run:

```bash
cargo test -p agent stop_fires_without_disk_write -- --exact
cargo test -p agent stop_keep_going_retries_capped_at_two -- --exact
cargo test -p agent --test streaming_test
```

Expected: both focused tests and all streaming tests pass.

#### Follow-up: queued steer must not overtake a Stop bridge

When `Stop` returns `KeepGoing`, the loop persists its assistant draft followed by the
`[astro:hook-context]` bridge user message. If an active steer was admitted while the
Stop hook ran, `multi_turn` sets a one-shot local deferral after successfully writing the
bridge user message. The next loop-top leaves that queued steer in `TurnContext` exactly
once, so the provider responds to the bridge first; after that assistant response is
persisted, the terminal pending-input branch records the steer as the next legal user
turn. Do not take and requeue the input, and do not apply this deferral to thinking-only
or other non-Stop retry paths. The Stop-local flag is not consumed when the bridge begins
sampling: it survives reasoning-only bridge retries and clears only after the normal
assistant persistence path succeeds (text or tool calls), which also preserves ordering
for a bridge that resumes with a tool call.

`stop_keep_going_with_queued_steer_preserves_role_order` holds the first Stop hook on a
Condvar, admits exactly one `follow up` steer during that pause, and verifies the bridge
is sampled before the steer, both appear once, and the final history alternates roles.
`thinking_only_retry_consumes_queued_steer_before_sampling` protects the non-Stop path:
its retry must consume an already queued steer before its next sampling request.
`stop_keep_going_defers_queued_steer_across_reasoning_only_bridge_retry` verifies the
Stop bridge keeps the steer deferred through a reasoning-only retry, then consumes it
only after a normal bridge response is persisted.

Queued steer admission stores `TurnInput` and its `UserPromptSubmit` InjectContext in one
FIFO TurnContext entry. Only successful pending-input persistence moves those contexts into
the session injection slot, so a Stop bridge cannot consume a follow-up's context. Contexts
from multiple queued inputs append in FIFO order. Successfully consuming queued external
input begins a new response chain: reset its Stop continuation quota and deferral state, while
the Stop bridge itself never performs that reset.

```bash
cargo test -p agent stop_keep_going_with_queued_steer_preserves_role_order -- --exact
```

- [x] **Step 5: Commit Task 4**

```bash
git add crates/agent-core/src/streaming/multi_turn.rs crates/agent-core/tests/streaming_test.rs
git commit -m "fix(agent): run stop hooks for terminal turns"
```

#### Follow-up: budget-exhaustion summaries share the Stop chain

`run_max_iterations_summary` is also a terminal-candidate producer. It emits the budget
notice once and adds `MAX_ITERATIONS_SUMMARY_PROMPT` only to the request-local system prompt;
it never persists a synthetic user message. This keeps an existing main-loop Stop bridge from
becoming user/user and leaves no dangling synthetic input when summary setup or the provider
fails. Every complete tool-less summary response dispatches `Stop` with the same session, turn id, last assistant message,
`stop_hook_active`, and attempt detail as the main loop. Pass the main loop's mutable
`verify_attempt` into the summary runner: accepted `KeepGoing` outcomes persist the summary
assistant reply plus one `[astro:hook-context]` user bridge and re-sample without tools; the
shared maximum remains two accepted continuations. A third candidate still dispatches Stop but
ignores KeepGoing and finishes normally.

The summary runner captures a fresh step context before each re-sample so queued steering
contexts are attached only to the matching request. Before final close, it atomically drains
the active `TurnContext`: an acknowledged steer is persisted after the summary assistant,
resets the response-chain Stop quota, and receives a subsequent no-tools summary response.
This avoids a Stop-hook admission window that could otherwise be acknowledged then discarded,
while retaining the normal assistant/user role ordering.

Summary setup receives the owning `CancellationToken`. Each outer iteration checks that token
and `PauseControl` before work, then uses biased cancellation selects around both step-context
capture and provider setup. The active summary stream uses the same token branch as its
PauseControl cancellation path: it clears the abort handle, accumulates the current round usage,
and emits exactly one interrupt terminal. Thus both a cancellation after a Stop `KeepGoing`
continuation and `abort_all_tasks()` during a pending summary stream stop without another
provider request.

Focused coverage:

```bash
cargo test -p agent --test streaming_test multi_turn_budget_exhausted_forces_toolless_summary -- --exact
cargo test -p agent --test streaming_test budget_summary_stop_keep_going_retries_capped_at_two -- --exact
cargo test -p agent --test streaming_test budget_summary_reuses_main_stop_keep_going_quota -- --exact
cargo test -p agent --test streaming_test budget_summary_stop_consumes_queued_steer_before_terminal_close -- --exact
cargo test -p agent --test streaming_test main_stop_continuation_then_budget_summary_preserves_role_order -- --exact
cargo test -p agent --test streaming_test budget_summary_provider_failure_leaves_no_synthetic_user -- --exact
cargo test -p agent --test streaming_test budget_summary_active_stream_cancels_with_task_token -- --exact
```

### Task 5: Make RegularTask the single AgentEnd authority and update server wiring

**Files:**
- Modify: `crates/agent-core/src/tasks/regular.rs:21-66`
- Modify: `crates/agent-core/src/streaming/multi_turn.rs:850-880`
- Modify: `crates/agent-server/src/grpc/astro_service.rs:334-365,710-890,1070-1095`
- Test: `crates/agent-core/tests/streaming_test.rs`
- Test: `crates/agent-server/src/grpc/astro_service.rs:1671-1745`

- [x] **Step 1: Write failing AgentEnd failure-path and duplicate-reset tests**

Extend the existing provider-error streaming test by capturing AgentEnd errors and asserting one call with the original provider failure after the stream ends:

```rust
let agent_end_errors = Arc::new(std::sync::Mutex::new(Vec::new()));
let errors = Arc::clone(&agent_end_errors);
agent.hook_bus().register(::hooks::AGENT_END, move |input| {
    errors.lock().unwrap().push(input.error.clone());
    ::hooks::HookOutcome::Continue
});
```

After draining the stream:

```rust
let errors = agent_end_errors.lock().unwrap();
assert_eq!(errors.len(), 1);
assert!(errors[0]
    .as_deref()
    .is_some_and(|error| error.contains("boom")));
```

Reuse `budget_summary_provider_failure_leaves_no_synthetic_user` to assert that a non-first-round summary setup/provider failure also produces exactly one AgentEnd whose error contains `summary provider failure`. Keep success, cancellation, and early receiver-drop coverage explicit: success/cancel/receiver drop each fire AgentEnd exactly once with `error=None`. Preparation failure remains a separate assertion that AgentEnd contains the `turn budget exhausted` error.

In `new_chat_preserves_hooks_while_release_session_skips_them`, pre-create a Session and bind the shared runtime before sending the new-chat control request:

```rust
let session = service.get_session("new-chat").await.unwrap();
session.set_hook_runtime(Arc::clone(&service.hook_runtime));
```

Keep these exact final assertions:

```rust
assert_eq!(gateway_hits.load(Ordering::SeqCst), 1);
assert_eq!(reset_hits.load(Ordering::SeqCst), 1);
assert_eq!(finalize_hits.load(Ordering::SeqCst), 1);
```

This fails on the current post-release direct Plugin bus duplicate.

- [x] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test -p agent error_has_single_error_terminal_before_done -- --exact
cargo test -p agent budget_summary_provider_failure_leaves_no_synthetic_user -- --exact
cargo test -p server grpc::astro_service::tests::new_chat_preserves_hooks_while_release_session_skips_them -- --exact
```

Expected: runtime provider and summary failures currently emit their stream terminal sequence but lose the error before AgentEnd, while the pre-created new-chat Session causes reset/finalize to be counted twice.

- [x] **Step 3: Finalize AgentEnd in RegularTask**

Clone `ctx` when constructing the arguments so it remains available for finalization:

```rust
let args = self
    .args
    .with_session_and_turn(Arc::clone(&sess), Arc::clone(&ctx));
```

Make `run_turn` return a crate-private typed terminal classification after preparation succeeds:

```rust
pub(crate) enum RunTurnOutcome {
    Success,
    Failed(String),
    Interrupted,
}

impl RunTurnOutcome {
    pub(crate) fn error(&self) -> Option<&str> {
        match self {
            Self::Failed(error) => Some(error),
            Self::Success | Self::Interrupted => None,
        }
    }
}
```

Every runtime branch that already calls `finish_error` returns `RunTurnOutcome::Failed` with that same error string. Success returns `Success`; cancellation, summary abort, and receiver drop return the non-error `Interrupted` outcome. Runtime `Failed` must not become a `SessionTask Err`, because its Error/RunFinished/Done sequence was already emitted and returning an error would create a second terminal sequence.

Wrap preparation and `run_turn` in a result-producing async block. Preparation errors remain `SessionTask Err`; a completed runtime outcome always maps to `Ok(None)` and is used only to populate AgentEnd.error. Then dispatch AgentEnd unconditionally before returning the SessionTask result:

```rust
let runtime_result: anyhow::Result<RunTurnOutcome> = async {
    let system_prompt = match args.prepared_system_prompt().map(str::to_owned) {
        Some(system_prompt) => {
            anyhow::ensure!(
                input.is_empty(),
                "prebuilt system prompt cannot be combined with initial input"
            );
            system_prompt
        }
        None => {
            let turn = sess.prepare_turn(&input).await?;
            match turn {
                TurnResult::Continue { system_prompt, .. } => system_prompt,
                TurnResult::BudgetExhausted => {
                    anyhow::bail!("conversation turn budget exhausted")
                }
                TurnResult::Interrupted => anyhow::bail!("regular turn interrupted"),
                TurnResult::Steered { .. }
                | TurnResult::ToolCalls(_)
                | TurnResult::Finished(_)
                | TurnResult::MaxDepth => {
                    anyhow::bail!("unsupported regular turn preparation result")
                }
            }
        }
    };
    Ok(run_turn(args.with_system_prompt(system_prompt), cancellation_token).await)
}
.await;

let (result, error): (SessionTaskResult, Option<String>) = match runtime_result {
    Ok(outcome) => (Ok(None), outcome.error().map(str::to_owned)),
    Err(error) => {
        let hook_error = error.to_string();
        (Err(error), Some(hook_error))
    }
};

let turn = sess.session_turn().await;
let _ = sess.fire_hook(
    ::hooks::AGENT_END,
    ::hooks::HookPayload {
        turn_id: Some(ctx.sub_id().to_string()),
        turn: Some(turn),
        error,
        detail: format!("turn={turn}"),
        ..Default::default()
    },
);
result
```

Delete the AgentEnd block from `multi_turn.rs`.

- [x] **Step 4: Remove the remaining server AgentEnd/reset/finalize duplicates**

Task 3 already injects the full runtime before steering and removes `is_new_session`, direct server SessionStart, and the later `set_hook_bus`. In this task remove only the stream-cleanup Gateway AgentEnd duplicate and simplify reset/finalize delivery.

Simplify `release_session_for_new_chat` so each event is dispatched exactly once before release:

```rust
self.hook_runtime.dispatch(::hooks::COMMAND_NEW_CHAT, &payload);
self.hook_runtime.dispatch(::hooks::SESSION_RESET, &payload);
self.hook_runtime.dispatch(::hooks::SESSION_FINALIZE, &payload);
let _ = self.release_session_runtime(session_id).await;
```

Do not emit SessionEnd in this batch.

- [x] **Step 5: Run focused, server, and Agent tests**

Run:

```bash
cargo test -p agent error_has_single_error_terminal_before_done -- --exact
cargo test -p agent budget_summary_provider_failure_leaves_no_synthetic_user -- --exact
cargo test -p agent agent_end_fires_once_on_success -- --exact
cargo test -p agent agent_end_fires_once_when_turn_preparation_fails -- --exact
cargo test -p agent agent_end_fires_once_when_stream_receiver_is_dropped -- --exact
cargo test -p agent cancellation_has_single_interrupt_terminal_before_done -- --exact
cargo test -p server grpc::astro_service::tests::new_chat_preserves_hooks_while_release_session_skips_them -- --exact
cargo test -p agent --test streaming_test
cargo test -p agent --all-targets
cargo test -p server --all-targets
```

Expected: provider and summary failures preserve their original message in exactly one AgentEnd; success, cancel, and receiver drop fire exactly one AgentEnd with no error; preparation failure remains a SessionTask error; stream terminal counts remain unchanged; new-chat reset/finalize fire once; and both crates pass all targets.

- [x] **Step 6: Commit Task 5**

```bash
git add crates/agent-core/src/tasks/regular.rs crates/agent-core/src/streaming/multi_turn.rs crates/agent-core/tests/streaming_test.rs crates/agent-server/src/grpc/astro_service.rs
git commit -m "fix(hooks): align main turn lifecycle"
```

### Task 6: Document B1 truth and run final verification

**Files:**
- Modify: `docs/hooks.md`
- Modify: `docs/examples/hooks/README.md`
- Modify: `docs/examples/hooks/config.yaml.snippet`
- Modify: `docs/examples/hooks/telemetry-webhook.sh`
- Modify: `docs/superpowers/plans/2026-08-19-codex-hooks-b1-unified-dispatch-lifecycle.md`

- [x] **Step 1: Update runtime documentation**

Make these factual changes:

- change the page introduction from “Batch A naming/serialization only” to “Batch A contract plus B1 dispatch/lifecycle”;
- document that Session events from `Session::fire_hook` now reach Plugin, Gateway, and Shell through one normalized dispatch;
- change SessionStart to one-shot after its first non-`Block` `startup` or `resume` admission, and list `clear`/`compact` as B2 gaps;
- add UserPromptSubmit before persistence, including Block and InjectContext behavior;
- change Stop to every terminal candidate, with `stop_hook_active=false` first and `true` on continuation, capped at two KeepGoing attempts;
- state that AgentEnd is emitted once by RegularTask for success or failure and is not SessionEnd;
- remove the old diagram’s duplicate Gateway SessionStart/AgentEnd nodes;
- move `SessionEnd`, PreCompact/PostCompact, and SubagentStop continuation into the remaining lifecycle gap list;
- retain the warning that Batch C command JSON/matcher/trust behavior is not implemented.

- [x] **Step 2: Check documentation for stale B1 claims**

Run:

```bash
rg -n "每次 run_turn|Gateway SessionStart|Gateway AgentEnd|不会自动投递|no disk|写盘|尚无 runtime fire 点" docs/hooks.md
```

Expected: no stale statement says SessionStart fires every turn, AgentLoop bypasses HookRuntime, Stop requires disk writes, or server emits a second AgentEnd. Any matches must describe removed behavior explicitly as migration history, not current behavior.

- [x] **Step 3: Run formatting and the full relevant verification suite**

Run:

```bash
cargo fmt --all -- --check
cargo test -p hooks
cargo test -p agent --all-targets
cargo test -p server --all-targets
cargo check --workspace --all-targets
git diff --check
```

Expected: every command exits 0 with zero test failures and no formatting or whitespace errors.

- [x] **Step 4: Review scope and compatibility**

Run:

```bash
git status --short
git log --oneline --decorate -6
rg -n "set_hook_bus\(|hook_bus\(\)\.fire|fire_gateway\(.*SESSION_START|fire_gateway\(.*AGENT_END" crates/agent-core crates/agent-server
```

Expected:

- changed files are limited to the B1 file map and this plan;
- compatibility `set_hook_bus` remains available but production server wiring uses `set_hook_runtime`;
- no Session-owned Agent core lifecycle call bypasses `HookRuntime::dispatch`; the documented `SubagentStart` / `SubagentStop` request-bus direct fires are the exception;
- no server-side SessionStart or AgentEnd duplicate remains.

**Review record:** distinguish retried blocked `SessionStart`; normal-loop API/transform/post hooks from direct summary streaming; all five private payload fields; construction-time (not readiness) `GatewayStartup`; and Plugin-only subagent lifecycle events. Keep Shell examples in sync with Session runtime routing without claiming subagent delivery; telemetry is custom JSON and requires a webhook or explicit adapter for third-party backends.

- [x] **Step 5: Commit documentation**

```bash
git add docs/hooks.md docs/examples/hooks/README.md docs/examples/hooks/config.yaml.snippet docs/examples/hooks/telemetry-webhook.sh docs/superpowers/plans/2026-08-19-codex-hooks-b1-unified-dispatch-lifecycle.md
git commit -m "docs(hooks): align examples with runtime"
```

## Self-review checklist

- Spec coverage: Session-owned B1 dispatch, startup/resume admission, prompt admission, Stop, AgentEnd, server de-duplication, documented Plugin-only subagent exceptions, compatibility, tests, and docs each map to a task.
- Deferred scope: SessionEnd, compact/clear, Pre/PostCompact, SubagentStart/Stop transport, SubagentStop continuation, and Command Hook behavior are explicitly excluded and remain documented.
- Type consistency: `HookRuntime::dispatch` takes `&HookPayload` and returns `HookOutcome`; Session stores `Arc<HookRuntime>`; steering returns `anyhow::Result<Option<String>>` at every caller.
- TDD consistency: every production behavior begins with a focused test that fails for the missing behavior.
- Commit consistency: each task stages only its listed files and produces an independently reviewable commit.
