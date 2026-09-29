# agent crate 域重组 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [x]`) syntax for tracking.

**Goal:** 将 `agent` crate 从平铺 `src/*.rs` 重组为 `runtime` / `streaming` / `control` / `prompt` / `exec` 域目录，拆开 `streaming.rs` 与 `loop_.rs`，并按 B 收紧 crate 根导出。

**Architecture:** 按运行时阶段分目录；巨石文件按职责切开；`lib.rs` 只 `pub use` 产品入口（Builder / Loop / 流式入口 / HITL·Interrupt / ToolRegistry）。旧路径（`loop_`、`cron_exec` 等）不保留对外兼容别名。

**Tech Stack:** Rust workspace、`agent` / `backend` / `astro-harness`（Tauri）、现有 `cargo test -p agent` 集成测试。

**Spec:** `docs/superpowers/specs/2026-07-16-agent-crate-reorg-design.md`

## Global Constraints

- 不改流式 / 主循环 / HITL / cron / delegate 运行时语义；本轮只搬家、拆文件、改路径与可见性。
- 不拆成多个 crate；不改前端 / proto。
- 对外不保留 `agent::loop_` / `agent::cron_exec` 等旧路径兼容层。
- 单文件目标约 400–600 行；`delegate` 超标可留 Task 7 再拆，不阻塞主路径。
- 每个 Task 结束：`cargo test -p agent` 通过；涉及调用方时再 `cargo check -p backend` / `cargo check -p astro-harness`。
- 频繁提交；一次 Task 一到数个 commit。
- 若工作区已有无关 WIP（如 session 硬切），先 stash / 另分支，避免混进本计划 diff。

## File Structure

| Path | Responsibility |
|------|----------------|
| `crates/agent-core/src/lib.rs` | 域 `mod` + B 根导出 |
| `crates/agent-core/src/builder.rs` | 保持顶层 |
| `crates/agent-core/src/event_bus.rs` | 保持顶层 |
| `crates/agent-core/src/timeline.rs` | 保持顶层 |
| `crates/agent-core/src/prompt/` | context、context_usage、prompt_builder、messages、hooks |
| `crates/agent-core/src/control/` | hitl、interrupt、smart_approval、schema_validate |
| `crates/agent-core/src/exec/` | cron、delegate、orchestration、multi_agent、memory_review |
| `crates/agent-core/src/runtime/` | AgentLoop 主体 + session/validate/budget/usage |
| `crates/agent-core/src/streaming/` | types/traits/provider/multi_turn/hitl_bridge/summary/fallback |
| `crates/agent-server/src/grpc/astro_service.rs` | 更新 `use` / `ImageGenTargets` / memory_review 路径 |
| `crates/agent-server/src/cron_runner.rs` | `agent::exec::cron` |
| `crates/agent-server/src/grpc/interrupt_store.rs` | 根 HITL/Interrupt 可不变；注释路径更新 |
| `apps/desktop/src-tauri/src/commands.rs` | `agent::cron_exec` → `agent::exec::cron` |
| `apps/desktop/src-tauri/src/memory_commands.rs` | 文档链接 `loop_` → `runtime` |
| `agent/tests/*.rs` | 全部 `use` 路径 |
| 文档注释 | `docs/hooks.md`、crate 头注释中的旧路径 |

内部路径映射（搬家后全局替换 `agent/src`）：

| 旧 | 新 |
|---|---|
| `crate::loop_` | `crate::runtime` |
| `crate::iteration_budget` | `crate::runtime::budget` |
| `crate::usage_record` | `crate::runtime::usage` |
| `crate::hitl` | `crate::control::hitl` |
| `crate::interrupt` | `crate::control::interrupt` |
| `crate::smart_approval` | `crate::control::smart_approval` |
| `crate::schema_validate` | `crate::control::schema_validate` |
| `crate::context` | `crate::prompt::context` |
| `crate::context_usage` | `crate::prompt::context_usage` |
| `crate::prompt_builder` | `crate::prompt::prompt_builder` |
| `crate::messages` | `crate::prompt::messages` |
| `crate::hooks` | `crate::prompt::hooks` |
| `crate::chat_fallback` | `crate::streaming::fallback` |
| `crate::cron_exec` | `crate::exec::cron` |
| `crate::delegate_exec` | `crate::exec::delegate` |
| `crate::orchestration` | `crate::exec::orchestration` |
| `crate::multi_agent` | `crate::exec::multi_agent` |
| `crate::memory_review_spawn` | `crate::exec::memory_review` |

