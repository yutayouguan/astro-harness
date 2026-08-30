# Astro Hook 契约

Astro 有三条 Hook 通道：

1. **Plugin Hooks** — 进程内 Rust callback，可观察或改变 Agent 流程。
2. **Gateway Event Hooks** — `~/.astro/hooks/<name>/HOOK.yaml` 声明的 Gateway 事件。
3. **Shell Hooks** — `~/.astro/config.yaml` 中的异步 shell 命令。

对外配置与 `HookInput.hook_event_name` 统一使用 PascalCase canonical 名称。本页同时记录 Batch A 的**命名与 serialization 契约**，以及 B1 的**统一派发与生命周期契约**。

可复制的配置样例见 [`docs/examples/hooks/`](./examples/hooks/)。

## 1. Canonical 事件名

### Codex 公开事件

以下 11 个名称是 Codex 对外事件集，拼写和大小写都是契约的一部分：

| Canonical 名称 | 契约含义 |
|---|---|
| `PreToolUse` | 工具使用前 |
| `PermissionRequest` | 权限请求 |
| `PostToolUse` | 工具使用后 |
| `PreCompact` | 上下文压缩前 |
| `PostCompact` | 上下文压缩后 |
| `SessionStart` | 会话开始 |
| `SessionEnd` | 会话结束 |
| `UserPromptSubmit` | 用户 prompt 提交 |
| `SubagentStart` | 子 Agent 首轮开始前（每 thread 一次） |
| `SubagentStop` | 子 Agent 每个 turn 准备结束时；可 `KeepGoing` |
| `Stop` | Agent 准备停止 |

### Astro 扩展事件

以下名称不属于 Codex 的 11 个公开事件，是 Astro 的扩展：

| Canonical 名称 | 用途 |
|---|---|
| `PreLlmCall` | LLM 调用前 |
| `PreApiRequest` | Provider API 请求前 |
| `PostApiRequest` | Provider API 请求后 |
| `TransformTerminalOutput` | 替换 terminal 输出 |
| `TransformToolResult` | 替换工具结果 |
| `TransformFinalLlmOutput` | 完整流已发出后，替换普通 candidate 的最终 assistant 文本（summary 除外） |
| `PostLlmCall` | 普通 candidate 已完成 transform 后（summary 除外） |
| `PostApprovalResponse` | 审批结果产生后 |
| `PreGatewayDispatch` | Gateway 入队前 |
| `SessionReset` | 会话重置 |
| `GatewayStartup` | Gateway 启动 |
| `AgentEnd` | 单次 Agent run 收尾 |
| `CommandNewChat` | 新建对话命令 |

`AgentEnd` 是 Astro 扩展，不是 `SessionEnd` 的另一种拼写。

### Canonical-only 名称契约

Plugin 注册与触发、Gateway manifest、Shell config key 均按字符串精确匹配，只接受上表的 canonical 名称或显式自定义事件名。运行时不猜测、不改写，也不提供旧事件名称兼容层。`ASTRO_HOOK_EVENT` 始终原样携带本次 canonical dispatch 名称。

## 2. 当前实际触发行为

下表记录当前真实 fire 点和结果。每个由 `Session` 发出的事件都会补全会话字段，并经同一个 `HookRuntime` 投递到 Plugin、Gateway 与 Shell；同步控制决策由 Plugin handlers 聚合，Gateway/Shell 当前为观察型 transport。

### Plugin/Agent 事件矩阵

