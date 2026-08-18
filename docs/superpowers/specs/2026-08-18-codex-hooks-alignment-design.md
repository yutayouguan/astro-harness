# Codex Hooks 对齐设计

## 1. 目标

将 Astro App 的 Hook 体系对齐 Codex 公开契约，覆盖：

- 事件名称、Rust 常量名称与外部载荷字段；
- 生命周期触发语义与决策聚合规则；
- `hooks.json` / `config.toml` 配置、matcher 和 Command Hook 执行；
- 项目 Hook 的信任、禁用和可见失败；
- gRPC / Tauri / React 的真实 Hook outcome 展示；
- 旧 Astro Hook 名称和 `config.yaml` 的一版兼容期。

对齐的是 Codex 公开契约与用户可观察语义，不要求复制 Codex 内部实现。

## 2. 设计原则

1. Codex 名称是唯一规范名称；Astro 旧名只存在于边界兼容层。
2. 所有触发点经过同一个 `HookRuntime::dispatch` 路径，不得绕过 Command Hook、信任或 UI 记录。
3. 内部 Rust Plugin Hook 与外部 Command Hook 共享事件契约，但保留各自的执行形式。
4. 项目级脚本在获得明确信任之前不执行。
5. 改动按可独立验证的批次完成，每个批次保持工程可编译、测试可重跑。

## 3. 规范事件

### 3.1 Codex 事件

| 外部名称 | Rust 常量 | Astro 触发语义 |
|---|---|---|
| `SessionStart` | `SESSION_START` | 主会话启动、恢复、清空或压缩后，带 `source` |
| `SessionEnd` | `SESSION_END` | 主会话真正释放时，带 `reason` |
| `UserPromptSubmit` | `USER_PROMPT_SUBMIT` | 用户 prompt 进入 Agent 之前 |
| `PreToolUse` | `PRE_TOOL_USE` | 中央工具分发前，可 deny 或改写 input |
| `PermissionRequest` | `PERMISSION_REQUEST` | 系统准备展示权限审批之前 |
| `PostToolUse` | `POST_TOOL_USE` | 工具已产生结果后 |
| `PreCompact` | `PRE_COMPACT` | 手动或自动上下文压缩前 |
| `PostCompact` | `POST_COMPACT` | 手动或自动上下文压缩后 |
| `SubagentStart` | `SUBAGENT_START` | 子 Agent 首轮开始前 |
| `SubagentStop` | `SUBAGENT_STOP` | 子 Agent 准备结束一次任务时，可要求续跑 |
| `Stop` | `STOP` | 主 Agent 准备结束当前 turn 时，可要求续跑 |

### 3.2 Astro 扩展事件

Astro 特有观测或变换点保留，对外统一使用 PascalCase：

- `PreLlmCall`
- `PreApiRequest`
- `PostApiRequest`
- `TransformTerminalOutput`
- `TransformToolResult`
- `TransformLlmOutput`
- `PostLlmCall`
- `PostApprovalResponse`
- `PreGatewayDispatch`
- `PreVerify`
- `SessionReset`
- `SessionFinalize`
- `GatewayStartup`
- `AgentEnd`
- `CommandNewChat`

扩展事件不得代替或重复触发同语义的 Codex 事件。

## 4. 兼容名称

引入单一的 `canonical_hook_event_name` 映射，并在注册、配置加载和手工触发边界使用。运行时、gRPC、UI 和日志只传播规范名称。

