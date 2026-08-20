# Codex Hooks Contract Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** Complete batch A of the approved Hooks design by making Codex event names and wire variable names canonical while preserving one-release compatibility for existing Astro event names and configuration.

**Architecture:** Add a typed `HookEvent` contract and a serializable `HookInput` wire model in the `hooks` crate. Normalize legacy names at every registration and dispatch boundary, migrate internal callers to canonical constants and fields, and keep old constants plus `HookPayload` as compatibility aliases. This batch changes naming and serialization only; lifecycle timing, command execution, trust, and decision aggregation remain for later batches.

**Tech Stack:** Rust 2021, serde/serde_json, existing `hooks`/`agent`/`tools`/`server` crates, Cargo tests.

**Reference implementation:** `/Users/iswm/CodeRope/codex/codex-rs/hooks/src/schema.rs`, `/Users/iswm/CodeRope/codex/codex-rs/hooks/src/events/`, `/Users/iswm/CodeRope/codex/codex-rs/config/src/hook_config.rs`, and `/Users/iswm/CodeRope/codex/codex-rs/protocol/src/protocol.rs:1502`.

---

## Batch boundary

This plan implements only the first independently verifiable batch:

- canonical Codex names for the eleven public events;
- PascalCase names for Astro-only extension events;
- legacy name normalization at registration, discovery, dispatch, shell config, UI, and logging boundaries;
- Codex-compatible input variable names and JSON serialization;
- migration of existing Rust call sites away from `tool_args`, `tool_result`, and ambiguous `message` fields;
- one-release compatibility aliases for old Rust constants, `HookPayload`, YAML keys, and `ASTRO_HOOK_*` environment variables.

It explicitly does not change when events fire, add command JSON stdin/stdout, implement matcher/aggregation, or add trust/UI management. Those are batches B-D in the approved design.

## File structure

- Create `crates/agent-hooks/src/event.rs`: typed canonical event set and legacy alias normalization.
- Modify `crates/agent-hooks/src/names.rs`: canonical constants and compatibility constants only.
- Modify `crates/agent-hooks/src/outcome.rs`: `HookInput` canonical wire fields and `HookPayload` type alias.
- Modify `crates/agent-hooks/src/plugin.rs`: canonicalize registration and dispatch keys.
- Modify `crates/agent-hooks/src/gateway.rs`: canonicalize manifest event subscriptions and emitted labels.
- Modify `crates/agent-hooks/src/shell.rs`: canonicalize YAML keys and derive legacy environment variables from canonical fields.
- Modify `crates/agent-hooks/src/ui.rs`: subscribe to and emit canonical event names.
- Modify `crates/agent-hooks/src/lib.rs`: exports and canonical runtime boundaries.
- Modify Agent/Tools/Server Hook call sites listed in Task 4.
- Modify `docs/hooks.md` and `docs/examples/hooks/*`: migration table and batch-A behavior.

### Task 1: Define canonical event names and legacy aliases

**Files:**
- Create: `crates/agent-hooks/src/event.rs`
- Modify: `crates/agent-hooks/src/names.rs`
- Modify: `crates/agent-hooks/src/lib.rs`
- Test: `crates/agent-hooks/src/event.rs`

- [x] **Step 1: Write failing event-contract tests**

