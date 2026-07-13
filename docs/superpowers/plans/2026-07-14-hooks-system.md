# Hooks System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 落地 Astro 三套 Hook 体系（Plugin / Gateway / Shell），钩子名对齐 Hermes，UI 与 memory 分流，并交付 `docs/hooks.md`。

**Architecture:** 新建 `hooks` crate；Agent 主循环经 `PluginHookBus` 点火；backend 负责 Gateway 入站与启动扫描；Shell 旁路异步执行；proto 增加 `HookEvent`。

**Tech Stack:** Rust workspace、tokio、serde_yaml、tonic/proto、现有 agent streaming / Tauri 前端

**Spec:** [`docs/superpowers/specs/2026-07-14-hooks-system-design.md`](../specs/2026-07-14-hooks-system-design.md)

---

## File map

| Path | Responsibility |
|------|----------------|
| `hooks/` (new crate) | PluginHookBus、Gateway、Shell、PluginContext、names、UiTimeline |
| `agent/src/hooks.rs` | 薄 re-export 或适配旧 API → 新 bus |
| `agent/src/loop_.rs` / `streaming.rs` / `delegate_exec.rs` | 点火点 |
| `backend/src/grpc/astro_service.rs` | pre_gateway_dispatch、挂 UI hooks、gateway 事件 |
| `proto/proto/astro.proto` | `HookEvent` oneof |
| `frontend/...` | 消费 HookEvent；`showHooks` |
| `permissions/src/hooks.rs` | 转发或删除重复 |
| `docs/hooks.md` | 用户/开发文档 |

---

### Task 1: Create `hooks` crate skeleton + PluginHookBus

**Files:**
- Create: `hooks/Cargo.toml`, `hooks/src/lib.rs`, `hooks/src/names.rs`, `hooks/src/plugin/mod.rs`, `hooks/src/context.rs`
- Modify: root `Cargo.toml` workspace members
- Test: `hooks/src/plugin/mod.rs` (unit tests) or `hooks/tests/plugin_bus.rs`

- [x] **Step 1:** Add workspace member `hooks` with deps: `tokio`, `serde`, `serde_json`, `anyhow`, `async-trait`, `tracing`, `serde_yaml`, `thiserror`
- [x] **Step 2:** Define `names` constants (`PRE_LLM_CALL`, …) matching spec
- [x] **Step 3:** Define `HookPayload`, `HookOutcome` (`Continue` / `Block` / `Modify` / `InjectContext` / `Allow` / `Skip` / `Rewrite`)
- [x] **Step 4:** Implement `PluginHookBus::register` + `fire` (ordered, first non-Continue short-circuit for mutating outcomes)
- [x] **Step 5:** `PluginContext::register_hook(name, callback)`
- [x] **Step 6:** Unit tests: order, Block, Modify, InjectContext
- [x] **Step 7:** `cargo test -p hooks`

### Task 2: Gateway + Shell + config

**Files:**
- Create: `hooks/src/gateway/mod.rs`, `hooks/src/shell/mod.rs`, `hooks/src/config.rs`
- Test: unit tests in same modules

- [x] **Step 1:** Load `~/.astro/config.yaml` (`ASTRO_MEMORY_DIR` aware) → `hooks:` map
- [x] **Step 2:** `ShellHookRunner::fire(event, env)` with timeout 5s
- [x] **Step 3:** Discover `~/.astro/hooks/*/HOOK.yaml`; `GatewayHookRegistry` bind Rust handlers by name
- [x] **Step 4:** Tests with tempfile dirs
- [x] **Step 5:** `cargo test -p hooks`

### Task 3: UiTimelineHooks + proto HookEvent

**Files:**
- Create: `hooks/src/ui.rs`
- Modify: `proto/proto/astro.proto`, regenerate / update proto rust
- Modify: `backend` + `frontend/src-tauri` event mapping
- Modify: `frontend/src/App.tsx`（不再走 memory_update）

- [x] **Step 1:** Add `message HookEvent { string name = 1; string detail = 2; string outcome = 3; }` to `ChatEvent` oneof
- [x] **Step 2:** Rebuild proto; map in backend `multi_turn` / hook channel
- [x] **Step 3:** Tauri emit `hook` type; App.tsx create `kind: "hook"` activity
- [x] **Step 4:** Remove ChannelHooks→MemoryUpdate path

### Task 4: Wire Agent lifecycle (Plugin Hooks)

**Files:**
- Modify: `agent/src/loop_.rs`, `agent/src/streaming.rs`, `agent/src/builder.rs`, `agent/src/lib.rs`
- Modify: `agent/src/hooks.rs` → adapt or re-export
- Modify: `agent/src/delegate_exec.rs` for `subagent_stop`
- Test: update `agent/tests/*` RecordingHooks expectations

- [x] **Step 1:** Agent holds `Arc<PluginHookBus>` (or adapter implementing old trait calling new names)
- [x] **Step 2:** Fire `on_session_start` / `pre_llm_call` / API / tool / `post_llm_call` / `on_session_end` per spec order
- [x] **Step 3:** Honor InjectContext / Block / Modify
- [x] **Step 4:** `subagent_stop` after child completes
- [x] **Step 5:** Fix tests; `cargo test -p agent`

### Task 5: Backend Gateway wiring

**Files:**
- Modify: `backend/src/grpc/astro_service.rs` (and startup)
- Modify: `backend/Cargo.toml` add `hooks`

- [x] **Step 1:** On server start: load config, shell runner, gateway registry, `gateway:startup`
- [x] **Step 2:** Chat inbound: `pre_gateway_dispatch`; Skip short-circuit
- [x] **Step 3:** `session:start` / `agent:end` / new_chat → `command:new_chat` + `on_session_reset` / `on_session_finalize`
- [x] **Step 4:** Attach UiTimelineHooks to agent sessions
- [x] **Step 5:** Also fire ShellHookRunner alongside plugin events where configured

### Task 6: permissions cleanup + docs

**Files:**
- Modify: `permissions/src/hooks.rs` / `lib.rs` — deprecate or forward
- Create: `docs/hooks.md`
- Modify: spec status → 已批准 / 实现中

- [x] **Step 1:** Remove duplicate HookBus or thin-wrap `hooks::plugin`
- [x] **Step 2:** Write `docs/hooks.md` (三套体系、示例、配置、事件表、UI)
- [x] **Step 3:** `cargo test` workspace smoke; frontend typecheck if needed

---

## Done when

验收标准见 spec「验收标准」一节。