| 旧 Astro 名称 | 规范名称 |
|---|---|
| `on_session_start` | `SessionStart` |
| `on_session_end` | `AgentEnd` |
| `pre_tool_call` | `PreToolUse` |
| `post_tool_call` | `PostToolUse` |
| `pre_approval_request` | `PermissionRequest` |
| `subagent_start` | `SubagentStart` |
| `subagent_stop` | `SubagentStop` |
| `pre_verify` | `Stop` |
| `pre_llm_call` | `PreLlmCall` |
| `pre_api_request` | `PreApiRequest` |
| `post_api_request` | `PostApiRequest` |
| `post_llm_call` | `PostLlmCall` |
| `transform_terminal_output` | `TransformTerminalOutput` |
| `transform_tool_result` | `TransformToolResult` |
| `transform_llm_output` | `TransformLlmOutput` |
| `post_approval_response` | `PostApprovalResponse` |
| `pre_gateway_dispatch` | `PreGatewayDispatch` |
| `on_session_reset` | `SessionReset` |
| `on_session_finalize` | `SessionFinalize` |
| `gateway:startup` | `GatewayStartup` |
| `agent:end` | `AgentEnd` |
| `command:new_chat` | `CommandNewChat` |

`pre_verify -> Stop` 只是配置兼容映射；新实现中 `Stop` 必须对每个准备结束的主 turn 生效，不再受“本轮已写盘”限制。

加载到旧名时记录一次去重的 deprecated warning，不在每次触发时重复输出。

## 5. Hook 载荷

### 5.1 共享字段

Command Hook 从 stdin 接收 JSON 对象，共享字段与 Codex 对齐：

```json
{
  "session_id": "session-id",
  "transcript_path": null,
  "cwd": "/workspace",
  "hook_event_name": "PreToolUse",
  "model": "provider/model",
  "turn_id": "turn-id",
  "permission_mode": "default"
}
```

`transcript_path` 无可用安定路径时为 `null`。`cwd` 使用当前 turn 的 project root / workspace root，不使用进程启动目录猜测。`model` 使用当前活跃 ChatTarget 的稳定 slug。`turn_id` 只出现在 turn-scoped 事件中；`permission_mode` 只出现在 Codex 契约要求的 Session、Tool、Prompt、Subagent 和 Stop 事件中。

### 5.2 事件字段

- `SessionStart`: `source`，值为 `startup | resume | clear | compact`。
- `SessionEnd`: `reason`，当前使用 `other`。
- `UserPromptSubmit`: `prompt`。
- `PreToolUse`: `tool_name`, `tool_use_id`, `tool_input`。
- `PermissionRequest`: `tool_name`, `tool_input`。
- `PostToolUse`: `tool_name`, `tool_use_id`, `tool_input`, `tool_response`。
- `PreCompact` / `PostCompact`: `trigger`，值为 `manual | auto`。
- `SubagentStart`: `agent_id`, `agent_type`。
- `SubagentStop`: `agent_id`, `agent_type`, `agent_transcript_path`, `stop_hook_active`, `last_assistant_message`。
- `Stop`: `stop_hook_active`, `last_assistant_message`。

内部 `HookInput` 采用可序列化的规范字段，不再用 `detail` / `message` / `tool_args` 作为对外契约。Astro UI 所需的摘要由独立 formatter 从规范字段生成。

### 5.3 旧环境变量

兼容期内 Command Hook 仍注入：

- `ASTRO_HOOK_EVENT`
- `ASTRO_HOOK_SESSION`
- `ASTRO_HOOK_TURN`
- `ASTRO_HOOK_DETAIL`
- `ASTRO_HOOK_TOOL`
- `ASTRO_HOOK_MESSAGE`

其值必须从规范 `HookInput` 派生；JSON stdin 是新的权威输入。

## 6. 统一调度器

### 6.1 调用链

```text
业务触发点
  -> HookEvent + HookInput
  -> HookRuntime::dispatch
      -> 规范化事件名
      -> 合并活跃配置层
      -> matcher 过滤
      -> Rust Plugin handlers
      -> Command handlers
      -> 按事件聚合决策
      -> HookDispatchResult
  -> 业务层应用决策
  -> 发布真实 UI / gRPC Hook event
```

`AgentLoop` 保存可共享的 `HookRuntime` / dispatcher 句柄，不再只保存 `PluginHookBus`。所有 Agent、Tool、Permission、Compression 和 Subagent 路径都调用调度器。