---

### Task 1: 搬 `prompt` + `control` 域

**Files:**
- Create: `crates/agent-core/src/prompt/mod.rs`
- Create: `crates/agent-core/src/control/mod.rs`
- Move: `context.rs` → `prompt/context.rs`；`context_usage.rs` → `prompt/context_usage.rs`；`prompt_builder.rs` → `prompt/prompt_builder.rs`；`messages.rs` → `prompt/messages.rs`；`hooks.rs` → `prompt/hooks.rs`
- Move: `hitl.rs` → `control/hitl.rs`；`interrupt.rs` → `control/interrupt.rs`；`smart_approval.rs` → `control/smart_approval.rs`；`schema_validate.rs` → `control/schema_validate.rs`
- Modify: `crates/agent-core/src/lib.rs`（临时仍从新路径 re-export 旧根符号，保证本 Task 后外部 `use agent::StaticContext` 等仍编译）
- Modify: 所有 `crate::context` / `crate::hitl` 等内部引用（见上表）

**Interfaces:**
- Consumes: 现有各模块公开 API（签名不变）
- Produces: `pub mod prompt` / `pub mod control`；本 Task **暂不**收紧根导出（留给 Task 5）

- [x] **Step 1: 创建目录并 git mv 文件**

```bash
cd agent/src
mkdir -p prompt control
git mv context.rs prompt/context.rs
git mv context_usage.rs prompt/context_usage.rs
git mv prompt_builder.rs prompt/prompt_builder.rs
git mv messages.rs prompt/messages.rs
git mv hooks.rs prompt/hooks.rs
git mv hitl.rs control/hitl.rs
git mv interrupt.rs control/interrupt.rs
git mv smart_approval.rs control/smart_approval.rs
git mv schema_validate.rs control/schema_validate.rs
```

- [x] **Step 2: 写 `prompt/mod.rs` 与 `control/mod.rs`**

`crates/agent-core/src/prompt/mod.rs`:

```rust
pub mod context;
pub mod context_usage;
pub mod hooks;
pub mod messages;
pub mod prompt_builder;
```

`crates/agent-core/src/control/mod.rs`:

```rust
pub mod hitl;
pub mod interrupt;
pub mod schema_validate;
pub mod smart_approval;
```

- [x] **Step 3: 更新 `lib.rs` 模块声明（保留旧根 re-export）**

将原先的 `pub mod context;` 等替换为：

```rust
pub mod control;
pub mod prompt;
// …其余未搬模块保持不变
```

根 re-export 改为从新路径拉出，例如：

```rust
pub use prompt::context::{DynamicContext, StaticContext};
pub use prompt::context_usage::{build_snapshot, ContextUsageSegment, ContextUsageSnapshot};
pub use prompt::hooks::{CancelSignal, PromptCancelled};
pub use control::hitl::{
    is_exclusive_tool, is_interactive_tool, HitlGate, HitlRegistry, HitlRequest, HitlResolution,
    HITL_DEFAULT_TIMEOUT_SECS,
};
pub use control::interrupt::{Interrupt, InterruptError, InterruptPending, ResumeItem};
```

删除对已搬走文件的顶层 `pub mod context;` / `pub mod hitl;` 等。

- [x] **Step 4: 更新 crate 内 `use` 路径**

在 `agent/src` 内批量替换（可用 IDE / `rg` 核对后手动改）：

- `crate::context` → `crate::prompt::context`
- `crate::context_usage` → `crate::prompt::context_usage`
- `crate::prompt_builder` → `crate::prompt::prompt_builder`
- `crate::messages` → `crate::prompt::messages`
- `crate::hooks` → `crate::prompt::hooks`
- `crate::hitl` → `crate::control::hitl`
- `crate::interrupt` → `crate::control::interrupt`
- `crate::smart_approval` → `crate::control::smart_approval`
- `crate::schema_validate` → `crate::control::schema_validate`

`control/hitl.rs` 内对 `interrupt` / `schema_validate` 改为 `crate::control::interrupt` 与 `crate::control::schema_validate`（或同目录 `super::`）。

