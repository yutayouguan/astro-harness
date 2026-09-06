# hooks

Astro 的 typed lifecycle hook runtime。它聚合四类执行面：进程内 Plugin、配置驱动的 Command/MCP handler、Gateway manifest 和 legacy Shell observer。

## 执行面

| 执行面 | 作用 | 是否影响控制流 |
| --- | --- | --- |
| `PluginHookBus` | 进程内同步扩展 | 是，按事件专属 outcome 聚合 |
| `CommandHookRunner` | `hooks.json` / `config.toml` 的 command 或 `mcp_tool` | 是，仅限支持的 canonical 事件和结果字段 |
| `GatewayHookRegistry` | `HOOK.yaml` 文件发现 | 当前为观察/通知 |
| `ShellHookRunner` | `config.yaml` 旧式命令映射 | 异步观察，不参与决策 |

`HookRuntime` 是聚合入口。`with_mcp_executor()` 在 Session 边界绑定 MCP 调用能力，并创建 session-owned 异步任务域；`shutdown()` 取消并排空该 session 的异步 command hooks。

## Canonical command 事件

Command/MCP handler 只接受以下精确名称：

`PreToolUse`、`PermissionRequest`、`PostToolUse`、`PreCompact`、`PostCompact`、`SessionStart`、`SessionEnd`、`UserPromptSubmit`、`SubagentStart`、`SubagentStop`、`Stop`、`Interrupt`。

大小写或旧 snake_case 名称不会自动归一化。Plugin bus 另有 LLM/API/transform/gateway 等 Astro 内部事件，完整定义见 `src/event.rs`。

## 关键模块

| 路径 | 职责 |
| --- | --- |
| `src/lib.rs` | `HookRuntime` 聚合、分发和 shutdown |
| `src/event.rs` / `names.rs` | canonical 事件 |
| `src/lifecycle_events.rs` | session、prompt、compact、stop、interrupt typed contracts |
| `src/tool_events.rs` | pre/permission/post tool typed contracts |
| `src/command.rs` | command/MCP 配置、信任、执行、输出校验、异步所有权 |
| `src/mcp.rs` | session-bound `HookMcpExecutor` |
| `src/run.rs` | `HookRunRecord`、生命周期 observer 和 recent runs |
| `src/plugin.rs` | 进程内 plugin bus |
| `src/gateway.rs` | manifest discovery |
| `src/shell.rs` | legacy shell observer |
| `src/ui.rs` | UI timeline slot |

## 不变量

1. 生命周期入口使用 typed request/outcome，不跨事件复用一套宽松 JSON 语义。
2. command 配置 `deny_unknown_fields`；输出只接受对应事件允许的字段，非法输出记录错误并 fail-open。
3. `SessionEnd` 和 `Interrupt` 超时限制为 1–3 秒；`SessionEnd` 强制同步。
4. command 输出上限 1 MiB，环境值上限 8 KiB；敏感环境变量不会继承。
5. 每个 session 最多并发 8 个异步 hook；runtime shutdown 时取消并排空。
6. project hooks 只在项目已信任时加载；内容 hash 改变会使已批准状态失效。
7. `HookStarted` / `HookCompleted` 用于 live UI，不写入 durable rollout。
8. 当前没有 executor-scoped plugin/request metadata；作用域由 process plugin bus、session runtime、turn/step request 明确承载。

## 文档与验证

- [运行契约](../../docs/hooks.md)
- [详细设计](../../docs/04-详细设计阶段/01-核心引擎层/08-Hooks系统详细设计.md)

```bash
cargo test -p hooks
```
