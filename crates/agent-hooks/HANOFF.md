# Codex Hook Alignment Handoff

> 状态：已落地基线
> 更新：2026-09-01

本文只记录维护边界。对外配置和输出契约见 [`../../docs/hooks.md`](../../docs/hooks.md)。

## 已完成

- 12 个 command hook 事件使用 canonical 精确名称；
- tool、session、prompt、compact、stop、interrupt 和 subagent 生命周期使用 typed request/outcome；
- command handler 接收事件专属 stdin JSON，并校验事件专属输出字段；
- `mcp_tool` handler 通过 session-bound `HookMcpExecutor` 实际执行；
- user/project 配置分层、project trust、handler hash 和 enable state 已接入；
- sync/async 生命周期、每 session 并发上限、shutdown cancel/drain 已实现；
- `SessionEnd` 单次发送且强制同步，`Interrupt` 使用短超时；
- Unix process group 与 Windows Job Object 收敛子进程生命周期；
- Hook run 通过 `HookStarted` / `HookCompleted` 进入 live timeline。

## 仍然明确不做

- `prompt` / `agent` command handler 类型尚未执行；只有 `command` 与 `mcp_tool` 可运行。
- Gateway 和 legacy Shell 仍是观察 transport，不参与 typed decision aggregation。
- Hook run 事件不进入 durable rollout。
- 不引入 executor-scoped plugin/request metadata；如未来确有跨 executor 隔离需求，需另行设计 ownership、恢复和清理协议。

## 维护检查

修改 Hook 行为时至少检查：

1. `HookEvent::COMMAND_HOOK_EVENTS` 的名称和顺序；
2. `HookInput::command_input_for_event()` 输入字段；
3. `event_output_has_codex_shape()` 与事件专属 outcome 聚合；
4. async runtime 是否仍由 session 所有并可 shutdown；
5. command 与 MCP handler 是否生成一致的 `HookRunRecord`；
6. live Hook 事件是否保持 transient；
7. `SessionEnd` 是否只发送一次。
