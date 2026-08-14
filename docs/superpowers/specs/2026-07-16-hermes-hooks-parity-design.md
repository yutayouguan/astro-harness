# Hermes 钩子补齐设计（Astro 对齐）

日期：2026-07-16  
状态：已定稿（待实现计划）  
参考：[Hermes Hooks](https://hermes-agent.nousresearch.com/docs/user-guide/features/hooks)、`docs/hooks.md`

## 背景

Astro 已实现与 Hermes 同名的核心 Plugin Hooks（`pre_llm_call`、`pre/post_tool_call`、`post_llm_call`、会话生命周期、`subagent_stop`、`pre_gateway_dispatch` 等），并额外有 `pre/post_api_request`。

相对 Hermes 公开 Plugin Hooks 表，仍缺：

- `pre_verify`
- `subagent_start`
- `pre_approval_request` / `post_approval_response`
- `transform_tool_result` / `transform_terminal_output` / `transform_llm_output`

## 目标

1. 补齐上述 7 个钩子名与触发点，行为对齐 Hermes 语义，并结合 Astro 现有 `PluginHookBus` / UI 时间线 / Shell 旁路。
2. `pre_verify` 按「写盘后才触发、可 KeepGoing 再跑一轮」落地（Hermes coding verify 语义的 Astro 适配）。
3. transform 类可通过返回值改写文本；审批对为观察型。

## 非目标

- 不加载 Hermes Python 插件 / 动态库。
- 不改 Gateway Event Hooks / Shell Hooks 体系本身（同名 shell 旁路继续可用）。
- 不重做前端 hook 卡片（继续 `kind: "hook"`，标题=钩子名）。
- 不实现 Hermes 以外的新钩子产品能力。

## 方案

扩展现有 `HookOutcome` + 在现有执行路径插入 fire 点（方案 1）。

### 新增名字（`crates/agent-hooks/src/names.rs`）

```text
pre_verify
subagent_start
pre_approval_request
post_approval_response
transform_tool_result
transform_terminal_output
transform_llm_output
```

### 扩展 `HookOutcome`

| 变体 | 用于 | 语义 |
|------|------|------|
| 现有 `Continue` / `Block` / `Modify` / `InjectContext` / `Allow` / `Skip` / `Rewrite` | 不变 | |
| **新增** `ReplaceText(String)` | 三个 `transform_*` | 用新字符串替换对应文本 |
| **新增** `KeepGoing(String)` | `pre_verify` | 不结束本 turn；注入 message 后再进 API/工具循环 |

`is_mutating_hook` 包含：`PRE_LLM_CALL`、`PRE_TOOL_CALL`、`PRE_GATEWAY_DISPATCH`、`PRE_VERIFY`、三个 `TRANSFORM_*`。

多回调短路：与现网一致，**首个非 Continue/Allow 生效**。

### 触发约定

| 钩子 | 时机 | Payload 要点 | 生效返回 |
|------|------|--------------|----------|
| `subagent_start` | 子 Agent 构造完、真正 `run` 前（每个 child 一次） | 父 `session_id`；`detail` 含角色/任务摘要 | 观察 |
| `pre_approval_request` | `terminal` 危险命令将走 `Ask`（含进 smart 前） | `message`=command；`detail`=surface | 观察 |
| `post_approval_response` | 审批结束（用户/超时/smart 结果） | `detail` 含 choice | 观察 |
| `transform_terminal_output` | `terminal` 原始 stdout/stderr 后、截断/脱敏前 | `tool_name`、`tool_result`=raw、`message`=command | `ReplaceText` |
| `transform_tool_result` | 任意工具返回后、写入会话 / `POST_TOOL_CALL` 前 | `tool_name` / args / result | `ReplaceText` |
| `transform_llm_output` | 最终 assistant 文本定稿前（`POST_LLM_CALL` 前） | `message`=最终文本 | `ReplaceText` |
| `pre_verify` | 无工具最终回复，且本轮执行过写盘工具，即将结束前 | `detail` 含 `attempt`；`message`=最终草稿 | `KeepGoing(msg)` |

### `pre_verify`（方案 A）

- **写盘白名单（初版）：** `terminal`；`file_ops` 的变更类 action（write/patch 等）。常量列表，后续可扩。
- **控制流：**

```text
无工具最终文本
  → （本轮写盘 && attempt < MAX）fire pre_verify
       → KeepGoing(msg): 注入用户侧提示，attempt++，再进 API 循环
       → 其它: 继续
  → transform_llm_output → post_llm_call → on_session_end
```

- `MAX_VERIFY_ATTEMPTS = 2`（含首次结束尝试）。超限不再 fire，直接收尾。

### 完整顺序

```text
on_session_start → pre_llm_call
  → [循环] pre_api_request → API → post_api_request
           → pre_tool_call → (pre_approval… / 工具 / post_approval…)
                → transform_terminal_output? → transform_tool_result → post_tool_call
           → subagent_start … subagent_stop（委派时）
  → pre_verify? → transform_llm_output → post_llm_call → on_session_end
```

## 接线文件

| 区域 | 文件 | 职责 |
|------|------|------|
| hooks | `names.rs` / `outcome.rs` / `ui.rs` | 常量、Outcome、UI 推送名单 |
| docs | `docs/hooks.md` | 钩子表与顺序 |
| agent | `runtime/mod.rs` | `transform_tool_result`；写盘标记；必要时工具路径 |
| agent | `exec/delegate.rs`（及 async） | `subagent_start` |
| agent | `streaming/tools_exec.rs` | approval 对；terminal transform 协调 |
| tools | `terminal.rs` 或 agent 包装 | `transform_terminal_output`（优先 agent 包装，避免强绑依赖） |
| agent | `streaming/multi_turn.rs` | `pre_verify`、`transform_llm_output`、attempt |

## 错误处理

- 插件 panic：捕获，`tracing::warn`，视为 `Continue`。
- 非对应钩子返回 `KeepGoing` / `ReplaceText`：调用方忽略，当 Continue。
- Shell 同名旁路：仍异步，不影响主流程返回值。

## 测试

| 用例 | 包 |
|------|-----|
| Outcome 短路 / 新变体 | `hooks` |
| `transform_tool_result` 改写后再 `post_tool_call` | `agent` |
| `transform_terminal_output` 截断前生效 | `agent`/`tools` |
| `transform_llm_output` 后再 `post_llm_call` | `agent` streaming |
| `pre_verify` KeepGoing 再进一轮；超限停止 | `agent` streaming |
| `subagent_start` 在 `subagent_stop` 前 | `agent` delegate |
| approval pre 在 park 前、post 在决议后 | `agent` tools_exec |

验证：`cargo test -p hooks`；`cargo test -p agent`。

## 落地顺序

1. hooks 基础（名字 + Outcome + UI + 单测）
2. `transform_tool_result`
3. `transform_terminal_output`
4. `transform_llm_output`
5. `pre_approval_request` / `post_approval_response`
6. `subagent_start`
7. `pre_verify`（控制流）
8. 更新 `docs/hooks.md` + 全量回归

## 风险

| 风险 | 缓解 |
|------|------|
| `pre_verify` 死循环 | attempt 上限；仅无工具终态触发 |
| tools↔hooks 依赖 | terminal transform 优先 agent 包装 |
| 时间线变吵 | 仍受「Hook 事件」开关控制 |
