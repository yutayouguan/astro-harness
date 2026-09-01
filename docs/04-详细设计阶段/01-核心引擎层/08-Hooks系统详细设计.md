# Hooks 系统详细设计

> 版本：v2.0
> 日期：2026-09-01
> 状态：当前实现基线

## 1. 目标与边界

Hooks 为 Agent 生命周期提供可观测、可拦截和可变换的扩展点。系统必须保证：

- 事件名、输入和输出都是 typed contract；
- 不同生命周期事件不能共享含义模糊的返回值；
- 外部 handler 失败默认 fail-open；
- 异步任务由创建它的 session 拥有并负责清理；
- Hook 不能绕过工具审批、sandbox、MCP 权限或 Responses history 不变量。

历史文档中的 `HookRegistry`、WASM 内置安全 Hook、统一任意 JSON 结果和未执行的 `mcp_tool` 描述已被本设计替代。

## 2. 组件

```text
typed lifecycle caller
        |
        v
    HookRuntime
   /    |      \
Plugin Command  observational
 bus   / MCP    Gateway + Shell
   \    |
    event-specific aggregation
        |
        v
typed outcome + HookRun live events
```

| 组件 | 生命周期 | 所有权 |
| --- | --- | --- |
| `PluginHookBus` | 进程 | `HookRuntime` 共享 |
| `CommandHookRunner` | 配置可共享，异步 runtime 绑定到 session | session shutdown 清理 |
| `HookMcpExecutor` | session | 由 `Session` 注入 |
| `GatewayHookRegistry` | 进程/工作区 | 观察 transport |
| `ShellHookRunner` | 进程 | legacy 异步观察 |
| `HookRunStore` | runtime 内存 | 最近 200 次运行及 observer |

不新增 executor-scoped plugin/request metadata。执行上下文由 `HookInput`、typed request、Session/Turn/Step ownership 传递；若未来需要隔离多个 executor，必须另立数据模型而不是把任意 metadata 塞入 Hook runtime。

## 3. 事件模型

### 3.1 Command/MCP 事件

`HookEvent::COMMAND_HOOK_EVENTS` 固定为 12 项，名称和顺序属于公开契约：

```text
PreToolUse, PermissionRequest, PostToolUse,
PreCompact, PostCompact,
SessionStart, SessionEnd, UserPromptSubmit,
SubagentStart, SubagentStop, Stop, Interrupt
```

只接受精确 canonical 名称。旧别名和 snake_case 不做兼容归一化。

### 3.2 Plugin 扩展事件

Plugin 还可订阅 LLM/API telemetry、terminal/tool/final transform、approval response、gateway dispatch、session reset/startup/end 和 command-new-chat 等内部事件。它们不自动开放给 command 配置。

## 4. Typed lifecycle contracts

Core 不直接构造松散 JSON 决策，而是调用事件专属 API：

| 生命周期 | Request / Outcome |
| --- | --- |
| session | `SessionStartRequest/Outcome`、`SessionEndRequest/Outcome` |
| prompt | `UserPromptSubmitRequest/Outcome` |
| tool | `PreToolUseRequest/Outcome`、`PermissionRequestRequest/Outcome`、`PostToolUseRequest/Outcome` |
| compact | `PreCompactRequest/Outcome`、`PostCompactRequest` |
| stop | `StopRequest/Outcome` + `StopHookTarget` |
| interrupt | `InterruptRequest/Outcome` |

typed API 负责把 runtime 数据转为 `HookPayload`，再按事件裁剪 command stdin。聚合后只把该事件允许的字段转回 typed outcome。

### 4.1 工具顺序

```text
schema/visibility check
 -> PreToolUse
 -> approval or PermissionRequest
 -> sandbox/network decision
 -> handler or MCP
 -> TransformToolResult
 -> PostToolUse
 -> canonical ResponseItem output
```

Hook 可以更新工具输入或附加上下文，但不能伪造 call id、跳过工具结果持久化或改变原生 call/output 类型。

### 4.2 Stop

Stop hook 可请求继续 sampling，并提供反馈上下文。Runtime 记录 stop-hook active 状态并实施 continuation guard，避免无限循环。主 Agent、subagent 和 memory consolidation 使用不同 target；memory consolidation 只接受 policy-owned hook source。

## 5. 配置、发现与信任

加载顺序：