| Canonical 名称 | 分类 | 当前实际 fire 点 | 可用 `HookOutcome` |
|---|---|---|---|
| `SessionStart` | Codex | 首次**成功（非 `Block`）**的 prompt admission 时、`UserPromptSubmit` 前；`source` 为 `startup`（空历史）或 `resume`（已恢复历史） | `Block(reason)` / `InjectContext(context)` |
| `UserPromptSubmit` | Codex | 每个初始或 steer 用户输入持久化前；带输入 `prompt` 和活跃 `turn_id`（若已有） | `Block(reason)` / `InjectContext(context)` |
| `PreLlmCall` | Astro | 初始用户输入已经接纳、system prompt 构建后，在首个 sampling 前 | `InjectContext` |
| `PreApiRequest` | Astro | 每次**普通主循环** Provider sampling request 前；带初始 provider/model、sampling `attempt`、`duration_ms=0`、`status=started` | 观察 |
| `PostApiRequest` | Astro | 每次**普通主循环** `ProviderStreamer::stream_chat` 返回 stream 或 error 后；带实际命中的 provider/model、建连/首包 `duration_ms`与 `succeeded|failed` status，不等待 token stream 消费完毕 | 观察 |
| `PermissionRequest` | Codex | 串行工具权限 preflight 需要审批时，在决议与 `PreToolUse` 前 | deny 优先；否则 allow；全 abstain 进入原审批流 |
| `PostApprovalResponse` | Astro | 审批决议后、`PreToolUse` 前；`choice` 当前可为 `allowlist`、`auto`、`allow`、`allow_once`、`deny`、`timeout` 或 `unavailable` | 观察 |
| `PreToolUse` | Codex | 所有 preflight 均已授权或无需授权后，在工具 dispatch 前；被拒绝/超时/无审批通道的调用不会 fire | `Block(reason)` / `Modify(args)` |
| `TransformTerminalOutput` | Astro | `terminal` 原始 stdout/stderr 组合后、64 KiB 截断前；当前经工具 `hook_bus` direct fire，**仅 Plugin** | `ReplaceText(text)` |
| `TransformToolResult` | Astro | 任意工具返回后、`PostToolUse` 前 | `ReplaceText(text)` |
| `PostToolUse` | Codex | 工具结果已应用 `TransformToolResult` 后 | `Block` 替换模型可见结果；`InjectContext` 追加上下文；`ReplaceText` 追加反馈 |
| `PreCompact` | Codex | 非空自动压缩计划执行前，或 Desktop 手动压缩前；`trigger=auto|manual` | `Block` / `Skip` 停止压缩；自动路径同时结束当前 turn |
| `PostCompact` | Codex | 自动压缩写入成功后，或 Desktop 手动 split 成功后 | `Block` / `Skip` 结束当前自动 turn；不回滚已完成压缩 |
| `SubagentStart` | Codex | 子 Agent startup admission、首次 Provider request 前；继承完整 `HookRuntime` | 仅 `InjectContext`；阻断/停止请求按 Codex 语义忽略 |
| `SubagentStop` | Codex | 子 Agent 每个 terminal turn；错误/中断也触发；Desktop close 不重复触发 | `KeepGoing(prompt)` 与主 Agent `Stop` 相同 |
| `Stop` | Codex | 每个无工具的普通 terminal candidate，以及 budget-exhaustion summary 的完整 terminal candidate | `KeepGoing(prompt)` |
| `TransformFinalLlmOutput` | Astro | 每个通过 Stop guard 的普通 candidate（含 tool-call 中间轮）的流已全部发出后、持久化前 | `ReplaceText(text)` |
| `PostLlmCall` | Astro | 每个通过 Stop guard 的普通 candidate，且已应用 `TransformFinalLlmOutput`；携带 LLM telemetry 字段 | 观察 |
| `AgentEnd` | Astro | `RegularTask` 在每次 regular run 收尾时唯一派发：成功、runtime failure、准备失败、取消或 receiver close 都恰好一次 | 观察 |
| `PreGatewayDispatch` | Astro | gRPC/Tauri chat 入站、加载会话前 | `Allow` / `Skip(reason)` / `Rewrite(message)` |
| `SessionReset` | Astro | UI `new_chat` 释放旧会话时 | 观察 |
| `SessionEnd` | Codex | session-owned runtime 真正释放时；没有 live runtime 的 new-chat fallback 同样触发 | 观察；`reason=other` |

`SessionStart` 只有在其 outcome 非 `Block` 时才清除 pending source。若它被 `Block`，source 会保留，下一次 admission 会再次向 Plugin、Gateway、Shell 派发同一 `SessionStart`；因此 handler 应保持幂等。`UserPromptSubmit` 的 `Block` 会在输入写入前阻断该 admission；`InjectContext` 与该输入绑定、按 FIFO 进入随后的 response chain，并受 prompt 预算约束。初始输入的 context 参与其首个请求；steer context 不会被 Stop 的 bridge sampling 提前消费。

所有公开事件均有生产 fire 点。`AgentEnd` 仍是 Astro 的单次 run 收尾扩展，不是 `SessionEnd`。

