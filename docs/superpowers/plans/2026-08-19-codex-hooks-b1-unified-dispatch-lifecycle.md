# Codex Hooks B1 Unified Dispatch and Lifecycle Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Route Astro Agent lifecycle hooks through one canonical `HookRuntime::dispatch` path and align the main-turn `SessionStart`, `UserPromptSubmit`, `Stop`, and `AgentEnd` behavior with the approved Codex contract.

**Architecture:** `Session` owns an `Arc<HookRuntime>` instead of only a Plugin bus, enriches every payload with stable session/model/cwd/permission fields, and delegates all events to one dispatcher that reaches Plugin, Gateway, and Shell exactly once. The main lifecycle fires a one-shot startup/resume event, evaluates prompt hooks before persistence, evaluates Stop for every terminal candidate with a bounded continuation loop, and emits one AgentEnd from `RegularTask` on both success and failure.

**Tech Stack:** Rust 2021 workspace, Tokio, `agent-hooks`, `agent-core`, `agent-server`, Cargo integration tests.

---

## Scope boundary

This plan implements B1 only:

- unified Plugin/Gateway/Shell dispatch;
- Session ownership of the shared runtime and common payload enrichment;
- `SessionStart(source=startup|resume)` once per in-memory Session;
- `UserPromptSubmit` block/context behavior before persistence;
- `Stop` on every terminal candidate, independent of disk writes;
- one authoritative `AgentEnd` per `RegularTask` run;
- removal of duplicate server-side SessionStart/AgentEnd/reset/finalize delivery;
- accurate B1 documentation.

The following remain separate B2/B3/C work:

- `SessionEnd`, clear/compact SessionStart sources, `PreCompact`, and `PostCompact`;
- resumable `SubagentStop`;
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

### Task 1: Add one canonical HookRuntime dispatcher

**Files:**
- Modify: `crates/agent-hooks/src/lib.rs:27-195`

- [ ] **Step 1: Write a failing dispatcher test**

Add a test that registers the same legacy-named event across Plugin, Gateway, and Shell, dispatches once, and proves that both observable buses receive the canonical name while the Plugin outcome is returned:

```rust
#[test]
fn dispatch_normalizes_once_and_reaches_all_transports() {
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
    assert!(rt.shell.lock().unwrap().has_event(PRE_TOOL_USE));
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

    pub fn fire_gateway(&self, name: &str, payload: &HookPayload) -> HookOutcome {
        self.dispatch(name, payload)
    }
}
```

Gateway and Shell remain observers in B1; the Plugin outcome remains the controlling result until the multi-handler aggregator lands in Batch C.

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
    self.set_hook_runtime(Arc::new(::hooks::HookRuntime::with_plugin_bus(bus)));
}