Gateway 清单发现可作为 Astro 扩展保留，但其事件也经过同一调度器，避免 Shell 旁路断链。

### 6.2 Rust Plugin handlers

- 注册时将旧名规范化。
- 处理器使用 `HookInput -> HookDecision`。
- panic 被转换为可见 Hook failure，不静默伪装成 `Continue`。
- 多处理器的结果不再简单“首个非 Continue 短路”，而是与 Command Hook 共享事件级聚合规则。

### 6.3 Command handlers

- 同一 matcher group 内匹配的 command handlers 并发启动。
- 默认超时 600 秒。
- `SessionEnd` 始终同步，默认 1 秒且最大 3 秒。
- `async = true` 的其他 handler 在后台运行，不提供可影响本次操作的决策。
- Unix 使用 `command`，Windows 优先使用 `commandWindows` / `command_windows`。
- stdout 按事件规则解析 plain text 或 JSON；stderr 用于失败与 exit code 2 的理由。

## 7. Matcher 与决策

### 7.1 Matcher

`matcher` 是正则表达式。`*`、空字符串或缺省表示全部匹配。

- `SessionStart` 匹配 `source`。
- `SessionEnd` 匹配 `reason`。
- `PreToolUse` / `PermissionRequest` / `PostToolUse` 匹配规范 `tool_name`。
- `PreCompact` / `PostCompact` 匹配 `trigger`。
- `SubagentStart` / `SubagentStop` 匹配 `agent_type`。
- `UserPromptSubmit` / `Stop` 忽略 matcher。

Astro 终端工具对外映射为 `Bash`；补丁工具使用 `apply_patch`，并接受 `Edit` / `Write` matcher alias；MCP 保持 `mcp__{server}__{tool}`。

### 7.2 决策聚合

- `PreToolUse`: deny 优先；无 deny 时只允许一个不冲突的 `updatedInput`；冲突改写记为 failure 并保持原输入。
- `PermissionRequest`: 任一 deny 则 deny；否则任一 allow 则 allow；全部 abstain 则进入原审批流。
- `PostToolUse`: block / `continue: false` 不回滚工具副作用，而是替换模型可见结果并继续 Agent 循环。
- `SessionStart` / `UserPromptSubmit`: 可注入 `additionalContext`；block 则不发起本次模型请求。
- `PreCompact` / `PostCompact`: `continue: false` 分别在压缩前和压缩后结束当前继续流。
- `Stop`: block 表示生成续跑 prompt；`continue: false` 的停止决策优先。
- `SubagentStop`: 聚合规则与 `Stop` 一致，但续跑目标是子 Agent。
- `SessionEnd`: 仅观察，输出不保持会话存活。

## 8. 生命周期接线

### 8.1 主会话

1. 首次创建 Session 时触发 `SessionStart(source="startup")`。
2. 恢复持久化 Session 时使用 `resume`。
3. 清空会话后的新流使用 `clear`。
4. 压缩后在下一次模型请求前触发 `SessionStart(source="compact")`。
5. 每条用户输入先触发 `UserPromptSubmit`，再记录到会话并进入 Agent 循环。
6. 每次真正工具分发使用 `PreToolUse` / `PostToolUse`。
7. 权限请求进入 HITL 前使用 `PermissionRequest`。
8. Agent 准备结束 turn 时触发 `Stop`。
9. 每次 chat run 结束的 Astro 观测事件是 `AgentEnd`，不是 `SessionEnd`。
10. 会话被删除、归档、应用正常关闭或运行时显式释放时触发 `SessionEnd`。

Astro 不在本设计中引入“30 分钟无客户端连接自动结束”机制；该能力需要独立的 session lease 设计。

### 8.2 压缩

手动与自动压缩共享一个包装器：

