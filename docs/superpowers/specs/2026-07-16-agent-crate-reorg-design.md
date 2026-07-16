# agent crate 重组设计

日期：2026-07-16  
状态：已定稿（待实现计划）

## 背景

`agent` crate 的 `src/` 为平铺结构，约 23 个模块 / ~7900 行。最大痛点是 `streaming.rs`（~2050 行）与 `loop_.rs`（~1060 行），中等体积还有 `delegate_exec` / `orchestration` / `hitl` / `cron_exec`。同仓库的 `tools`、`providers`、`home` 已按域分子目录。

## 目标

1. **按运行时阶段分子目录**，降低顶层平铺噪音。
2. **拆开巨石文件**，单文件目标约 400–600 行。
3. **根导出按 B 收紧**：crate 根只保留产品级入口；细节走深路径或 `pub(crate)`。允许破坏性改动，不保留旧路径兼容别名。

## 非目标

- 不拆成多个 crate。
- 不改流式 / 主循环 / HITL / cron / delegate 的运行时语义。
- 不重构 `delegate` 算法（仅在超标时做物理切开）。
- 不改前端 / proto。

## 架构：目录与文件归属

```text
agent/src/
  lib.rs
  builder.rs                    # ~240 行，保持单文件

  runtime/
    mod.rs                      # AgentLoop / AgentConfig / TurnResult / MaxDepthError
    session.rs                  # hydrate / message 转换 / project_root
    validate.rs                 # validate_message_order
    budget.rs                   # ← iteration_budget.rs
    usage.rs                    # ← usage_record.rs（继续非对外）

  streaming/
    mod.rs                      # 对外入口 re-export
    types.rs                    # StreamedAssistantContent, MultiTurnStreamItem, ...
    traits.rs                   # StreamingCompletion / Chat / Prompt
    provider.rs                 # ProviderStreamer + 组装 helpers
    multi_turn.rs               # run_multi_turn_stream* / stream_multi_turn*
    hitl_bridge.rs              # 父会话 HITL 桥 / child park
    summary.rs                  # max_iterations 总结
    fallback.rs                 # ← chat_fallback.rs

  control/
    mod.rs
    hitl.rs
    interrupt.rs
    smart_approval.rs
    schema_validate.rs          # 对 control 内部公开即可

  prompt/
    mod.rs
    context.rs
    context_usage.rs
    prompt_builder.rs
    messages.rs
    hooks.rs

  exec/
    mod.rs
    cron.rs                     # ← cron_exec.rs
    delegate.rs                 # ← delegate_exec.rs（若仍 >600 行再内拆）
    orchestration.rs
    multi_agent.rs
    memory_review.rs            # ← memory_review_spawn.rs

  event_bus.rs                  # 薄，留顶层
  timeline.rs                   # 薄，留顶层
```

约定：

- 删除顶层 `loop_` / `streaming` / `cron_exec` 等旧模块路径；**不**对外保留兼容别名。
- `ImageGenTargets` 继续来自 `tools`；backend 改为 `tools::ImageGenTargets`（agent 根不 re-export）。
- `delegate.rs` 超 600 行可第二刀再拆，不阻塞第一轮。

## 公开 API（B）

### 根上保留（`use agent::...`）

| 符号 | 来源 |
|---|---|
| `AgentBuilder`, `BuiltAgentSpec` | `builder` |
| `AgentConfig`, `AgentLoop`, `TurnResult`, `MaxDepthError` | `runtime` |
| `stream_multi_turn`, `stream_multi_turn_with_hitl`, `stream_multi_turn_from_provider`, `run_multi_turn_stream`, `run_multi_turn_stream_from_provider` | `streaming` |
| `MultiTurnStreamItem`, `StreamedAssistantContent`, `ProviderStreamer`, `StreamingChat`, `StreamingCompletion`, `StreamingPrompt` | `streaming` |
| `HitlGate`, `HitlRegistry`, `HitlRequest`, `HitlResolution`, `HITL_DEFAULT_TIMEOUT_SECS` | `control` |
| `is_exclusive_tool`, `is_interactive_tool` | `control` |
| `Interrupt`, `InterruptError`, `InterruptPending`, `ResumeItem` | `control` |
| `ToolEntry`, `ToolRegistry` | `tools` re-export |

### 根上拿掉

| 现状 | 改后 |
|---|---|
| `StaticContext` / `DynamicContext` | `agent::prompt::{...}` |
| `context_usage::*` | `agent::prompt::context_usage`（模块公开，不进根） |
| `CancelSignal`, `PromptCancelled` | `agent::prompt::hooks` |
| `IterationBudget`, `DEFAULT_*`, `should_refund_*` | `agent::runtime::budget` |
| `to_provider_messages` | `agent::prompt::messages` |
| `chat_fallback::*` / `ActiveTargetMeta` | `agent::streaming::fallback`（多 `pub(crate)`） |
| `memory_review_spawn::*` | `agent::exec::memory_review` |
| `providers::{PauseControl, Usage}` | 调用方直接 `use providers::...` |
| `ImageGenTargets` via `loop_` | `tools::ImageGenTargets` |

### 深路径公开（模块 `pub`，不进根）

- `agent::exec::{cron, delegate, orchestration, multi_agent, memory_review}`
- `agent::prompt::*`
- `agent::event_bus`、`agent::timeline`
- `chat_target_from_provider_config`、`targets_and_registry_from_primary` 留在 `streaming` 子模块，**不**根导出

### 已知调用方迁移

- `backend/src/grpc/astro_service.rs`：`loop_` → 根或 `runtime`；`spawn_background_review_*` → `exec::memory_review`；`ImageGenTargets` → `tools`
- `backend/src/cron_runner.rs`：`agent::cron_exec` → `agent::exec::cron`
- `backend/src/grpc/interrupt_store.rs`：根上 Interrupt/HITL 可保持
- `agent/tests/*`：只改 `use` 路径

## 落地顺序

1. 建域目录 + 整文件搬家；更新 `lib.rs` 的 `mod`（过渡别名仅限 crate 内、不提交给外部）。
2. 拆 `streaming.rs` 与 `loop_.rs` 到上表子文件。
3. 收紧根导出（B）并更新 `backend` / `agent/tests`。
4. 清理过渡别名、多余 `pub`、文档路径；grep 确认无 `loop_` / `cron_exec` 旧路径残留。

## 风险与缓解

| 风险 | 缓解 |
|---|---|
| `pub use` 漏改导致大面积编译失败 | 每步 `cargo test -p agent` + `cargo check -p backend` |
| `streaming` 子模块循环依赖 | 类型/trait 下沉；`multi_turn` 依赖它们，避免反向 |
| 注释/文档路径过时 | 更新模块头与 `lib.rs`；grep 旧名 |

## 验证

- `cargo test -p agent`
- `cargo check -p backend`
- 人工确认：`lib.rs` 根导出符合本文清单；无旧模块名对外暴露

## 测试策略

- 文件内 `#[cfg(test)]` 随模块搬家。
- 集成测试只改 import，不断言逻辑变更。