pub fn hook_bus(&self) -> Arc<::hooks::PluginHookBus> {
    Arc::clone(&self.session_configuration().hook_runtime.plugin)
}
```

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
- Modify: `crates/agent-core/src/runtime/session_state.rs:14-63`
- Modify: `crates/agent-core/src/runtime/turn_lifecycle.rs:69-220`
- Test: `crates/agent-core/tests/rig_agent_test.rs`
- Test: `crates/agent-core/tests/streaming_test.rs:110-205`

- [ ] **Step 1: Write failing startup, resume, block, and context tests**

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

Extend the active steering test to register `USER_PROMPT_SUBMIT`, capture `prompt`, call `.await.unwrap().expect(...)`, and assert the captured prompt is `follow up` before releasing the first provider call.

- [ ] **Step 2: Run the new tests and verify RED**

Run:

```bash
cargo test -p agent session_start_fires_once_before_each_user_prompt -- --exact
cargo test -p agent hydrated_session_starts_with_resume_source -- --exact
cargo test -p agent user_prompt_submit_block_prevents_persistence -- --exact
cargo test -p agent user_prompt_submit_context_enters_initial_system_prompt -- --exact
cargo test -p agent steered_input_is_consumed_by_the_active_regular_task -- --exact
```

Expected: lifecycle assertions fail because SessionStart fires every turn and UserPromptSubmit is not yet fired; the steering test also requires the new Result-returning API.

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

Add a Session helper:

```rust
async fn take_session_start_source(&self) -> Option<String> {
    self.state.lock().await.pending_session_start_source.take()
}
```

- [ ] **Step 4: Add prompt admission helper and lifecycle ordering**

Add a shared async helper:

```rust
async fn admit_user_prompt(
    &self,
    prompt: &str,
    turn_id: Option<String>,
) -> anyhow::Result<()> {
    match self.fire_hook(
        ::hooks::USER_PROMPT_SUBMIT,
        ::hooks::HookPayload {
            turn_id,
            prompt: Some(prompt.to_string()),
            detail: prompt.chars().take(200).collect(),
            ..Default::default()
        },
    ) {
        ::hooks::HookOutcome::Block(reason) | ::hooks::HookOutcome::Skip(reason) => {
            anyhow::bail!("user prompt blocked by hook: {reason}")
        }
        ::hooks::HookOutcome::InjectContext(context) => {
            let mut state = self.state.lock().await;
            state.pending_inject_context = Some(match state.pending_inject_context.take() {
                Some(existing) => format!("{existing}\n\n{context}"),
                None => context,
            });
        }
        _ => {}
    }
    Ok(())
}
```

In `prepare_turn`, after the budget check and before persistence:

```rust
if let Some(source) = self.take_session_start_source().await {
    let _ = self.fire_hook(
        ::hooks::SESSION_START,
        ::hooks::HookPayload {
            source: Some(source),
            detail: format!("session={}", self.session_id),
            ..Default::default()
        },
    );
}
self.begin_user_turn().await;
let turn_id = self.current_turn_id().await;
for item in input {
    let TurnInput::UserInput { content, .. } = item;
    self.admit_user_prompt(content, turn_id.clone()).await?;
}
```

Delete the old per-turn SessionStart block near `PRE_LLM_CALL`. Keep `PreLlmCall` after system-prompt construction.

Change steering to return admission errors and fire before queueing:

```rust
pub async fn steer_input(
    &self,
    user_message: &str,
    image_data_urls: &[String],
) -> anyhow::Result<Option<String>> {
    if user_message.trim().is_empty() && image_data_urls.is_empty() {
        return Ok(None);
    }
    let active_turn = self.active_turn.lock().await;
    let Some(running) = active_turn.as_ref().and_then(|turn| turn.task.as_ref()) else {
        return Ok(None);
    };
    if running.kind != TaskKind::Regular {
        return Ok(None);
    }
    self.admit_user_prompt(
        user_message,
        Some(running.turn_context.sub_id().to_string()),
    )
    .await?;
    let accepted = running.turn_context.push_input(TurnInput::UserInput {
        content: user_message.to_string(),
        image_data_urls: image_data_urls.to_vec(),
    });
    Ok(accepted.then(|| running.turn_context.sub_id().to_string()))
}
```

Update `start_or_steer_turn_with_images` to use `self.steer_input(...).await?`.

- [ ] **Step 5: Run lifecycle tests and Agent tests**

Run:

```bash
cargo test -p agent session_start_fires_once_before_each_user_prompt -- --exact
cargo test -p agent hydrated_session_starts_with_resume_source -- --exact
cargo test -p agent user_prompt_submit_block_prevents_persistence -- --exact
cargo test -p agent user_prompt_submit_context_enters_initial_system_prompt -- --exact
cargo test -p agent steered_input_is_consumed_by_the_active_regular_task -- --exact
cargo test -p agent --all-targets
```

Expected: all focused tests and all Agent targets pass.

- [ ] **Step 6: Commit Task 3**

```bash
git add crates/agent-core/src/runtime/session_state.rs crates/agent-core/src/runtime/turn_lifecycle.rs crates/agent-core/tests/rig_agent_test.rs crates/agent-core/tests/streaming_test.rs
git commit -m "feat(agent): align session and prompt hooks"
```

### Task 4: Fire Stop for every terminal candidate

**Files:**
- Modify: `crates/agent-core/src/streaming/multi_turn.rs:560-635`
- Test: `crates/agent-core/tests/streaming_test.rs:596-818`

- [ ] **Step 1: Rewrite the no-write test for the Codex behavior**

Rename it to `stop_fires_without_disk_write` and capture the stop flag:

```rust
let stop_flags = Arc::new(std::sync::Mutex::new(Vec::new()));
let flags = Arc::clone(&stop_flags);
agent.hook_bus().register(::hooks::STOP, move |input| {
    flags.lock().unwrap().push(input.stop_hook_active);
    ::hooks::HookOutcome::Continue
});
```

Replace the old negative assertion with:

```rust
assert_eq!(stop_flags.lock().unwrap().as_slice(), [Some(false)]);
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

- [ ] **Step 2: Run both tests and verify RED**

Run:

```bash
cargo test -p agent stop_fires_without_disk_write -- --exact
cargo test -p agent stop_keep_going_retries_capped_at_two -- --exact
```

