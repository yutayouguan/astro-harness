# Astro Hook 运行契约

> 状态：当前实现基线
> 更新：2026-09-01
> 代码事实源：`crates/agent-hooks/src/`

## 1. Runtime 组成

`HookRuntime` 聚合四类执行面：

| 执行面 | 生命周期 | 结果语义 |
| --- | --- | --- |
| Plugin | 进程级 bus，同步 callback | 可返回 typed `HookOutcome` 并影响流程 |
| Command/MCP | 配置来自 user/project，绑定到 session runtime | 可按事件返回 block、permission、modify、context 或 keep-going |
| Gateway | 扫描 `hooks/{name}/HOOK.yaml` | 当前为观察/通知 |
| legacy Shell | `~/.astro/config.toml` 的命令映射 | 异步观察，输出不参与决策 |

`dispatch()` 会为同一事件生成事件专属 payload，依次触达这些执行面，再聚合 Plugin 与 Command/MCP 的有效决策。Gateway/Shell 的存在不改变控制流。

## 2. Canonical 事件

Command/MCP 配置只支持以下 12 个精确名称：

| 事件 | 触发点 | 可产生的核心效果 |
| --- | --- | --- |
| `SessionStart` | session runtime 建立 | additional context |
| `UserPromptSubmit` | 用户输入准入 | block / additional context |
| `PreToolUse` | 工具审批和执行前 | block / updated input / permission 建议 |
| `PermissionRequest` | 需要人工授权时 | allow / deny |
| `PostToolUse` | 工具返回后 | block / additional context |
| `PreCompact` | 压缩前 | cancel/continue 控制 |
| `PostCompact` | 压缩后 | 观察或 stop |
| `SubagentStart` | 子 Agent 启动 | additional context |
| `SubagentStop` | 子 Agent 结束 | stop / feedback |
| `Stop` | 主 Agent 准备结束 | stop 或要求继续 sampling |
| `Interrupt` | 活跃任务中断 | system message |
| `SessionEnd` | session-owned runtime 关闭 | 观察；强制同步 |

名称大小写敏感，不接受 snake_case 或历史别名。

Plugin bus 还定义 `PreLlmCall`、`PreApiRequest`、`PostApiRequest`、`TransformTerminalOutput`、`TransformToolResult`、`TransformFinalLlmOutput`、`PostLlmCall`、`PostApprovalResponse`、`PreGatewayDispatch`、`SessionReset`、`GatewayStartup`、`AgentEnd` 和 `CommandNewChat`。这些是 Astro 进程内扩展事件，不等同于 command hook 配置面。

## 3. 输入契约

所有 command/MCP hook 输入都从 `HookInput` 生成，但 stdin JSON 按事件裁剪，不把内部字段全部暴露。公共字段包括：

- `session_id`、`transcript_path`、`cwd`、`hook_event_name`；
- `model`、`turn_id`、`permission_mode`；
- 与事件相关的 `source`、`reason`、`prompt`、tool、subagent 或 stop 字段。

工具事件使用 `tool_name`、`tool_use_id`、`tool_input`、`tool_response`；subagent 事件使用 `agent_id`、`agent_type`、`agent_transcript_path`；Stop 使用 `stop_hook_active` 和 `last_assistant_message`。未属于该事件的字段不会进入 command stdin。

## 4. 输出契约

Command stdout 为空表示继续。JSON 输出只允许事件支持的字段：

- 通用控制：`continue`、`stopReason`、`suppressOutput`、`systemMessage`；
- decision：`decision`、`reason`；
- `PreToolUse`：`updatedInput`、permission decision/reason、additional context；
- `PermissionRequest`：allow/deny decision；
- `PostToolUse` / `UserPromptSubmit` / start events：additional context；
- `Stop` / `SubagentStop`：阻止结束或反馈继续。

输出必须符合对应事件的形状。未知字段、字段组合不合法、输出不是允许的 JSON、unsupported `suppressOutput` 或不适用的 MCP output 修改都会把本 handler 标为错误，并清空其控制决策。默认 fail-open：一个坏 hook 不应意外阻断 Agent。