`AgentEnd` 的 runtime failure 会保留原始失败文本于 `error`；取消与 receiver close 属于非错误收尾，`error` 为空。准备阶段的失败也同样派发一次 `AgentEnd`，但保持其原有 task error 返回，避免把 runtime 已经输出过的 terminal 再输出一次。

Plugin callbacks 按注册顺序同步执行。通用事件中 `Continue` 和 `Allow` 会继续下一个 callback，首个其他 outcome 短路；`PermissionRequest` 会遍历全部 callback 并按 deny > allow > abstain 聚合，`PostToolUse` 会遍历全部 callback 聚合 block/context/feedback。Callback panic 被记录并当作 `Continue`。Shell Hook 是异步旁路，不参与 outcome 聚合。

### 当前执行顺序

```text
PreGatewayDispatch
  → SessionStart (startup | resume；首个 non-Block admission 后一次) → UserPromptSubmit
  → 输入持久化 → PreLlmCall → [sampling / 工具循环]
      → PreApiRequest → Provider API → PostApiRequest    # 每个主循环 request
      → 无 tool calls？Stop
          → KeepGoing：持久化 assistant + hook bridge 后 continue；不 fire Transform/PostLlm
          → 否：跳过 Stop
      → TransformFinalLlmOutput → PostLlmCall      # 通过 Stop guard，或本轮有 tool calls
      → assistant 持久化
      → 有 tool calls？
          → PermissionRequest? → 审批 → PostApprovalResponse?
              → 拒绝 / 超时 / unavailable：返回工具结果，不进入 PreToolUse
          → PreToolUse                              # 仅 preflight 通过/无需审批
              → 工具执行
              → TransformTerminalOutput? → TransformToolResult → PostToolUse
              → loop
      → 无 tool calls：收尾
  → RegularTask AgentEnd                           # Astro run-level extension
  → runtime release: SessionEnd(reason=other)       # once per Session

压缩：
  → PreCompact(trigger=auto|manual)
  → durable compressed views / manual split
  → PostCompact(trigger=auto|manual)

子 Agent：
  → SubagentStart (startup admission, once per thread)
  → sampling/tool loop
  → SubagentStop (every terminal turn; KeepGoing may continue)

预算耗尽 summary（独立分支）：
  → 直接 ProviderStreamer::stream_chat（无 PreApiRequest/PostApiRequest）
  → Stop → KeepGoing 时持久化 bridge 并继续 summary sampling
  → 通过 guard 时持久化/收尾（无 TransformFinalLlmOutput/PostLlmCall）
```

### `Stop` continuation guard

`Stop` 不依赖 `turn_wrote_disk`。每个 terminal candidate 都会得到包含 `turn_id`、完整 `last_assistant_message` 和 `attempt=N` detail 的 payload。新 response chain 的首个 candidate 设置 `stop_hook_active=false`；在接受 continuation 后的候选设置为 `true`。

每个 response chain 最多接受两次 `KeepGoing(prompt)`：接受时持久化 candidate assistant 与 `[astro:hook-context]` bridge user，再继续 sampling。第三个及之后的 candidate 仍派发 `Stop`（且 `stop_hook_active=true`），但忽略 `KeepGoing` 并正常收尾。新的已持久化 steer 输入会开始独立 response chain 并重置这个配额。budget summary 与主循环共享同一配额；summary instruction 是 request-local，notice 只出现一次。

普通主循环每个 provider request 都会 fire `PreApiRequest` / `PostApiRequest`。每个普通 candidate（包括含 tool-call 的中间轮）只有在没有被 `Stop::KeepGoing` 提前 continue 后，才 fire `TransformFinalLlmOutput` 和 `PostLlmCall`。`TransformFinalLlmOutput` 的命名明确表示它修改的是完整流结束后的定稿文本，不会回溯改写已发出的 streaming delta。budget summary 则直接调用 `ProviderStreamer::stream_chat`：它只处理 summary candidate 的 `Stop`、持久化与 continuation，不 fire `PreApiRequest`、`PostApiRequest`、`TransformFinalLlmOutput` 或 `PostLlmCall`。

为了维持角色交替，Stop bridge 后已排队的 steer 会先让 provider 响应 bridge；bridge 经过 reasoning-only retry 时这个顺序仍被保留。terminal assistant 持久化后才会消费 queued input；其绑定 context 也只在相应新 chain 的 sampling 使用。

### Gateway 当前触发点