```text
PreCompact(trigger)
  -> compact
  -> PostCompact(trigger)
  -> SessionStart(source="compact")
```

中途失败会产生 Hook/compaction failure 事件，不伪造 `PostCompact`。

### 8.3 Subagent

- `SubagentStart` 在子 Agent 首轮模型请求前触发。
- 子 Agent 一次任务准备进入 completed 时触发 `SubagentStop`。
- 若 Hook 要求续跑，将 reason 作为新的 follow-up prompt，并设置 `stop_hook_active=true`。
- 防循环使用与主 `Stop` 一致的续跑计数与硬上限。
- 线程真正 close 只是资源生命周期，不额外伪造第二个 `SubagentStop`。

## 9. 配置发现

### 9.1 支持的源

Astro 使用 Codex Hook schema，支持：

- `~/.astro/hooks.json`
- `~/.astro/config.toml` 内联 `[hooks]`
- `<repo>/.codex/hooks.json`
- `<repo>/.codex/config.toml` 内联 `[hooks]`
- 已启用 Astro Plugin 打包的 `hooks/hooks.json`
- 兼容期的 `~/.astro/config.yaml` `hooks:` 字符串映射

Astro 不自动读取 `~/.codex/hooks.json` 或 `~/.codex/config.toml`，避免未经明示授权执行另一个产品的全局脚本。项目 `.codex` 是两端共享的明确兼容层。

### 9.2 合并

- 所有活跃源的匹配 Hook 都运行，高优先级层不覆盖低优先级 Hook。
- 同一层同时有 `hooks.json` 和内联 `[hooks]` 时合并并警告。
- 项目 `.codex` 只在项目已信任时加载。
- 旧 YAML 字符串自动转换为一个无 matcher 的 command handler。

### 9.3 当前 handler 类型

与 Codex 当前公开行为一致，只执行 `command` handler。`prompt` 和 `agent` handler 可解析、显示为 skipped，但不执行。

## 10. 信任模型

- 非托管 command hook 的信任单位是规范化 Hook 定义内容的 hash。
- 新增或变更后状态为 `review_required`，在审核前跳过执行。
- 用户可对单个定义执行 trust、disable 或 re-enable。
- 信任记录保存在 `~/.astro/hook-trust.json`，不写入项目仓库。
- 项目整体信任与 Hook 定义信任是两层条件，两者均满足才可执行项目 command hook。
- 定义中的密钥、token 或环境变量值不进入日志、UI 或 hash 摘要文本；hash 仍覆盖完整规范定义。

## 11. gRPC / Tauri / UI

新增 Hook 管理能力：

- list discovered hook definitions and sources；
- trust changed/new hook；
- disable / enable non-managed hook；
- list recent hook runs and failures。

Chat 流中的 Hook 事件至少包含：

- `hook_event_name`
- `source`
- `handler_id`
- `status`: `running | allowed | denied | modified | continued | failed | skipped`
- `summary`
- `duration_ms`

UI 不再在观察 handler 中固定写入 `continue`，而是在调度器完成聚合后发布真实结果。对异步 handler，UI 先收到 `running`，完成后再收到终态。

设置页增加 Hook 管理区，显示来源、matcher、command 摘要、hash 变更状态和开关。审核时必须显示完整 command，不只显示摘要。

## 12. 错误处理

- 配置语法错误：跳过该文件，保留其它源，并产生可见诊断。
- matcher 非法：禁用该 matcher group，不退化成全匹配。
- 未信任：记为 `skipped/review_required`，不启动进程。
- 超时：终止子进程，记录 failure，按该事件的 fail-open / fail-closed 契约继续。
- 非零退出：exit code 2 按 Codex 的 block / feedback 语义处理；其它非零码记为 failure。
- stdout 非法：记为 failure，不应用部分解析的决策。
- Plugin panic：转换为 failure，继续其它 handler。
- 冲突改写：记录 failure 并使用未改写的原始值。