`Stop` 请求继续时受 continuation guard 限制，避免 hook 永久阻止终止。

## 5. 配置与信任

Command/MCP hooks 可来自：

- user：`~/.astro/hooks.json`、`~/.astro/config.toml`；
- project：从 project root 到当前 cwd 的各级 `.astro/hooks.json`、`.astro/config.toml`。

Project 配置只在项目已信任时加载。每个 handler 根据事件、matcher、配置和来源生成稳定 key/hash；配置内容变化后，旧 trusted hash 不再有效。`config.toml` 的 `hooks.state` 保存 enabled 与 trusted hash。

示例 `hooks.json`：

```json
{
  "description": "project checks",
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "exec_command|apply_patch",
        "hooks": [
          {
            "type": "command",
            "command": "./scripts/check-tool.sh",
            "timeout": 10,
            "statusMessage": "checking tool request"
          }
        ]
      }
    ],
    "PostToolUse": [
      {
        "hooks": [
          {
            "type": "mcp_tool",
            "server": "audit",
            "tool": "record",
            "input": { "event": "${hook_event_name}" }
          }
        ]
      }
    ]
  }
}
```

`prompt` 和 `agent` handler 可被解析，但当前不执行；使用它们不会获得隐式 Agent 能力。

## 6. Command 执行安全

- 默认 timeout 600 秒；`SessionEnd`/`Interrupt` 限制为 1–3 秒；
- `SessionEnd` 忽略 async 配置并同步执行；
- stdout 与 stderr 各自限制 1 MiB；MCP 文本结果限制 1 MiB；
- 单个注入环境值限制 8 KiB；
- 敏感和不可继承环境变量会被剥离；
- runtime 绑定时捕获环境快照，运行中不读取漂移的全局环境；
- Unix 使用独立 process group，Windows 使用 Job Object，timeout/cancel 会收敛子进程树；
- timeout 覆盖 stdin 写入、进程执行和 pipe drain 的完整阶段。

## 7. MCP handler

`mcp_tool` 通过 `HookMcpExecutor` 执行，不是假配置。Session 在 `with_mcp_executor()` 边界注入实际 MCP transport；没有 executor 时产生结构化 unavailable 错误。

MCP input 在加载时验证为 TOML 可表达结构，运行时再展开模板。Command 与 MCP handler 共用 matcher、trust、timeout、run record 和事件专属输出校验。

## 8. 异步生命周期与可观测性

标记 `async: true` 的 command hook 不阻塞主流程，其输出也不能在事后改变已完成的控制决策。每个 session 最多并发 8 个异步 hook；session shutdown 调用 `HookRuntime::shutdown()` 取消并排空剩余任务。

每次 command/MCP 执行生成 `HookRunRecord`。Core 把开始/完成投影为 `EventMsg::HookStarted` 和 `HookCompleted`，Server 转给 UI 时间线。它们是 transient 事件，不写入 durable rollout；最近运行记录由内存中的 `HookRunStore` 提供。

## 9. 生命周期语义

- `SessionStart` 在 session runtime 获得真实上下文后发送；
- `SessionEnd` 由 session-owned shutdown 只发送一次；
- `Interrupt` 与 task abort 共享取消边界，但 hook 有独立短超时；
- `PreCompact` / `PostCompact` 同时覆盖显式 compact 和运行中压缩；
- Stop target 区分主任务、subagent 和 memory consolidation；memory consolidation 只允许 policy-owned source，不执行 user/project/plugin hook；
- 无 executor-scoped plugin/request metadata。Process、session、turn、step 的 ownership 已显式存在，未来如扩展 executor scope 必须先定义恢复与清理语义。

## 10. 验证

```bash
cargo test -p hooks
cargo test -p agent hook
```

重点回归：canonical names、event-specific input/output、MCP execution、project trust/hash、async concurrency/shutdown、SessionEnd exactly-once、Stop continuation guard、Unix/Windows child-process cleanup。