Create `crates/agent-hooks/src/event.rs` with the tests first, before adding the production definitions:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_events_use_exact_pascal_case_labels() {
        let actual = HookEvent::CODEX
            .iter()
            .map(|event| event.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            actual,
            vec![
                "PreToolUse",
                "PermissionRequest",
                "PostToolUse",
                "PreCompact",
                "PostCompact",
                "SessionStart",
                "SessionEnd",
                "UserPromptSubmit",
                "SubagentStart",
                "SubagentStop",
                "Stop",
            ]
        );
    }

    #[test]
    fn legacy_names_normalize_to_canonical_labels() {
        assert_eq!(canonical_hook_event_name("pre_tool_call"), "PreToolUse");
        assert_eq!(canonical_hook_event_name("post_tool_call"), "PostToolUse");
        assert_eq!(canonical_hook_event_name("pre_approval_request"), "PermissionRequest");
        assert_eq!(canonical_hook_event_name("pre_verify"), "Stop");
        assert_eq!(canonical_hook_event_name("on_session_start"), "SessionStart");
        assert_eq!(canonical_hook_event_name("on_session_end"), "AgentEnd");
        assert_eq!(canonical_hook_event_name("gateway:startup"), "GatewayStartup");
        assert_eq!(canonical_hook_event_name("custom:event"), "custom:event");
    }
}
```

- [x] **Step 2: Run the tests and verify RED**

Run:

```bash
cargo test -p hooks event::tests -- --nocapture
```

Expected: compilation fails because `HookEvent` and `canonical_hook_event_name` are not defined.

- [x] **Step 3: Implement the event enum and canonicalizer**

Add the production portion above the tests in `event.rs`:

```rust
use std::borrow::Cow;
use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    PreToolUse,
    PermissionRequest,
    PostToolUse,
    PreCompact,
    PostCompact,
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    SubagentStart,
    SubagentStop,
    Stop,
    PreLlmCall,
    PreApiRequest,
    PostApiRequest,
    TransformTerminalOutput,
    TransformToolResult,
    TransformLlmOutput,
    PostLlmCall,
    PostApprovalResponse,
    PreGatewayDispatch,
    SessionReset,
    SessionFinalize,
    GatewayStartup,
    AgentEnd,
    CommandNewChat,
}

impl HookEvent {
    pub const CODEX: [Self; 11] = [
        Self::PreToolUse,
        Self::PermissionRequest,
        Self::PostToolUse,
        Self::PreCompact,
        Self::PostCompact,
        Self::SessionStart,
        Self::SessionEnd,
        Self::UserPromptSubmit,
        Self::SubagentStart,
        Self::SubagentStop,
        Self::Stop,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PreToolUse => "PreToolUse",
            Self::PermissionRequest => "PermissionRequest",
            Self::PostToolUse => "PostToolUse",
            Self::PreCompact => "PreCompact",
            Self::PostCompact => "PostCompact",
            Self::SessionStart => "SessionStart",
            Self::SessionEnd => "SessionEnd",
            Self::UserPromptSubmit => "UserPromptSubmit",
            Self::SubagentStart => "SubagentStart",
            Self::SubagentStop => "SubagentStop",
            Self::Stop => "Stop",
            Self::PreLlmCall => "PreLlmCall",
            Self::PreApiRequest => "PreApiRequest",
            Self::PostApiRequest => "PostApiRequest",
            Self::TransformTerminalOutput => "TransformTerminalOutput",
            Self::TransformToolResult => "TransformToolResult",
            Self::TransformLlmOutput => "TransformLlmOutput",
            Self::PostLlmCall => "PostLlmCall",
            Self::PostApprovalResponse => "PostApprovalResponse",
            Self::PreGatewayDispatch => "PreGatewayDispatch",
            Self::SessionReset => "SessionReset",
            Self::SessionFinalize => "SessionFinalize",
            Self::GatewayStartup => "GatewayStartup",
            Self::AgentEnd => "AgentEnd",
            Self::CommandNewChat => "CommandNewChat",
        }
    }
}

pub fn canonical_hook_event_name(name: &str) -> Cow<'_, str> {
    let canonical = match name {
        "pre_tool_call" => HookEvent::PreToolUse.as_str(),
        "post_tool_call" => HookEvent::PostToolUse.as_str(),
        "pre_approval_request" => HookEvent::PermissionRequest.as_str(),
        "pre_verify" => HookEvent::Stop.as_str(),
        "on_session_start" | "session:start" => HookEvent::SessionStart.as_str(),
        "on_session_end" | "agent:end" => HookEvent::AgentEnd.as_str(),
        "subagent_start" => HookEvent::SubagentStart.as_str(),
        "subagent_stop" => HookEvent::SubagentStop.as_str(),
        "pre_llm_call" => HookEvent::PreLlmCall.as_str(),
        "pre_api_request" => HookEvent::PreApiRequest.as_str(),
        "post_api_request" => HookEvent::PostApiRequest.as_str(),
        "transform_terminal_output" => HookEvent::TransformTerminalOutput.as_str(),
        "transform_tool_result" => HookEvent::TransformToolResult.as_str(),
        "transform_llm_output" => HookEvent::TransformLlmOutput.as_str(),
        "post_llm_call" => HookEvent::PostLlmCall.as_str(),
        "post_approval_response" => HookEvent::PostApprovalResponse.as_str(),
        "pre_gateway_dispatch" => HookEvent::PreGatewayDispatch.as_str(),
        "on_session_reset" => HookEvent::SessionReset.as_str(),
        "on_session_finalize" => HookEvent::SessionFinalize.as_str(),
        "gateway:startup" => HookEvent::GatewayStartup.as_str(),
        "command:new_chat" => HookEvent::CommandNewChat.as_str(),
        canonical => return Cow::Borrowed(canonical),
    };
    Cow::Borrowed(canonical)
}