| Gateway 事件 | 当前触发点 |
|---|---|
| `GatewayStartup` | `AstroServiceImpl::new` 内 `HookRuntime` bootstrap 成功后立即触发；此时 `Self` 尚未组装且早于 listener bind，bootstrap 失败则不触发，因此不是 readiness 信号 |
| `PreGatewayDispatch` | gRPC/Tauri chat 入站、加载 session 前 |
| `SessionStart` / `UserPromptSubmit` / `AgentEnd` / `SessionEnd` | 由 session 的统一 dispatch 到达 Gateway；无 live runtime 时 server 仅补 `SessionEnd` |
| `CommandNewChat` | UI `chat_control(new_chat)`；随后派发 `SessionReset` 并释放 runtime，释放完成后触发 `SessionEnd` |

未为 manifest 注册自定义 handler 时，bootstrap 会为已发现 manifest 安装 tracing fallback。Gateway handlers 是观察型，其返回值不参与 Plugin `HookOutcome`。

### UI 当前行为

- Hook 流事件使用 `ThreadEvent.item_started/item_completed` 中的 Hook `TurnItem`，Tauri 再投影
  `name` / `detail` / `outcome`，不借用 memory extension。
- 前端将其渲染为 `kind: "hook"` 的活动卡，标题是 canonical 事件名。
- 设置中的「Hook 事件」开关控制可见性；`normal` / `detailed` 预设开启，`compact` 关闭。
- UI 新建对话调用 `chat_control(new_chat)`，对应上表的 `CommandNewChat` → `SessionReset` → runtime 卸载 → `SessionEnd`。

## 3. Canonical `HookInput` JSON

`HookInput` 是 Plugin/Gateway 之间共用的 canonical 载荷，`HookPayload` 是它的 Rust 类型别名。`hook_event_name` 在发送前会归一化为 canonical 名称。

### 必出字段

以下 key 总会出现在 JSON 中；字符串字段在上游未填充时可为空字符串，`transcript_path` 可为 `null`。

| JSON key | 类型 | 说明 |
|---|---|---|
| `session_id` | string | 会话 ID |
| `transcript_path` | string \| null | transcript 路径 |
| `cwd` | string | 当前工作目录 |
| `hook_event_name` | string | canonical 事件名 |
| `model` | string | 当前模型 |

### 可选字段

以下 key 仅在有值时序列化：

| JSON key | 类型 | 说明 |
|---|---|---|
| `turn_id` | string | 当前流式回合/run ID |
| `permission_mode` | string | 权限模式 |
| `source` | string | 事件来源 |
| `reason` | string | 原因 |
| `prompt` | string | 用户 prompt |
| `tool_name` | string | 工具名 |
| `tool_use_id` | string | 工具调用 ID |
| `tool_input` | any JSON | 结构化工具输入；不从其中猜测 prompt |
| `tool_response` | any JSON | 结构化工具响应 |
| `trigger` | string | 触发来源 |
| `agent_id` | string | Agent ID |
| `agent_type` | string | Agent 类型 |
| `agent_transcript_path` | string | Subagent transcript 路径 |
| `stop_hook_active` | boolean | Stop hook 是否正在处理 |
| `last_assistant_message` | string | 最近的 assistant 文本 |
| `provider` | string | LLM/API telemetry 的 provider backend；`PostApiRequest`/`PostLlmCall` 使用实际命中的 fallback 目标 |
| `attempt` | integer | 当前 turn 内的 sampling 序号，从 1 开始；不是 fallback 子尝试序号 |
| `duration_ms` | integer | 阶段墙钟耗时；`PostApiRequest` 为打开流，`PostLlmCall` 为完整流 |
| `status` | string | telemetry 状态：`started`、`succeeded` 或 `failed` |

### Astro 私有字段

`detail`、`system_prompt_chars`、`assistant_chars`、`turn` 和 `error` 这五个字段都不序列化到 canonical JSON，但 Plugin 与 Gateway 的 Rust callback 可直接读取它们。`detail` 会写入 `ASTRO_HOOK_DETAIL`，并且是 UI timeline 的首选显示；若它为空，UI 才依次回退到 tool、prompt/assistant message、`system_prompt_chars`、`assistant_chars` 和 `turn`。`error` 目前既不进入 Shell 环境变量，也不进入 canonical JSON，UI detail 也不会自动展示它。`turn` 是会话轮次计数，不等于 `turn_id`。