- [x] **Step 5: 验证**

```bash
cargo test -p agent
```

Expected: PASS（行为不变，仅路径变）

- [x] **Step 6: Commit**

```bash
git add agent/src
git commit -m "$(cat <<'EOF'
refactor(agent): move prompt and control modules into domain dirs

EOF
)"
```

---

### Task 2: 搬 `exec` 域

**Files:**
- Create: `crates/agent-core/src/exec/mod.rs`
- Move: `cron_exec.rs` → `exec/cron.rs`；`delegate_exec.rs` → `exec/delegate.rs`；`orchestration.rs` → `exec/orchestration.rs`；`multi_agent.rs` → `exec/multi_agent.rs`；`memory_review_spawn.rs` → `exec/memory_review.rs`
- Modify: `crates/agent-core/src/lib.rs`（临时：`pub use exec::memory_review::...` 等保持根符号）
- Modify: 内部 `crate::cron_exec` 等；**本 Task 暂不改** `backend` / Tauri（仍靠根 re-export 或下一步）

**Interfaces:**
- Produces: `pub mod exec` with submodules `cron`, `delegate`, `orchestration`, `multi_agent`, `memory_review`
- 临时根兼容：`pub use exec::memory_review::{...}`；对外 `agent::cron_exec` 在 Task 5 删除前，本 Task 在 `lib.rs` 增加：

```rust
pub mod exec;
/// 过渡：Task 5 删除。外部仍写 `agent::cron_exec`。
pub use exec::cron as cron_exec;
pub use exec::delegate as delegate_exec;
pub use exec::orchestration;
pub use exec::multi_agent;
```

注意：`pub use exec::cron as cron_exec` 让 `agent::cron_exec::CronExecCredentials` 继续可用。`orchestration` / `multi_agent` 名称已与旧模块同名，可直接 `pub use exec::orchestration;` 若与 `pub mod` 冲突则只 `pub mod exec` + `pub use exec::orchestration::{run_orchestration, ...}` 并更新测试——**优先**在本 Task 末尾就改 `agent/tests` 到 `agent::exec::...`，backend/Tauri 留 Task 5。

- [x] **Step 1: git mv**

```bash
cd agent/src
mkdir -p exec
git mv cron_exec.rs exec/cron.rs
git mv delegate_exec.rs exec/delegate.rs
git mv orchestration.rs exec/orchestration.rs
git mv multi_agent.rs exec/multi_agent.rs
git mv memory_review_spawn.rs exec/memory_review.rs
```

- [x] **Step 2: `exec/mod.rs`**

```rust
pub mod cron;
pub mod delegate;
pub mod memory_review;
pub mod multi_agent;
pub mod orchestration;
```

- [x] **Step 3: 更新 `lib.rs` 与内部路径**

- 删除顶层 `pub mod cron_exec` 等。
- 增加 `pub mod exec;`
- 根过渡：

```rust
pub use exec::cron as cron_exec;
pub use exec::memory_review::{
    job_from_agent, maybe_run_background_review, review_notify_from_applied,
    spawn_background_review_after_turn, BackgroundReviewJob, MemoryReviewNotify,
};
```

若 `pub use exec::cron as cron_exec` 无法作为模块路径给 `agent::cron_exec::execute_job` 用，改为：

```rust
pub mod cron_exec {
    pub use crate::exec::cron::*;
}
```

- 内部：`crate::cron_exec` → `crate::exec::cron`；`crate::delegate_exec` → `crate::exec::delegate`；以此类推。
- `exec/multi_agent.rs` / `orchestration` / `delegate` / `cron` / `memory_review` 内对 `loop_` / `messages` / `chat_fallback` 的引用先保持旧名（若尚未搬 runtime/streaming），或按当前已搬路径更新。

- [x] **Step 4: 更新 agent 集成测试路径（exec 相关）**

- `agent/tests/cron_exec_test.rs`: `agent::cron_exec` → `agent::exec::cron`（或仍走过渡 `cron_exec`）
- `agent/tests/orchestration_test.rs`: `agent::orchestration` → `agent::exec::orchestration`
- `agent/tests/multi_agent_test.rs`: `agent::multi_agent` → `agent::exec::multi_agent`

- [x] **Step 5: 验证**

```bash
cargo test -p agent
cargo check -p backend
cargo check -p astro-harness
```