pub fn normalize_hook_event_name(name: &str) -> Cow<'_, str> {
    let canonical = canonical_hook_event_name(name);
    if canonical.as_ref() != name {
        static WARNED: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
        let should_warn = WARNED
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .map(|mut warned| warned.insert(name.to_string()))
            .unwrap_or(false);
        if should_warn {
            tracing::warn!(legacy = name, canonical = %canonical, "legacy hook event name is deprecated");
        }
    }
    canonical
}
```

Replace `names.rs` with canonical constants backed by `HookEvent::as_str()`, followed by compatibility constants:

```rust
use crate::event::HookEvent;

pub const PRE_TOOL_USE: &str = HookEvent::PreToolUse.as_str();
pub const PERMISSION_REQUEST: &str = HookEvent::PermissionRequest.as_str();
pub const POST_TOOL_USE: &str = HookEvent::PostToolUse.as_str();
pub const PRE_COMPACT: &str = HookEvent::PreCompact.as_str();
pub const POST_COMPACT: &str = HookEvent::PostCompact.as_str();
pub const SESSION_START: &str = HookEvent::SessionStart.as_str();
pub const SESSION_END: &str = HookEvent::SessionEnd.as_str();
pub const USER_PROMPT_SUBMIT: &str = HookEvent::UserPromptSubmit.as_str();
pub const SUBAGENT_START: &str = HookEvent::SubagentStart.as_str();
pub const SUBAGENT_STOP: &str = HookEvent::SubagentStop.as_str();
pub const STOP: &str = HookEvent::Stop.as_str();

pub const PRE_LLM_CALL: &str = HookEvent::PreLlmCall.as_str();
pub const PRE_API_REQUEST: &str = HookEvent::PreApiRequest.as_str();
pub const POST_API_REQUEST: &str = HookEvent::PostApiRequest.as_str();
pub const TRANSFORM_TERMINAL_OUTPUT: &str = HookEvent::TransformTerminalOutput.as_str();
pub const TRANSFORM_TOOL_RESULT: &str = HookEvent::TransformToolResult.as_str();
pub const TRANSFORM_LLM_OUTPUT: &str = HookEvent::TransformLlmOutput.as_str();
pub const POST_LLM_CALL: &str = HookEvent::PostLlmCall.as_str();
pub const POST_APPROVAL_RESPONSE: &str = HookEvent::PostApprovalResponse.as_str();
pub const PRE_GATEWAY_DISPATCH: &str = HookEvent::PreGatewayDispatch.as_str();
pub const SESSION_RESET: &str = HookEvent::SessionReset.as_str();
pub const SESSION_FINALIZE: &str = HookEvent::SessionFinalize.as_str();
pub const GATEWAY_STARTUP: &str = HookEvent::GatewayStartup.as_str();
pub const AGENT_END: &str = HookEvent::AgentEnd.as_str();
pub const COMMAND_NEW_CHAT: &str = HookEvent::CommandNewChat.as_str();

// One-release source compatibility.
pub const PRE_TOOL_CALL: &str = PRE_TOOL_USE;
pub const POST_TOOL_CALL: &str = POST_TOOL_USE;
pub const PRE_APPROVAL_REQUEST: &str = PERMISSION_REQUEST;
pub const PRE_VERIFY: &str = STOP;
pub const ON_SESSION_START: &str = SESSION_START;
pub const ON_SESSION_END: &str = AGENT_END;
pub const ON_SESSION_RESET: &str = SESSION_RESET;
pub const ON_SESSION_FINALIZE: &str = SESSION_FINALIZE;

