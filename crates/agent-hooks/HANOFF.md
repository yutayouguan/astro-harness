# Codex Hook Alignment Handoff

This file records the production contract for the Codex-compatible hook surface. It is intentionally named `HANOFF.md` to match the requested handoff artifact.

## Canonical runtime path

The five alignment areas covered by this change enter through the session-owned `HookRuntime`:

```text
business boundary
  -> Session::fire_hook / typed Session hook method
  -> HookRuntime
       -> PluginHookBus (synchronous decisions)
       -> GatewayHookRegistry (observer)
       -> ShellHookRunner (asynchronous observer)
       -> UI timeline slot
```

Child Agent Threads inherit the complete `Arc<HookRuntime>` from the parent runtime material. They must not call `PluginHookBus` directly. Gateway and Shell transports remain observers; decisions are produced synchronously by the Plugin bus and applied by the owning business boundary.

## Aligned lifecycle events

| Event | Production boundary | Control semantics |
|---|---|---|
| `PreCompact` | Immediately before a non-empty automatic compression plan, or before Desktop manual compaction | `Block`/`Skip` prevents automatic/manual compaction and stops the active automatic turn |
| `PostCompact` | After compressed views are durably updated, or after Desktop manual split succeeds | `Block`/`Skip` stops the active automatic turn; completed side effects are not rolled back |
| `SessionEnd` | Once during session-owned runtime shutdown; the no-live-runtime new-chat fallback emits the same canonical event | Observer; `reason="other"` |
| `PermissionRequest` | Before policy/HITL approval | Any `Deny` wins; otherwise any `Allow` grants; all abstain continues the normal policy/HITL path |
| `PostToolUse` | After the tool side effect and `TransformToolResult` | Block replaces the model-visible result; additional contexts and feedback are appended; media assets are retained |
| `SubagentStart` | Child startup admission, before its first provider request | Uses the inherited full `HookRuntime`; only additional context is applied, while block/stop requests are ignored as in Codex |
| `SubagentStop` | Every child terminal turn, including interrupted/errored turns | Same `KeepGoing` continuation guard as root `Stop`; Desktop `Shutdown` does not emit a duplicate stop |

`SessionFinalize` remains an Astro extension constant for compatibility, but it is not used as the canonical session-shutdown lifecycle event.

## Payloads

- Compact events set `trigger` to `auto` or `manual`.
- Session end sets `reason="other"`.
- Permission and tool events carry canonical tool name/input/response fields.
- Subagent events set `agent_id`, `agent_type`, `turn_id` where available, and include the canonical path in the internal detail string.
- `SubagentStop` carries `last_assistant_message` on normal terminal candidates and `stop_hook_active` during continuation retries.

## Decision aggregation

`PermissionRequest` and `PostToolUse` deliberately do not use the generic first-non-continue rule:

- permission callbacks are all evaluated; deny has precedence over allow;
- post-tool callbacks aggregate the first block reason, every additional context, and every feedback message;
- subagent-start callbacks ignore block/stop requests and combine all injected context;
- subagent-stop callbacks combine all continuation prompts for the same terminal candidate;
- callback panics are contained and treated as abstain/continue.

Tool side effects are never rolled back by `PostToolUse`. A block controls only the result exposed to the next model sampling step.

## Regression coverage

Keep tests for these invariants when changing hooks:

1. automatic compression observes `PreCompact -> PostCompact` and reports `trigger=auto`;
2. permission allow bypasses HITL and permission deny prevents dispatch;
3. post-tool block/context/feedback changes the model-visible text without dropping media;
4. a child start is emitted once, while child stop is emitted for every terminal turn and never duplicated by Desktop close;
5. concurrent/repeated shutdown emits one `SessionEnd` only after the active task has completed;
6. Desktop manual compaction emits `manual` compact boundaries when a live Session exists.

## Known transport boundary

Plugin handlers are the synchronous decision source. Gateway manifests and legacy `config.yaml` Shell commands receive the same canonical events but are observational because their current APIs do not return a structured decision. Adding Codex `hooks.json` command-response parsing should extend `HookRuntime` rather than creating another business-layer hook path.