Expected: the first test reports zero Stop calls and the text-only KeepGoing test finishes without the expected two continuations.

- [ ] **Step 3: Remove the disk-write gate and set stop_hook_active**

Replace the Stop gate with:

```rust
let verify_outcome = if verify_attempt < MAX_VERIFY_ATTEMPTS {
    let agent = session.as_ref();
    let sid = agent.session_id().to_string();
    let turn_id = agent.current_turn_id().await;
    Some(agent.fire_hook(
        ::hooks::STOP,
        ::hooks::HookPayload {
            session_id: sid,
            turn_id,
            stop_hook_active: Some(verify_attempt > 0),
            last_assistant_message: Some(full_response.clone()),
            detail: format!("attempt={}", verify_attempt + 1),
            ..Default::default()
        },
    ))
} else {
    None
};
if let Some(::hooks::HookOutcome::KeepGoing(prompt)) = verify_outcome {
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
    continue;
}
```

Keep `MAX_VERIFY_ATTEMPTS = 2`. Do not remove `turn_wrote_disk`; it remains unrelated runtime state.

- [ ] **Step 4: Run Stop and streaming tests**

Run:

```bash
cargo test -p agent stop_fires_without_disk_write -- --exact
cargo test -p agent stop_keep_going_retries_capped_at_two -- --exact
cargo test -p agent --test streaming_test
```

Expected: both focused tests and all streaming tests pass.

- [ ] **Step 5: Commit Task 4**

```bash
git add crates/agent-core/src/streaming/multi_turn.rs crates/agent-core/tests/streaming_test.rs
git commit -m "fix(agent): run stop hooks for terminal turns"
```

### Task 5: Make RegularTask the single AgentEnd authority and update server wiring

**Files:**
- Modify: `crates/agent-core/src/tasks/regular.rs:21-66`
- Modify: `crates/agent-core/src/streaming/multi_turn.rs:850-880`
- Modify: `crates/agent-server/src/grpc/astro_service.rs:334-365,710-890,1070-1095`
- Test: `crates/agent-core/tests/streaming_test.rs`
- Test: `crates/agent-server/src/grpc/astro_service.rs:1671-1745`

- [ ] **Step 1: Write failing AgentEnd failure-path and duplicate-reset tests**

Extend the existing provider-error streaming test by registering AgentEnd and asserting one call after the stream ends:

```rust
let agent_end_hits = Arc::new(AtomicUsize::new(0));
let agent_end_counter = Arc::clone(&agent_end_hits);
agent.hook_bus().register(::hooks::AGENT_END, move |_| {
    agent_end_counter.fetch_add(1, Ordering::SeqCst);
    ::hooks::HookOutcome::Continue
});
```

After draining the stream:

```rust
assert_eq!(agent_end_hits.load(Ordering::SeqCst), 1);
```

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

- [ ] **Step 2: Run both tests and verify RED**

Run:

```bash
cargo test -p agent error_has_single_error_terminal_before_done -- --exact
cargo test -p server new_chat_preserves_hooks_while_release_session_skips_them -- --exact
```

Expected: AgentEnd is absent on the early error return, and the pre-created new-chat Session causes reset/finalize to be counted twice.

- [ ] **Step 3: Finalize AgentEnd in RegularTask**

Clone `ctx` when constructing the arguments so it remains available for finalization:

```rust
let args = self
    .args
    .with_session_and_turn(Arc::clone(&sess), Arc::clone(&ctx));
```

Wrap preparation and `run_turn` in a result-producing async block, then dispatch AgentEnd unconditionally before returning that result:

```rust
let result: SessionTaskResult = async {
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
    run_turn(args.with_system_prompt(system_prompt), cancellation_token).await;
    Ok(None)
}
.await;

let turn = sess.session_turn().await;
let _ = sess.fire_hook(
    ::hooks::AGENT_END,
    ::hooks::HookPayload {
        turn_id: Some(ctx.sub_id().to_string()),
        turn: Some(turn),
        error: result.as_ref().err().map(ToString::to_string),
        detail: format!("turn={turn}"),
        ..Default::default()
    },
);
result
```

Delete the AgentEnd block from `multi_turn.rs`.

- [ ] **Step 4: Inject full runtime before steering and remove server duplicates**

Immediately after `get_session`, before `steer_input`, add:

```rust
let session = self.get_session(&session_id).await?;
session.set_hook_runtime(Arc::clone(&self.hook_runtime));
let steered_turn_id = session
    .steer_input(&content, &image_data_urls)
    .await
    .map_err(|error| Status::failed_precondition(error.to_string()))?;
```