pub fn is_mutating_hook(name: &str) -> bool {
    matches!(
        crate::event::canonical_hook_event_name(name).as_ref(),
        PRE_LLM_CALL
            | PRE_TOOL_USE
            | PRE_GATEWAY_DISPATCH
            | STOP
            | TRANSFORM_TOOL_RESULT
            | TRANSFORM_TERMINAL_OUTPUT
            | TRANSFORM_LLM_OUTPUT
    )
}
```

Export `event` and its public types from `lib.rs`:

```rust
pub mod event;
pub use event::{HookEvent, canonical_hook_event_name, normalize_hook_event_name};
```

- [x] **Step 4: Run tests and verify GREEN**

Run:

```bash
cargo test -p hooks event::tests names:: -- --nocapture
```

Expected: all event and name tests pass.

- [x] **Step 5: Commit the canonical event contract**

```bash
git add crates/agent-hooks/src/event.rs crates/agent-hooks/src/names.rs crates/agent-hooks/src/lib.rs
git commit -m "refactor(hooks): define codex event names"
```

### Task 2: Add the Codex-compatible HookInput wire model

**Files:**
- Modify: `crates/agent-hooks/src/outcome.rs`
- Modify: `crates/agent-hooks/src/lib.rs`
- Test: `crates/agent-hooks/src/outcome.rs`

- [x] **Step 1: Write failing wire-shape tests**

Add these tests before changing the production struct:

```rust
#[cfg(test)]
mod wire_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn pre_tool_use_serializes_codex_variable_names() {
        let input = HookInput {
            session_id: "s1".into(),
            turn_id: Some("t1".into()),
            transcript_path: None,
            cwd: "/workspace".into(),
            hook_event_name: "PreToolUse".into(),
            model: "openai/gpt-5.6".into(),
            permission_mode: Some("default".into()),
            tool_name: Some("Bash".into()),
            tool_use_id: Some("call-1".into()),
            tool_input: Some(json!({"command": "cargo test"})),
            ..Default::default()
        };
        assert_eq!(
            serde_json::to_value(input).unwrap(),
            json!({
                "session_id": "s1",
                "turn_id": "t1",
                "transcript_path": null,
                "cwd": "/workspace",
                "hook_event_name": "PreToolUse",
                "model": "openai/gpt-5.6",
                "permission_mode": "default",
                "tool_name": "Bash",
                "tool_use_id": "call-1",
                "tool_input": {"command": "cargo test"}
            })
        );
    }

    #[test]
    fn astro_ui_fields_are_not_part_of_command_wire_json() {
        let input = HookInput {
            session_id: "s1".into(),
            cwd: "/workspace".into(),
            hook_event_name: "AgentEnd".into(),
            detail: "turn=1".into(),
            assistant_chars: Some(42),
            ..Default::default()
        };
        let value = serde_json::to_value(input).unwrap();
        assert!(value.get("detail").is_none());
        assert!(value.get("assistant_chars").is_none());
    }

    #[test]
    fn hook_payload_remains_a_source_compatible_alias() {
        let _: HookPayload = HookInput::default();
    }
}
```

- [x] **Step 2: Run the tests and verify RED**

Run:

```bash
cargo test -p hooks outcome::wire_tests -- --nocapture
```

Expected: compilation fails because `HookInput` and canonical fields do not exist.

- [x] **Step 3: Implement HookInput while retaining temporary legacy construction fields**

Replace the current `HookPayload` struct with:

```rust
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize)]
pub struct HookInput {
    pub session_id: String,
    pub transcript_path: Option<String>,
    pub cwd: String,
    pub hook_event_name: String,
    pub model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permission_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_use_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_response: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trigger: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_transcript_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stop_hook_active: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,

    #[serde(skip)]
    pub detail: String,
    #[serde(skip)]
    pub system_prompt_chars: Option<usize>,
    #[serde(skip)]
    pub assistant_chars: Option<usize>,
    #[serde(skip)]
    pub turn: Option<usize>,
    #[serde(skip)]
    pub error: Option<String>,