Expected: PASS / 无 error

- [x] **Step 6: Commit**

```bash
git add agent backend apps/desktop/src-tauri 2>/dev/null || git add agent
git commit -m "$(cat <<'EOF'
refactor(agent): move exec domain (cron/delegate/orchestration)

EOF
)"
```

---

### Task 3: 建 `runtime` 并拆 `loop_.rs`

**Files:**
- Create: `crates/agent-core/src/runtime/mod.rs`（主 `AgentLoop` 体，来自原 `loop_.rs` 的 config/loop/TurnResult/MaxDepthError）
- Create: `crates/agent-core/src/runtime/session.rs`（`hydrate_session_messages`、`stored_message_to_runtime`、`resolve_session_project_root`）
- Create: `crates/agent-core/src/runtime/validate.rs`（`validate_message_order`）
- Move: `iteration_budget.rs` → `runtime/budget.rs`；`usage_record.rs` → `runtime/usage.rs`
- Delete: `crates/agent-core/src/loop_.rs`
- Modify: `lib.rs` — `pub mod runtime;`；过渡 `pub use runtime as loop_;` **或** `pub mod loop_ { pub use crate::runtime::*; }` 仅本 Task；Task 5 删除
- Modify: 所有 `crate::loop_` → `crate::runtime`；`crate::iteration_budget` → `crate::runtime::budget`；`crate::usage_record` → `crate::runtime::usage`

**Interfaces:**
- Produces:
  - `runtime::{AgentConfig, AgentLoop, TurnResult, MaxDepthError}`
  - `runtime::validate::validate_message_order`
  - `runtime::budget::{IterationBudget, DEFAULT_MAX_ITERATIONS, DEFAULT_CHILD_MAX_ITERATIONS, should_refund_tool_round}`
  - `runtime::usage::apply_llm_usage_dual_write`（`pub(crate)`）
- **停止** 从 runtime 再导出 `ImageGenTargets`（改为调用方 `tools::ImageGenTargets`）；`AgentLoop` 字段类型仍用 `tools::ImageGenTargets`

- [x] **Step 1: 建目录并移 budget/usage**

```bash
mkdir -p agent/src/runtime
git mv agent/src/iteration_budget.rs agent/src/runtime/budget.rs
git mv agent/src/usage_record.rs agent/src/runtime/usage.rs
```

- [x] **Step 2: 从 `loop_.rs` 抽出 `validate.rs` 与 `session.rs`**

`crates/agent-core/src/runtime/validate.rs` — 整段搬迁原 `validate_message_order`（约 L994–1006），签名不变：

```rust
use common::message::Message;

pub fn validate_message_order(messages: &[Message]) -> bool {
    use common::message::Role;
    for window in messages.windows(2) {
        let (a, b) = (&window[0], &window[1]);
        if a.role == Role::User && b.role == Role::User {
            return false;
        }
        if a.role == Role::Assistant && b.role == Role::Assistant {
            return false;
        }
    }
    true
}
```

`crates/agent-core/src/runtime/session.rs` — 搬迁 `hydrate_session_messages` / `stored_message_to_runtime` / `resolve_session_project_root`（原 L1008 末尾），按当前 `loop_.rs` 实际依赖调整 `use`（`MemoryManager` / `SessionStore` 以 HEAD 为准）。

- [x] **Step 3: `runtime/mod.rs` 承接 `AgentConfig` / `AgentLoop` / `TurnResult` / `MaxDepthError`**

```bash
git mv agent/src/loop_.rs agent/src/runtime/mod.rs
```

然后从 `mod.rs` 删除已抽走的函数；顶部增加：

```rust
pub mod budget;
pub mod session;
pub mod usage;
pub mod validate;

pub use validate::validate_message_order;
```

删除 `pub use tools::{ImageGenCreds, ImageGenTargets};`。`AgentLoop` 内改用 `tools::ImageGenTargets`。`mod.rs` 内调用 hydrate 改为 `session::hydrate_session_messages` 等。默认 `multi_turn` 改为 `budget::DEFAULT_MAX_ITERATIONS`。

- [x] **Step 4: 更新 `lib.rs`**