默认不因单个 Hook 实现错误导致 Agent 崩溃。明确的 deny / block 决策不是实现错误，必须应用。

## 13. 分批实施

### 批次 A：规范契约与兼容层

- `HookEvent`、规范常量、alias 映射、`HookInput`、`HookDecision`。
- 旧 API 和旧字段的过渡适配。
- UI / 日志输出规范事件名。

### 批次 B：统一调度与生命周期

- Agent / Tool / Permission / Compression / Subagent 统一经过 dispatcher。
- 修正 SessionStart / SessionEnd / AgentEnd 语义。
- 实现 UserPromptSubmit、Pre/PostCompact、Stop 和可续跑 SubagentStop。

### 批次 C：Command Hook 执行器

- Codex schema 解析、matcher、JSON stdin/stdout、并发、异步、timeout、exit code 2。
- 决策聚合与旧 `ASTRO_HOOK_*` 环境变量。
- 修复现有 Shell Hook 断链。

### 批次 D：发现、信任与管理 UI

- 用户/项目/Plugin 配置源。
- hash 信任、disable/enable、后端命令和设置页。
- 真实 Hook outcome 和 failure 流。

### 批次 E：文档与兼容收尾

- 更新 `docs/hooks.md` 和可复制示例。
- 说明 deprecated aliases 和迁移方法。
- 保留兼容代码一个发布周期，具体移除由后续独立发布决策确定。

## 14. 测试与验证

### 14.1 单元测试

- 每个 legacy alias 都映射到唯一规范名。
- 规范事件序列化与 Codex 字段一致。
- matcher 匹配、全匹配和非法正则处理。
- PreToolUse、PermissionRequest、PostToolUse、Stop 的多 handler 聚合。
- 信任 hash 在定义修改时变化，密钥不出现在展示摘要。

### 14.2 集成测试

- startup/resume/clear/compact 的 SessionStart 顺序。
- SessionEnd 不在每次 chat run 后触发。
- UserPromptSubmit block/context 语义。
- Tool 输入改写、拒绝、后置 feedback 与媒体结果保留。
- PermissionRequest deny 优先与 allow 跳过 HITL。
- 手动/自动压缩的 Pre/PostCompact 与 compact SessionStart。
- Stop / SubagentStop 续跑上限。
- Command Hook JSON stdin/stdout、exit 2、超时、异步与 SessionEnd 同步。
- 项目未信任、Hook 未审核、hash 变更和 disable。
- gRPC / Tauri / React 显示真实 outcome。

### 14.3 验证命令

```bash
cargo test -p hooks
cargo test -p agent --all-targets
cargo test -p server --all-targets
cargo check --workspace --all-targets
cd apps/desktop && npx tsc --noEmit
```

当批次涉及前端产物或 Tauri 合同时，追加：

```bash
cd apps/desktop && npm run build
```

## 15. 非目标

- 不在本设计中实现 prompt handler 或 agent handler。
- 不引入第三方动态库加载。
- 不自动执行 Codex 用户全局 Hook。
- 不在本设计中实现基于 30 分钟无活跃的 SessionEnd lease。
- 不要求 Astro 扩展事件成为 Codex 公开契约的一部分。

## 16. 验收标准

1. 新配置、UI、gRPC 和日志中不再出现旧 snake_case 事件名，除了明确的 deprecated warning。
2. 旧 Astro Hook 配置仍能触发对应规范事件。
3. Command Hook 收到与 Codex 同名的 JSON 字段。
4. 十一个 Codex 事件都有实际生命周期触发点和集成测试。
5. PermissionRequest、PreToolUse、PostToolUse、Stop 和 SubagentStop 决策真正影响执行流。
6. 未信任或定义已变更的项目 Hook 不启动进程。
7. UI 显示真实 Hook 结果和失败，不再固定为 `continue`。
8. 旧 Shell Hook 的核心生命周期断链被修复。