    // Temporary one-batch construction compatibility. Task 4 removes these.
    #[serde(skip)]
    pub tool_args: Option<Value>,
    #[serde(skip)]
    pub tool_result: Option<String>,
    #[serde(skip)]
    pub message: Option<String>,
}

impl HookInput {
    pub fn normalized_for_event(&self, event_name: &str) -> Self {
        let mut normalized = self.clone();
        normalized.hook_event_name =
            crate::event::canonical_hook_event_name(event_name).into_owned();
        if normalized.tool_input.is_none() {
            normalized.tool_input = normalized.tool_args.clone();
        }
        if normalized.tool_response.is_none() {
            normalized.tool_response = normalized.tool_result.clone().map(Value::String);
        }
        if normalized.prompt.is_none() {
            normalized.prompt = normalized.message.clone();
        }
        normalized
    }
}

pub type HookPayload = HookInput;
```

Keep the existing `HookOutcome` enum unchanged in this batch.

Update the public re-export in `lib.rs`:

```rust
pub use outcome::{HookInput, HookOutcome, HookPayload};
```

- [x] **Step 4: Run tests and verify GREEN**

Run:

```bash
cargo test -p hooks outcome::wire_tests -- --nocapture
```

Expected: all three wire tests pass.

- [x] **Step 5: Commit the wire input model**

```bash
git add crates/agent-hooks/src/outcome.rs crates/agent-hooks/src/lib.rs
git commit -m "refactor(hooks): add codex input variables"
```

### Task 3: Normalize names at every Hook boundary

**Files:**
- Modify: `crates/agent-hooks/src/plugin.rs`
- Modify: `crates/agent-hooks/src/gateway.rs`
- Modify: `crates/agent-hooks/src/shell.rs`
- Modify: `crates/agent-hooks/src/ui.rs`
- Modify: `crates/agent-hooks/src/lib.rs`
- Test: the same files' existing unit-test modules

- [x] **Step 1: Write failing boundary tests**

Add these focused tests:

```rust
// plugin.rs
#[test]
fn legacy_registration_and_canonical_fire_share_one_slot() {
    let bus = PluginHookBus::new();
    let seen = Arc::new(std::sync::Mutex::new(None));
    let capture = Arc::clone(&seen);
    bus.register("pre_tool_call", move |input| {
        *capture.lock().unwrap() = Some(input.hook_event_name.clone());
        HookOutcome::Continue
    });
    bus.fire("PreToolUse", &crate::HookInput::default());
    assert_eq!(seen.lock().unwrap().as_deref(), Some("PreToolUse"));
}

// gateway.rs
#[test]
fn legacy_manifest_event_matches_canonical_dispatch() {
    let dir = tempfile::tempdir().unwrap();
    let hook_dir = dir.path().join("hooks").join("audit");
    std::fs::create_dir_all(&hook_dir).unwrap();
    std::fs::write(
        hook_dir.join("HOOK.yaml"),
        "name: audit\nevents:\n  - gateway:startup\n",
    )
    .unwrap();
    let registry = GatewayHookRegistry::new();
    registry.discover(dir.path()).unwrap();
    let hits = Arc::new(AtomicUsize::new(0));
    let capture = Arc::clone(&hits);
    registry.register_handler("audit", move |event, _| {
        assert_eq!(event, "GatewayStartup");
        capture.fetch_add(1, Ordering::SeqCst);
    });
    registry.fire("GatewayStartup", &crate::HookInput::default());
    assert_eq!(hits.load(Ordering::SeqCst), 1);
}

// shell.rs
#[test]
fn legacy_yaml_key_is_stored_under_canonical_name() {
    let runner = ShellHookRunner::new(HashMap::from([(
        "post_tool_call".to_string(),
        "true".to_string(),
    )]));
    assert!(runner.has_event("PostToolUse"));
}