示例：

```json
{
  "session_id": "session-1",
  "transcript_path": null,
  "cwd": "/workspace",
  "hook_event_name": "PreToolUse",
  "model": "gpt-5.6-sol",
  "tool_name": "terminal",
  "tool_use_id": "call-1",
  "tool_input": {"command": "pwd"}
}
```

这是载荷的 serialization 契约，不是 Shell/Command Hook 的 JSON stdin/stdout 协议。Batch A 不会把该 JSON 自动写入 shell stdin，也不解析 shell stdout 为新的 command response。

## 4. 注册与配置

### Plugin Hooks

进程内使用 canonical Rust 常量注册：

```rust
use hooks::{HookOutcome, HookPayload, PluginContext, POST_TOOL_USE};

fn register(ctx: &PluginContext<'_>) {
    ctx.register_hook(POST_TOOL_USE, |payload: &HookPayload| {
        tracing::info!(tool = ?payload.tool_name, "tool finished");
        HookOutcome::Continue
    });
}
```

当前 `HookOutcome` 保留 `Continue`、`Block`、`Modify`、`InjectContext`、`Allow`、`Skip`、`Rewrite`、`ReplaceText` 和 `KeepGoing`。除上文所述 `PermissionRequest` / `PostToolUse` 专用聚合外，Plugin callbacks 使用首个非继续结果。Gateway 与 Shell 是观察型 transport：Gateway handler 不改变该 outcome，Shell 异步旁路也不阻塞或改变它。

### Gateway Event Hooks

Agent Thread 继承父 Session 的完整 `HookRuntime`，因此 `SubagentStart` / `SubagentStop` 会到达 Plugin、Gateway、Shell 和 UI timeline。Plugin 的 `SubagentStart` 只注入上下文，不会因 block/stop 取消子 Agent；`SubagentStop::KeepGoing` 可继续当前子 Agent turn。Hook 是进程内实时边界，重启不会回放历史回调。

`~/.astro/hooks/<name>/HOOK.yaml`：

```yaml
name: audit
description: 会话审计
events:
  - GatewayStartup
  - SessionStart
  - AgentEnd
  - CommandNewChat
```

Handler 可在进程内按 manifest `name` 绑定：

```rust
ctx.register_gateway_handler("audit", |event, payload| {
    tracing::info!(%event, session = %payload.session_id, "gateway hook");
});
```

### Shell Hooks

`~/.astro/config.yaml`（数据根可用 `ASTRO_MEMORY_DIR` 覆盖）：

```yaml
hooks:
  PreGatewayDispatch: 'mkdir -p "$HOME/.astro/logs" && echo "$ASTRO_HOOK_EVENT $ASTRO_HOOK_SESSION" >> "$HOME/.astro/logs/chat-audit.log"'
  CommandNewChat: 'true'
```

Shell Hook 异步执行，默认超时 5 秒，失败只记录日志。`HookRuntime::dispatch` 会在同一次标准化 dispatch 中同步调用 Plugin、通知 Gateway，并旁路调度同名 Shell Hook；因此所有 `Session::fire_hook` 事件都能到达 Shell。Shell 的异步结果不参与 Plugin outcome 聚合，也不会阻塞 Agent。

11 个 Codex 事件及 Astro 扩展事件都可经统一 runtime 到达 Shell；代表事件包括 `PreCompact`、`PostCompact`、`SessionEnd`、`PermissionRequest`、`PostToolUse`、`SubagentStart`、`SubagentStop`、`Stop`、`AgentEnd` 与 `CommandNewChat`。Shell 是异步观察面，不参与同步决策。

一版兼容环境变量：