1. user `~/.astro/hooks.json`；
2. user `~/.astro/config.toml` 的 `[hooks.*]`；
3. project root 到 cwd 各级 `.astro/hooks.json` / `.astro/config.toml`；
4. legacy `~/.astro/config.yaml` shell map；
5. Gateway manifests。

Project command hooks 只有在 `agent-config` 判定项目为 trusted 时启用。handler identity 由来源、事件、组序号、handler 序号、matcher 和规范化配置计算；内容变化会得到新 hash，已批准状态随之失效。

`HooksFile` 和 handler 配置拒绝未知字段。`prompt` / `agent` handler 类型为解析兼容保留，当前不执行。

## 6. Command 与 MCP 执行

### 6.1 Command

- stdin：事件专属 JSON；
- cwd：payload 的 canonical cwd；
- environment：session 绑定时快照 + 保留 hook 变量，敏感值被清理；
- timeout：普通事件默认 600 秒，`SessionEnd`/`Interrupt` 限制 1–3 秒；
- output：stdout 与 stderr 各自最大 1 MiB，MCP 文本结果最大 1 MiB；
- process tree：Unix process group，Windows Job Object；
- cancellation：覆盖 stdin、子进程和 pipe drain。

`SessionEnd` 始终同步，确保 runtime 销毁前完成或超时。

### 6.2 MCP

`mcp_tool` handler 配置 `server`、`tool` 和结构化 `input`。输入在加载时验证为 TOML 可表达值，在调用时展开模板，经 session-bound `HookMcpExecutor` 进入真实 MCP Hub。没有绑定 executor 时返回明确 unavailable 运行结果。

MCP 与 command 共享 matcher、timeout、trust、status message、run records 和结果校验。

## 7. 输出解析和聚合

Command/MCP 输出先按 Codex shape 校验，再按事件解析：

- 任何未知或不适用字段使该 handler 决策失效；
- `reason` 不能脱离相应 decision 使用；
- unsupported `suppressOutput` / MCP output rewrite 不会静默生效；
- 非法 JSON 或 schema mismatch 记录在 `HookRunRecord.error`；
- 默认 fail-open，继续评估其他 handler。

聚合原则：deny/block 优先；允许票只能在没有 deny 时生效；输入修改按稳定配置顺序应用；additional context 按完成顺序收集；异步 handler 不参与当前同步决策。

## 8. 异步所有权

每个 `CommandHookRunner` 的异步 runtime 使用 semaphore 限制最多 8 个并发任务，并通过 `JoinSet` 持有它们。`with_mcp_executor()` 是 session 绑定边界，会创建独立异步 runtime 和环境快照。`HookRuntime::shutdown()` 取消并 drain 未完成任务。

因此配置和 run store 可以共享，但异步任务 ownership 不能跨 session 泄漏。

## 9. 可观测性与持久化

每个 handler 执行生成 `HookRunRecord`，包含：

- run/handler/event id；
- handler type、sync/async mode、scope/source/trust；
- start/end/duration/status；
- status message、summary、输出条目和错误。

Core observer 将其映射为 `EventMsg::HookStarted` / `HookCompleted`，Server 再投影到 UI。这些事件是运行时 telemetry，不进入 durable rollout；重启恢复不承诺重放 hook 动画或运行中状态。

影响 Agent 状态的最终结果必须由其所属 durable event 体现，例如 tool output、turn terminal、compaction replacement，而不是依赖 HookCompleted 回放。

## 10. 失败模型

| 故障 | 行为 |
| --- | --- |
| 配置未知字段 | 拒绝该配置文件/handler |
| 非 canonical 事件 | 忽略并告警 |
| project 未信任 | source 可见但禁用 |
| hash 改变 | 标记 modified，等待重新信任 |
| command spawn/timeout/non-zero | 记录失败，按合法输出与事件策略 fail-open |
| 输出超限/非法 shape | 丢弃控制决策，记录错误 |
| MCP executor 未绑定 | 结构化失败，不伪装成功 |
| session shutdown | 取消异步任务并排空 |

## 11. 验证要求

必须覆盖：

- 12 个事件名称与顺序；
- 每事件 stdin 字段和 output allowlist；
- block/permission/modify/context/keep-going 聚合；
- user/project 加载、trust 与 hash 变化；
- command/MCP 同构 run record；
- async 上限与 shutdown；
- SessionEnd exactly-once；
- Stop continuation guard；
- Unix/Windows 子进程树终止；
- HookStarted/Completed 为 transient。

运行与配置示例见 [`../../hooks.md`](../../hooks.md)。