// ui.rs
#[test]
fn timeline_emits_canonical_name_for_legacy_fire() {
    let bus = PluginHookBus::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    install_ui_timeline(&bus, tx);
    bus.fire("pre_tool_call", &crate::HookInput::default());
    assert_eq!(rx.try_recv().unwrap().name, "PreToolUse");
}
```

- [x] **Step 2: Run the tests and verify RED**

Run:

```bash
cargo test -p hooks legacy_ -- --nocapture
cargo test -p hooks timeline_emits_canonical_name_for_legacy_fire -- --nocapture
```

Expected: failures show that registration, discovery, shell maps, or UI still use raw names.

- [x] **Step 3: Canonicalize each boundary**

Apply these exact rules:

```rust
// PluginHookBus::register
let name = name.into();
let name = crate::event::normalize_hook_event_name(&name).into_owned();

// PluginHookBus::fire
let name = crate::event::normalize_hook_event_name(name);
let payload = payload.normalized_for_event(name.as_ref());
// Look up callbacks with name.as_ref() and invoke each callback with &payload.

// GatewayHookRegistry::fire
let event = crate::event::normalize_hook_event_name(event);
// Compare canonical_hook_event_name(manifest_event).as_ref() with event.as_ref().
// Pass event.as_ref() to handlers.

// ShellHookRunner::new
let commands = commands
    .into_iter()
    .map(|(event, command)| {
        (crate::event::normalize_hook_event_name(&event).into_owned(), command)
    })
    .collect();

// ShellHookRunner::fire_async and fire_await
let event = crate::event::canonical_hook_event_name(event);
// Lookup and emit the canonical event.
```

Add a private test-only helper to `ShellHookRunner`:

```rust
#[cfg(test)]
fn has_event(&self, event: &str) -> bool {
    self.commands
        .contains_key(crate::event::canonical_hook_event_name(event).as_ref())
}
```

Replace `UI_HOOK_NAMES` entries with the canonical constants from `names.rs`. In `detail_from_payload`, use `tool_input`, `prompt`, and `last_assistant_message`; do not read legacy fields.

In `HookRuntime::fire_plugin` and `fire_gateway`, normalize once before forwarding so Plugin, Gateway, and Shell observe the same label.

- [x] **Step 4: Run tests and verify GREEN**

Run:

```bash
cargo test -p hooks -- --nocapture
```

Expected: all `hooks` crate tests pass, including existing order/block tests.

- [x] **Step 5: Commit boundary normalization**

```bash
git add crates/agent-hooks/src/plugin.rs crates/agent-hooks/src/gateway.rs crates/agent-hooks/src/shell.rs crates/agent-hooks/src/ui.rs crates/agent-hooks/src/lib.rs
git commit -m "refactor(hooks): normalize legacy hook names"
```

### Task 4: Migrate internal callers to canonical constants and variables

**Files:**
- Modify: `crates/agent-core/src/exec/subagents.rs`
- Modify: `crates/agent-core/src/runtime/turn_lifecycle.rs`
- Modify: `crates/agent-core/src/runtime/tool_dispatch.rs`
- Modify: `crates/agent-core/src/streaming/maintenance.rs`
- Modify: `crates/agent-core/src/streaming/multi_turn.rs`
- Modify: `crates/agent-core/src/streaming/tools_exec.rs`
- Modify: `crates/agent-tools/src/builtin/shell/terminal.rs`
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Modify: `crates/agent-core/tests/rig_agent_test.rs`
- Modify: `crates/agent-core/tests/streaming_test.rs`
- Modify: `crates/agent-hooks/src/outcome.rs`
- Test: `crates/agent-core/tests/rig_agent_test.rs`
- Test: `crates/agent-core/tests/streaming_test.rs`
- Test: `crates/agent-tools/src/builtin/shell/terminal.rs`

- [x] **Step 1: Update assertions first to require canonical variables**

Change focused test callbacks and assertions before production call sites:

```rust
// rig_agent_test.rs post-tool capture
*captured2.lock().unwrap() = payload
    .tool_response
    .as_ref()
    .and_then(serde_json::Value::as_str)
    .map(str::to_string);

// streaming_test.rs approval capture
*pre_seen.lock().unwrap() = Some((
    payload.tool_input.clone(),
    payload.detail.clone(),
));