Remove `is_new_session`, the server-side Gateway SessionStart block, the later `set_hook_bus` call, and the stream-cleanup Gateway AgentEnd block.

Simplify `release_session_for_new_chat` so each event is dispatched exactly once before release:

```rust
self.hook_runtime.dispatch(::hooks::COMMAND_NEW_CHAT, &payload);
self.hook_runtime.dispatch(::hooks::SESSION_RESET, &payload);
self.hook_runtime.dispatch(::hooks::SESSION_FINALIZE, &payload);
let _ = self.release_session_runtime(session_id).await;
```

Do not emit SessionEnd in this batch.

- [ ] **Step 5: Run focused, server, and Agent tests**

Run:

```bash
cargo test -p agent error_has_single_error_terminal_before_done -- --exact
cargo test -p server new_chat_preserves_hooks_while_release_session_skips_them -- --exact
cargo test -p agent --all-targets
cargo test -p server --all-targets
```

Expected: AgentEnd fires once on error, new-chat reset/finalize fire once, and both crates pass all targets.

- [ ] **Step 6: Commit Task 5**

```bash
git add crates/agent-core/src/tasks/regular.rs crates/agent-core/src/streaming/multi_turn.rs crates/agent-core/tests/streaming_test.rs crates/agent-server/src/grpc/astro_service.rs
git commit -m "fix(hooks): align main turn lifecycle"
```

### Task 6: Document B1 truth and run final verification

**Files:**
- Modify: `docs/hooks.md`

- [ ] **Step 1: Update runtime documentation**

Make these factual changes:

- change the page introduction from “Batch A naming/serialization only” to “Batch A contract plus B1 dispatch/lifecycle”;
- document that Session events from `Session::fire_hook` now reach Plugin, Gateway, and Shell through one normalized dispatch;
- change SessionStart to one-shot `startup` or `resume` and list `clear`/`compact` as B2 gaps;
- add UserPromptSubmit before persistence, including Block and InjectContext behavior;
- change Stop to every terminal candidate, with `stop_hook_active=false` first and `true` on continuation, capped at two KeepGoing attempts;
- state that AgentEnd is emitted once by RegularTask for success or failure and is not SessionEnd;
- remove the old diagram’s duplicate Gateway SessionStart/AgentEnd nodes;
- move `SessionEnd`, PreCompact/PostCompact, and SubagentStop continuation into the remaining lifecycle gap list;
- retain the warning that Batch C command JSON/matcher/trust behavior is not implemented.

- [ ] **Step 2: Check documentation for stale B1 claims**

Run:

```bash
rg -n "每次 run_turn|Gateway SessionStart|Gateway AgentEnd|不会自动投递|no disk|写盘|尚无 runtime fire 点" docs/hooks.md
```

Expected: no stale statement says SessionStart fires every turn, AgentLoop bypasses HookRuntime, Stop requires disk writes, or server emits a second AgentEnd. Any matches must describe removed behavior explicitly as migration history, not current behavior.

- [ ] **Step 3: Run formatting and the full relevant verification suite**

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

- [ ] **Step 4: Review scope and compatibility**

Run:

```bash
git status --short
git log --oneline --decorate -6
rg -n "set_hook_bus\(|hook_bus\(\)\.fire|fire_gateway\(.*SESSION_START|fire_gateway\(.*AGENT_END" crates/agent-core crates/agent-server
```

Expected:

- changed files are limited to the B1 file map and this plan;
- compatibility `set_hook_bus` remains available but production server wiring uses `set_hook_runtime`;
- no Agent core call bypasses `HookRuntime::dispatch`;
- no server-side SessionStart or AgentEnd duplicate remains.

- [ ] **Step 5: Commit documentation**

```bash
git add docs/hooks.md
git commit -m "docs(hooks): describe unified lifecycle dispatch"
```

## Self-review checklist

- Spec coverage: B1 dispatch, startup/resume, prompt admission, Stop, AgentEnd, server de-duplication, compatibility, tests, and docs each map to a task.
- Deferred scope: SessionEnd, compact/clear, Pre/PostCompact, SubagentStop, and Command Hook behavior are explicitly excluded and remain documented.
- Type consistency: `HookRuntime::dispatch` takes `&HookPayload` and returns `HookOutcome`; Session stores `Arc<HookRuntime>`; steering returns `anyhow::Result<Option<String>>` at every caller.
- TDD consistency: every production behavior begins with a focused test that fails for the missing behavior.
- Commit consistency: each task stages only its listed files and produces an independently reviewable commit.