```rust
pub mod runtime;

pub use runtime::{AgentConfig, AgentLoop, MaxDepthError, TurnResult};
pub use runtime::budget::{
    should_refund_tool_round, IterationBudget, DEFAULT_CHILD_MAX_ITERATIONS,
    DEFAULT_MAX_ITERATIONS,
};

// 过渡：供 backend 的 agent::loop_ 路径，Task 5 删除
pub mod loop_ {
    pub use crate::runtime::*;
}
```

- [x] **Step 5: 全局替换内部引用并更新 agent 测试**

- `crate::loop_` → `crate::runtime`
- 测试：`use agent::loop_` → `use agent::runtime` 或 `use agent::{AgentLoop, ...}`
- `validate_message_order`：`agent::runtime::validate_message_order` 或根 re-export（本 Task 可不根导出，测试走 `agent::runtime::validate_message_order`）

- [x] **Step 6: 验证**

```bash
cargo test -p agent
cargo check -p backend
```

Expected: PASS

- [x] **Step 7: Commit**

```bash
git add agent
git commit -m "$(cat <<'EOF'
refactor(agent): extract runtime module from loop_

EOF
)"
```

---

### Task 4: 拆 `streaming` 为子模块

**Files:**（以当前 `streaming.rs` 行号为切割指南，搬迁时以符号为准）

| 新文件 | 从原 `streaming.rs` 取 |
|--------|------------------------|
| `streaming/hitl_bridge.rs` | L38–107 + `AstroHitlPayload` / `parse_astro_hitl` / `park_*` / `try_park_parent_hitl` / live parent 表（凡 HITL 桥相关） |
| `streaming/types.rs` | `StreamedAssistantContent`、`MultiTurnStreamItem`、stream type aliases、`chunk_to_contents`、`map_provider_stream` |
| `streaming/traits.rs` | `StreamingCompletion` / `StreamingChat` / `StreamingPrompt` |
| `streaming/provider.rs` | `ProviderStreamer` + impls + `chat_target_from_provider_config` + `targets_and_registry_from_primary` |
| `streaming/summary.rs` | `run_max_iterations_summary` |
| `streaming/multi_turn.rs` | `emit`/`finish_*`/`record_llm_usage`、`run_multi_turn_stream*`、`execute_tools_*`、`stream_multi_turn*` |
| `streaming/fallback.rs` | 整文件自原 `chat_fallback.rs` |
| `streaming/mod.rs` | `mod` + `pub use` 对外符号 |

**Interfaces:**
- Produces: 与现公开 API 同名同签的 `stream_multi_turn*` / `run_multi_turn_stream*` / types / traits / `ProviderStreamer`
- `parse_astro_hitl` / `try_park_parent_hitl`：`pub(crate)`，供 `exec::delegate` 使用（`crate::streaming::hitl_bridge::...` 或经 `streaming` re-export）
- `fallback`：`pub(crate)` 为主；测试需要的符号可 `pub`

- [x] **Step 1: 建目录，先搬 fallback**

```bash
mkdir -p agent/src/streaming
git mv agent/src/chat_fallback.rs agent/src/streaming/fallback.rs
# 暂时保留 streaming.rs，下一步再拆
```

更新引用：`crate::chat_fallback` → `crate::streaming::fallback`。在临时 `streaming` 模块方案上：先把现有 `streaming.rs` 改成目录——

```bash
git mv agent/src/streaming.rs agent/src/streaming/multi_turn.rs
```

先写最小 `mod.rs`：

```rust
pub mod fallback;
mod multi_turn;
pub use multi_turn::*;
```

确认 `cargo test -p agent` 通过后再继续切开。

- [x] **Step 2: 抽出 `types.rs` / `traits.rs` / `provider.rs`**

从 `multi_turn.rs` 剪出类型与 trait / ProviderStreamer 到对应文件；`mod.rs`：

```rust
pub mod fallback;
pub mod hitl_bridge;
mod multi_turn;
mod provider;
mod summary;
mod traits;
mod types;

pub use types::{
    AssistantContentStream, MultiTurnStream, MultiTurnStreamItem, StreamedAssistantContent,
};
pub use traits::{StreamingChat, StreamingCompletion, StreamingPrompt};
pub use provider::{
    chat_target_from_provider_config, targets_and_registry_from_primary, ProviderStreamer,
};
pub use multi_turn::{
    run_multi_turn_stream, run_multi_turn_stream_from_provider, stream_multi_turn,
    stream_multi_turn_from_provider, stream_multi_turn_with_hitl,
};
pub(crate) use hitl_bridge::{parse_astro_hitl, try_park_parent_hitl};
```