// Assert the terminal approval input shape.
assert_eq!(
    pre_payload.0,
    Some(serde_json::json!({
        "command": "rm -rf ./tmp",
        "description": "clean up the temp dir"
    }))
);
```

Add an event-name assertion to one integration test:

```rust
assert!(events.iter().any(|event| event == "PreToolUse"));
assert!(!events.iter().any(|event| event == "pre_tool_call"));
```

- [x] **Step 2: Run focused tests and verify RED**

Run:

```bash
cargo test -p agent --test rig_agent_test pre_tool_call_block_via_hook_bus -- --nocapture
cargo test -p agent --test streaming_test approval_hooks_fire_pre_then_post_on_allow -- --nocapture
```

Expected: assertions fail or compilation fails because production call sites still populate legacy fields.

- [x] **Step 3: Migrate constants and struct fields mechanically**

Use this exact constant replacement table in the listed production and test files:

| Old constant | Canonical constant |
|---|---|
| `ON_SESSION_START` | `SESSION_START` |
| `ON_SESSION_END` | `AGENT_END` |
| `ON_SESSION_RESET` | `SESSION_RESET` |
| `ON_SESSION_FINALIZE` | `SESSION_FINALIZE` |
| `PRE_TOOL_CALL` | `PRE_TOOL_USE` |
| `POST_TOOL_CALL` | `POST_TOOL_USE` |
| `PRE_APPROVAL_REQUEST` | `PERMISSION_REQUEST` |
| `PRE_VERIFY` | `STOP` |

Use these exact field transformations:

```rust
// Tool input
tool_input: Some(args.clone()),

// Tool output
tool_response: Some(serde_json::Value::String(raw_text.clone())),

// User prompt at the gateway
prompt: Some(content.clone()),

// Final assistant text for Stop / transform events
last_assistant_message: Some(full_response.clone()),