| 变量 | 设置条件 | 值的来源 |
|---|---|---|
| `ASTRO_HOOK_EVENT` | 必出 | 始终是 canonical 事件名，即使调用方传入 legacy 名称 |
| `ASTRO_HOOK_SESSION` | 必出 | `session_id` |
| `ASTRO_HOOK_DETAIL` | 必出 | Astro 私有 `detail` |
| `ASTRO_HOOK_TURN` | `turn_id` 为非空字符串时 | `turn_id`；不是轮次计数 `turn` |
| `ASTRO_HOOK_TOOL` | `tool_name` 有值时 | `tool_name` |
| `ASTRO_HOOK_MESSAGE` | `prompt` 或 `last_assistant_message` 有值时 | 优先 `prompt`，否则 `last_assistant_message`；不从 `tool_input` 猜测文本 |
| `ASTRO_HOOK_PROVIDER` | `provider` 有值时 | provider backend |
| `ASTRO_HOOK_MODEL` | `model` 非空时 | 模型 ID |
| `ASTRO_HOOK_ATTEMPT` | `attempt` 有值时 | turn 内从 1 开始的 sampling 序号 |
| `ASTRO_HOOK_DURATION_MS` | `duration_ms` 有值时 | 毫秒耗时 |
| `ASTRO_HOOK_STATUS` | `status` 有值时 | `started`、`succeeded` 或 `failed` |

`tool_input` 和 `tool_response` 不写入环境变量，避免通过进程环境扩大敏感数据暴露面。
每次启动子进程时，Astro 会先从继承环境中移除上表十一个保留变量，再应用本次 payload；因此缺失的可选值不会读到父进程旧值。其他父进程环境仍正常继承。

### 遥测旁路与安全

[`docs/examples/hooks/telemetry-webhook.sh`](./examples/hooks/telemetry-webhook.sh) 把 `ASTRO_HOOK_EVENT`、session、turn、tool 以及 provider/model/attempt/duration/status 序列化为 JSON 并 POST 到 `ASTRO_TELEMETRY_URL`；默认把 `detail` 留空，只有显式设置 `ASTRO_TELEMETRY_INCLUDE_DETAIL=1` 才发送。可选 token 通过 `Authorization: Bearer` 头发送。脚本需要 `curl` 和 `jq`，未配置 URL 时静默退出，发送失败不阻塞 Agent。

推荐从已接线的 `GatewayStartup`、`PreGatewayDispatch`、`UserPromptSubmit` 或 `CommandNewChat` 开始配置该脚本。`PostToolUse`、`PreLlmCall`、`PostLlmCall` 等同样通过统一 runtime 路由；它们可用于 telemetry，但 Shell 失败仍只会留下日志。

- 不要在日志、echo 或 hook detail 中输出 telemetry token。
- `ASTRO_HOOK_DETAIL` 可能包含 prompt 原文、工具参数或工具结果预览；示例默认不发送它。只有确认接收端和数据策略安全后，才设置 `ASTRO_TELEMETRY_INCLUDE_DETAIL=1`。
- 示例脚本不发送 `ASTRO_HOOK_MESSAGE`、`tool_input` 或 `tool_response`；自定义脚本如果增加这些字段，需要先评估 prompt、assistant 文本和工具参数的敏感性。
- Shell 失败只记录 warning，不是投递成功保证；需要可靠导出时应在自有端点实现幂等和重试。

## 5. 已完成边界与后续范围

Batch A 完成：

- canonical 事件名与 legacy 名称归一化；
- `HookInput`/`HookPayload` 的 canonical JSON serialization；
- 旧 `ASTRO_HOOK_*` 环境变量的一版弃用兼容。

B1 与 Codex lifecycle alignment 完成：

- `Session::fire_hook` 到 Plugin、Gateway、Shell 的一次标准化 dispatch；
- one-shot `SessionStart`、输入 admission、Stop continuation guard 与 `RegularTask` 唯一 `AgentEnd`；
- canonical `SessionEnd` 与 server lifecycle 去重；
- 自动/手动 `PreCompact` / `PostCompact`；
- Permission allow/deny 与 PostToolUse block/context/feedback；
- Subagent 的完整 runtime transport、startup 与每-turn stop/continuation。

后续范围是 Batch C 的 Codex `hooks.json` Command Hook JSON stdin/stdout、matcher、multi-handler 聚合/冲突和 trust 模型/UI 管理。当前 Gateway/Shell transport 是观察型，只有同步 Plugin handlers 返回业务决策。`SessionStart(source=clear|compact)` 的 durable handoff 也仍属于后续 Session lifecycle 扩展；它不影响本轮 compact 边界事件。

不要从 canonical 名称推断尚未实现的 Command Hook matcher、trust 或 JSON I/O 语义。

## 6. 验证

```bash
cargo test -p hooks
cargo test -p agent --all-targets
cargo test -p server --all-targets
cargo check --workspace --all-targets
cargo fmt --all -- --check
git diff --check
```