- [x] **Step 3: 抽出 `hitl_bridge.rs` 与 `summary.rs`**

将 HITL 桥与 `run_max_iterations_summary` 移出 `multi_turn.rs`。`multi_turn` 通过 `super::hitl_bridge` / `super::summary` 调用。目标：`multi_turn.rs` ≤ ~600–800 行；若仍过大，把 `execute_tools_serial` / `execute_tools_concurrent` 再拆到 `streaming/tools_exec.rs`（可选，同 Task 内完成）。

- [x] **Step 4: 更新 `lib.rs` streaming 导出**

```rust
pub mod streaming;
pub use streaming::{
    run_multi_turn_stream, run_multi_turn_stream_from_provider, stream_multi_turn,
    stream_multi_turn_from_provider, stream_multi_turn_with_hitl, MultiTurnStreamItem,
    ProviderStreamer, StreamedAssistantContent, StreamingChat, StreamingCompletion,
    StreamingPrompt,
};
// 不再根导出 chat_fallback / ActiveTargetMeta / chat_target_* 
```

（若本 Task 暂留根导出 `chat_target_*`，必须在 Task 5 删除。）

- [x] **Step 5: 验证**

```bash
cargo test -p agent
cargo check -p backend
```

Expected: PASS；`crates/agent-core/src/streaming/` 下单文件目视 < 800 行（理想 < 600）

- [x] **Step 6: Commit**

```bash
git add agent/src/streaming agent/src/lib.rs
git commit -m "$(cat <<'EOF'
refactor(agent): split streaming monolith into domain modules

EOF
)"
```

---

### Task 5: 收紧根导出（B）并更新所有调用方

**Files:**
- Modify: `crates/agent-core/src/lib.rs`（最终根导出清单，见 spec §公开 API）
- Modify: `crates/agent-server/src/grpc/astro_service.rs`
- Modify: `crates/agent-server/src/cron_runner.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/memory_commands.rs`（文档注释）
- Modify: `agent/tests/*.rs`
- Modify: `docs/hooks.md`（`agent::hooks` → `agent::prompt::hooks`）
- Modify: `delegate/src/lib.rs`、`cron/src/lib.rs`、`crates/agent-tools/src/approval.rs`、`crates/agent-tools/src/builtin/agents/orchestration.rs` 中的路径注释

**Interfaces（最终 `lib.rs` 根导出）：**

```rust
pub mod builder;
pub mod control;
pub mod event_bus;
pub mod exec;
pub mod prompt;
pub mod runtime;
pub mod streaming;
pub mod timeline;

pub use builder::{AgentBuilder, BuiltAgentSpec};
pub use control::hitl::{
    is_exclusive_tool, is_interactive_tool, HitlGate, HitlRegistry, HitlRequest, HitlResolution,
    HITL_DEFAULT_TIMEOUT_SECS,
};
pub use control::interrupt::{Interrupt, InterruptError, InterruptPending, ResumeItem};
pub use runtime::{AgentConfig, AgentLoop, MaxDepthError, TurnResult};
pub use streaming::{
    run_multi_turn_stream, run_multi_turn_stream_from_provider, stream_multi_turn,
    stream_multi_turn_from_provider, stream_multi_turn_with_hitl, MultiTurnStreamItem,
    ProviderStreamer, StreamedAssistantContent, StreamingChat, StreamingCompletion,
    StreamingPrompt,
};
pub use tools::{ToolEntry, ToolRegistry};
```

**必须删除的根导出 / 过渡模块：**

- `pub mod loop_` 过渡
- `pub use exec::cron as cron_exec` / `cron_exec` 模块门面
- `StaticContext` / `DynamicContext` / `context_usage::*` / hooks / budget / `to_provider_messages` / fallback / memory_review / `PauseControl` / `Usage`

- [x] **Step 1: 改 `lib.rs` 为最终清单**（如上）

- [x] **Step 2: 更新 backend**

`astro_service.rs`:

```rust
use agent::builder::AgentBuilder;
use agent::{AgentLoop, HitlGate, HitlRegistry, TurnResult};
use agent::streaming::{/* 仅仍需要的项；若已根导出可 use agent::... */};
// ImageGenTargets:
use tools::ImageGenTargets;
// review:
agent::exec::memory_review::spawn_background_review_after_turn(...);
```

`cron_runner.rs`:

```rust
use agent::exec::cron::{self, CronExecCredentials};
```

- [x] **Step 3: 更新 Tauri**

`commands.rs` 中所有 `agent::cron_exec` → `agent::exec::cron`。

`memory_commands.rs` 注释：`agent::loop_::AgentLoop` → `agent::AgentLoop` / `agent::runtime::AgentLoop`。

- [x] **Step 4: 更新 `agent/tests`**

| 文件 | 改法 |
|------|------|
| `agent_test.rs` | `agent::loop_` → `agent::runtime` 或根类型；`prompt_builder` → `agent::prompt::prompt_builder` |
| `rig_agent_test.rs` | `context` → `agent::prompt::context`；`loop_` → `runtime` |
| `streaming_test.rs` | `loop_` → 根/`runtime`；streaming 用根或 `agent::streaming` |
| `cron_exec_test.rs` | `agent::exec::cron` |
| `orchestration_test.rs` | `agent::exec::orchestration` |
| `multi_agent_test.rs` | `agent::exec::multi_agent` |
| `event_bus_test.rs` | 仍 `agent::event_bus` |
| `memory_snapshot_test.rs` | `AgentLoop` 走根或 `runtime` |

- [x] **Step 5: grep 清理旧路径**

```bash
rg -n 'agent::(loop_|cron_exec|delegate_exec|chat_fallback|memory_review_spawn|iteration_budget)\b' \
  --glob '!docs/superpowers/**'
rg -n 'crate::(loop_|cron_exec|delegate_exec|chat_fallback|iteration_budget|usage_record)\b' agent/
```

Expected: 无业务代码命中（spec/plan 历史文档除外）

- [x] **Step 6: 验证**

```bash
cargo test -p agent
cargo check -p backend
cargo check -p astro-harness
```

Expected: 全部成功

- [x] **Step 7: Commit**

```bash
git add agent backend apps/desktop/src-tauri docs/hooks.md delegate cron tools
git commit -m "$(cat <<'EOF'
refactor(agent): tighten crate root exports and update callers

EOF
)"
```

---

### Task 6: 文档与最终验收

**Files:**
- Modify: 任何仍写旧路径的模块头 / `docs/hooks.md` / crate 注释
- Verify: 目录树与单文件行数

- [x] **Step 1: 核对目录**

```bash
find agent/src -type f -name '*.rs' | sort
wc -l agent/src/streaming/*.rs agent/src/runtime/*.rs | sort -n
```

Expected: 顶层仅 `lib.rs`、`builder.rs`、`event_bus.rs`、`timeline.rs` + 域目录；无 `loop_.rs` / 平铺 `streaming.rs`；streaming/runtime 单文件尽量 ≤ 600（`multi_turn` 若略超，在 Step 2 再拆 `tools_exec.rs`）。

- [x] **Step 2（可选）: 若 `multi_turn.rs` 或 `delegate.rs` > 700 行**

拆 `streaming/tools_exec.rs`（serial/concurrent tool 执行）或 `exec/delegate/` 子文件；再跑 `cargo test -p agent`。

- [x] **Step 3: 全量验证**

```bash
cargo test -p agent
cargo check -p backend
cargo check -p astro-harness
```

- [x] **Step 4: Commit（若有文档/再拆改动）**

```bash
git add agent docs
git commit -m "$(cat <<'EOF'
docs(agent): align path references after crate reorg

EOF
)"
```

---

## Spec coverage（自检）

| Spec 要求 | Task |
|-----------|------|
| 域目录 runtime/streaming/control/prompt/exec | 1–4 |
| 拆 streaming / loop_ | 3–4 |
| 根导出 B 清单 | 5 |
| 删除旧路径兼容 | 5 |
| ImageGenTargets 走 tools | 3 + 5 |
| backend / tests / cron 调用方 | 5 |
| 单文件 400–600 目标 | 4 + 6 |
| 不改语义 / 不拆多 crate | Global Constraints |
| `cargo test -p agent` + check backend | 每 Task |

## Placeholder scan

无 TBD；行号为切割指南，以符号名为准。