// Terminal approval request
tool_name: Some("Bash".into()),
tool_input: Some(serde_json::json!({
    "command": command,
    "description": request_summary,
})),
```

For `PostApprovalResponse`, retain the command/summary in `tool_input` and put the decision label in `detail`. For `TransformTerminalOutput`, populate `tool_input` and `tool_response`. For `PostToolUse`, populate both `tool_input` and `tool_response` so a later command runner can serialize the exact Codex shape without reopening session state.

After all call sites and tests compile, delete the temporary `tool_args`, `tool_result`, and `message` fields plus fallback copying from `HookInput::normalized_for_event`. Keep only event-name normalization in that method.

- [x] **Step 4: Verify no internal legacy usage remains**

Run:

```bash
rg -n "tool_args:|tool_result:|message: Some\(" crates/agent-hooks crates/agent-core crates/agent-tools crates/agent-server
rg -n "ON_SESSION_START|ON_SESSION_END|PRE_TOOL_CALL|POST_TOOL_CALL|PRE_APPROVAL_REQUEST|PRE_VERIFY" crates/agent-core crates/agent-tools crates/agent-server crates/agent-hooks/src/ui.rs
```

Expected: no matches from Hook construction or internal dispatch. Compatibility definitions in `names.rs` are allowed and are excluded by the second command's path list.

- [x] **Step 5: Run focused tests and verify GREEN**

Run:

```bash
cargo test -p agent --test rig_agent_test -- --nocapture
cargo test -p agent --test streaming_test approval_hooks_fire -- --nocapture
cargo test -p tools transform_terminal_output_hook_replaces_before_truncation -- --nocapture
cargo test -p server new_chat_preserves_hooks_while_release_session_skips_them -- --nocapture
```

Expected: all focused Hook integration tests pass with canonical event labels and variables.

- [x] **Step 6: Commit internal migration**

```bash
git add crates/agent-core/src/exec/subagents.rs crates/agent-core/src/runtime/turn_lifecycle.rs crates/agent-core/src/runtime/tool_dispatch.rs crates/agent-core/src/streaming/maintenance.rs crates/agent-core/src/streaming/multi_turn.rs crates/agent-core/src/streaming/tools_exec.rs crates/agent-tools/src/builtin/shell/terminal.rs crates/agent-server/src/grpc/astro_service.rs crates/agent-core/tests/rig_agent_test.rs crates/agent-core/tests/streaming_test.rs crates/agent-hooks/src/outcome.rs
git commit -m "refactor(hooks): use codex hook variables"
```

### Task 5: Preserve environment compatibility and document the migration

**Files:**
- Modify: `crates/agent-hooks/src/shell.rs`
- Modify: `docs/hooks.md`
- Modify: `docs/examples/hooks/README.md`
- Modify: `docs/examples/hooks/config.yaml.snippet`
- Test: `crates/agent-hooks/src/shell.rs`

- [x] **Step 1: Write failing legacy-environment tests**

Replace the current environment tests with canonical-input assertions:

```rust
#[test]
fn legacy_env_is_derived_from_canonical_input() {
    let env = env_from_payload(
        "PreToolUse",
        &crate::HookInput {
            session_id: "s1".into(),
            turn_id: Some("turn-abc".into()),
            hook_event_name: "PreToolUse".into(),
            tool_name: Some("Bash".into()),
            tool_input: Some(serde_json::json!({"command": "cargo test"})),
            prompt: Some("run tests".into()),
            detail: "Bash cargo test".into(),
            ..Default::default()
        },
    );
    assert!(env.iter().any(|(k, v)| k == "ASTRO_HOOK_EVENT" && v == "PreToolUse"));
    assert!(env.iter().any(|(k, v)| k == "ASTRO_HOOK_TOOL" && v == "Bash"));
    assert!(env.iter().any(|(k, v)| k == "ASTRO_HOOK_MESSAGE" && v == "run tests"));
    assert!(env.iter().any(|(k, v)| k == "ASTRO_HOOK_TURN" && v == "turn-abc"));
}
```

- [x] **Step 2: Run the test and verify RED**

Run:

```bash
cargo test -p hooks legacy_env_is_derived_from_canonical_input -- --nocapture
```

Expected: failure because `env_from_payload` still reads the removed `message` field or emits a legacy event label.

- [x] **Step 3: Update environment derivation and documentation**

Implement the compatibility message fallback:

```rust
if let Some(message) = payload
    .prompt
    .as_ref()
    .or(payload.last_assistant_message.as_ref())
{
    env.push(("ASTRO_HOOK_MESSAGE".into(), message.clone()));
}
```

Always set `ASTRO_HOOK_EVENT` from `canonical_hook_event_name(event)`.

Update `docs/hooks.md` with:

- the eleven Codex names and Astro extension names;
- the legacy-to-canonical mapping table from the approved design;
- the canonical JSON variable table (`hook_event_name`, `tool_input`, `tool_response`, `prompt`, `last_assistant_message`);
- an explicit batch-A note that lifecycle timing and Command Hook JSON stdin arrive in later batches;
- a one-release deprecation statement for snake_case names and `ASTRO_HOOK_*` variables.

Update YAML examples to use canonical keys such as `PostToolUse`, `PreLlmCall`, and `AgentEnd`, and show the equivalent accepted legacy key in a commented migration example.

- [x] **Step 4: Run tests and full batch verification**

Run:

```bash
cargo fmt --all -- --check
cargo test -p hooks
cargo test -p agent --test rig_agent_test
cargo test -p agent --test streaming_test
cargo test -p tools transform_terminal_output_hook_replaces_before_truncation
cargo test -p server new_chat_preserves_hooks_while_release_session_skips_them
cargo check --workspace --all-targets
git diff --check
```

Expected: every command exits 0, all tests report zero failures, and `git diff --check` prints nothing.

- [x] **Step 5: Commit compatibility docs and batch verification changes**

```bash
git add crates/agent-hooks/src/shell.rs docs/hooks.md docs/examples/hooks/README.md docs/examples/hooks/config.yaml.snippet
git commit -m "docs(hooks): document codex naming migration"
```

## Completion checklist

- [x] All eleven Codex names exactly match OpenAI's PascalCase labels.
- [x] Astro extension names are PascalCase and distinct from official events.
- [x] Every old event name maps to one canonical name at all boundaries.
- [x] New Hook JSON contains Codex variable names and omits Astro UI-only fields.
- [x] Internal production call sites no longer construct `tool_args`, `tool_result`, or `message` Hook fields.
- [x] UI and logging emit canonical names only.
- [x] Existing YAML keys and `ASTRO_HOOK_*` variables remain usable for one release.
- [x] Focused tests and `cargo check --workspace --all-targets` pass.
