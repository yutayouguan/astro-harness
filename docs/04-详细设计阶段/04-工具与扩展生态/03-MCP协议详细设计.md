# MCP 接入系统详细设计

> **Harness 当前基线（2026-09-07）**：`McpHub` 是每 Agent 进程级连接池。工具在模型侧以原生 `namespace=mcp__{server_id}` + `name={tool_name}` 暴露，Hub 内部才使用 `mcp__{server_id}__{tool_name}` 执行键。普通 MCP tool 与 broker 都同时注册 `ToolEntry` 和 `DynamicToolAdapter` runtime，统一经 `ToolRouter -> ToolRegistry::dispatch` 执行。当 `tool_search` 可用时默认 Deferred，否则回退 Direct；激活不等于授权。Server instructions 是不可信 dynamic context，MCP prompt/resource 不得被当作 system authority。

> 阶段：详细设计
> 状态：当前实现基线；剩余项显式标注
> 适用范围：Astro MCP Host/Client、桌面端 MCP 管理界面、工具注册与审批
> 外部基线：[Codex Model Context Protocol](https://learn.chatgpt.com/docs/extend/mcp)（2026-08-17）

## 1. 设计结论

Astro 的 MCP 接入以 Codex MCP 配置与运行语义为兼容基线：

- 仅支持 **STDIO** 与 **Streamable HTTP** 两种传输。
- 不声明、不模拟旧版 SSE 传输；历史 `type = "sse"` 配置必须显式报错并由用户迁移。
- MCP Server 的配置、认证、启动、工具发现、审批和状态必须是完整的一等能力，不能只保存 `command` 或 `url`。
- MCP Server 初始化返回的 `instructions` 必须进入受控系统上下文。
- MCP 工具统一进入 Astro `ToolRegistry`、权限系统、审计链路和用量统计，不能绕过审批。
- 全局配置和可信项目配置遵循 Codex 分层语义；Astro 统一使用 `.astro/config.toml`，
  不再读取 Agent 私有 MCP 配置。
- Codex 公开文档只明确承诺 tools 与 initialize `instructions`；resources/resource templates/prompts 作为 Astro 扩展仅显式按需访问，不宣称为 Codex 自动行为。
- 当前阶段只设计 Astro 作为 MCP Host/Client。将 Astro 自身暴露为 MCP Server 不属于本轮范围，后续若需要应单独立项。

### 1.1 模型身份与内部执行键

MCP 工具不再以展平函数名暴露给 Responses 模型：

```json
{
  "type": "namespace",
  "name": "mcp__calendar",
  "description": "Tools in the mcp__calendar namespace.",
  "tools": [
    { "type": "function", "name": "list_events", "parameters": {} }
  ]
}
```

`ToolEntrySpec` 同时携带三种用途不同的字段：

| 字段 | 用途 | 示例 |
| --- | --- | --- |
| `namespace` | Responses schema / call 的结构化所有权 | `mcp__calendar` |
| `native_name` | Server `tools/list` 返回的子工具名 | `list_events` |
| `qualified_name` | Hub 内部连接解析与唯一执行键 | `mcp__calendar__list_events` |

`ToolRouter` 用 `(namespace, native_name)` 校验当前 Step 的模型调用，再回映射到
`qualified_name` 调用 `McpHub`。模型直接输出展平名不会命中路由。

Deferred MCP 工具由 `tool_search` 返回原生 namespace schema；同一 Server 的命中子工具
会合并到同一 namespace 容器。新 Step 从 durable `ToolSearchOutput` 重建
`ToolName` 集合，而不依赖独立的内存激活表。

### 1.2 端到端链路

```text
capture_step_context
  -> reload_tools_and_mcp
  -> ExtensionSnapshot.mcp_servers
  -> McpHub::reload_with_configs
  -> initialize / tools/list / enabled_tool_entries
  -> register_mcp_tool_entries
  -> ToolRegistry::register_dynamic(ToolEntry, DynamicToolAdapter)
  -> build_tool_router -> StepContext
  -> Prompt.tools -> ResponsesRequest.tools
  -> ResponseItem tool call
  -> model_can_call + MCP approval/HITL
  -> ToolRouter: (namespace, native_name) => qualified_name
  -> ToolRegistry::dispatch -> DynamicToolAdapter::handle
  -> McpHub::call_tool -> Peer::call_tool
  -> ToolOutput -> hooks -> final output budget
  -> matching ResponseItem output + rollout/session projection
```

`attach_mcp_tools()` 每次先 `unregister_toolset(MCP_TOOLSET)`，再根据 Hub 的健康连接快照重建元数据和 runtime。任何只有 schema、没有 `CoreToolRuntime` 的 MCP 条目都不得进入 Step Router。

## 2. 目标与非目标

### 2.1 目标

1. 与 Codex 对齐 MCP 配置字段、默认超时、工具过滤和审批语义。
2. 为 STDIO 与远程 HTTP Server 提供可诊断、可中断、可热重载的连接生命周期。
3. 支持 Bearer Token、环境变量 Header 与 OAuth，避免凭据明文进入普通配置文件。
4. 在桌面端提供连接状态、认证、重连、错误详情和工具策略管理。
5. 保持 Agent 隔离、项目隔离、沙箱权限和工具命名空间不变量。
6. 以显式、分页、按需的 broker 工具开放 resources、resource templates 和 prompts，不把 MCP 简化为纯工具协议。

### 2.2 非目标

- 不继续兼容旧 `/sse` + `/message` 双端点传输。
- 不允许把 SSE URL 静默当作 Streamable HTTP URL。
- 不在第一阶段实现远程执行环境中的 STDIO Server。
- 不允许 MCP 自定义配置提升当前 Session 或 Subagent 的权限。
- 不在 MCP 配置中保存 OAuth access token、refresh token 或可直接使用的长期密钥。

## 3. 当前实现与剩余验收差距

当前代码位于 `crates/agent-mcp`，已经具备 `rmcp` 客户端、STDIO/Streamable HTTP 连接、`tools/list_changed`、工具注册、Agent 级配置、启动/工具调用超时和基础沙箱约束。

需要消除的差距：

| 领域 | 当前实现 | 目标状态 |
| --- | --- | --- |
| 传输 | 仅 STDIO、Streamable HTTP；旧 SSE 配置显式报迁移错误 | 仅 STDIO、Streamable HTTP |
| 配置 | TOML；全局 → 显式可信项目 → Agent 整体覆盖；旧 JSON 不作为输入 | 已实现 |
| 认证 | 已支持环境凭据、OAuth Authorization Code + PKCE、系统 Keychain、登录/登出；显式 Authorization 优先，缺省先匿名连接 | 补齐固定 callback override 与企业会话认证边界 |
| 初始化 | 已消费 initialize `instructions`，仅保留健康连接内存快照，经长度与安全边界注入 system prompt | 补充真实 Server 端到端验收 |
| 超时 | 启动默认 10s、工具默认 60s；配置可完整 round-trip | 启动默认 10s、工具默认 60s |
| 失败策略 | optional 降级；`required` 结构化失败阻止 LLM | `required` 控制是否阻止任务开始 |
| 工具策略 | `enabled_tools` allow → `disabled_tools` deny → legacy bool gate；已接入 Server/Tool 审批策略与 annotations | 补充真实 Server 的端到端交互验收 |
| UI | 增删、刷新、开关、超时、运行状态、错误、重连、环境凭据引用、OAuth 登录/登出及审批策略编辑 | 补充保存失败回滚和结构化错误 |
| Event stream | manager 基础已完成：active 握手、attempt id、有界队列、权限/Server 生命周期取消 | 接入具体 Server opener、Desktop 订阅 RPC 与真实长流验收 |
| Extension reconcile | 当前 turn 冻结；变更生成 pending snapshot，下一 turn 激活并报告受影响能力 | 已实现 |
| 错误传播 | 部分 UI 保存错误被吞掉 | 所有持久化和连接错误可见 |

## 4. 模块边界

```text
apps/desktop/
├── src/components/settings/McpServersPanel.tsx
├── src/components/chat/ComposerMcpMenu.tsx
└── src-tauri/src/commands/mcp.rs

crates/agent-mcp/src/
├── lib.rs
├── config.rs          # 分层 TOML、inline 配置合并与严格校验
├── auth.rs            # Bearer、环境 Header、OAuth、Keychain 引用
├── event_stream.rs    # 进程级长流所有权、激活握手、attempt 与取消
├── hub.rs             # Agent/Session 连接池、instructions、tools 与 capability 生命周期
├── session.rs         # 单 Server 生命周期、超时、中断、通知
├── protocol.rs        # resources/resource templates/prompts 显式按需协议适配
├── policy.rs          # allow/deny、审批模式、required
└── names.rs           # mcp__{server} namespace + Hub qualified key
```

职责约束：

- `agent-mcp` 不读取 Provider 密钥，不决定用户审批结果。
- `agent-core` 负责把 MCP schema 和 instructions 组装进当前 Agent 回合。
- `agent-tools` 负责统一审批、审计、调用统计和 ToolResult 规范化。
- Tauri command 只暴露 DTO，不在前端持有 OAuth token。
- Desktop UI 只提交配置引用与授权动作，不直接操作凭据文件。

## 5. 配置契约

### 5.1 配置位置与优先级

```text
~/.astro/config.toml                 # 全局配置
<project>/.astro/config.toml         # 可信项目配置
```

合并优先级由低到高：

```text
系统策略 → 全局 → 可信项目
```

规则：

1. 项目 MCP 配置仅在项目被标记为可信后加载。
2. 同名 Server 使用高优先级配置整体覆盖；禁止字段级混合密钥来源。
3. 自定义角色的 MCP 只通过 `.astro/agents/<role>.toml` 声明，不读取
   `agents/<agent_id>/config.toml`。
4. Agent/Subagent 覆盖不得扩大父 Session 的网络、文件系统或审批权限。
5. 配置解析失败时保留上一份已验证快照，并向 UI 返回诊断。

### 5.2 Server 配置

```toml
[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]
env_vars = ["CONTEXT7_API_KEY"]
cwd = "/workspace"
startup_timeout_sec = 10
tool_timeout_sec = 60
enabled = true
required = false
enabled_tools = ["resolve-library-id", "query-docs"]
default_tools_approval_mode = "prompt"

[mcp_servers.context7.tools.query-docs]
approval_mode = "approve"

[mcp_servers.figma]
url = "https://mcp.figma.com/mcp"
auth = "oauth"
bearer_token_env_var = "FIGMA_OAUTH_TOKEN"
http_headers = { "X-Figma-Region" = "us-east-1" }
env_http_headers = { "X-Workspace-Token" = "FIGMA_WORKSPACE_TOKEN" }
```

### 5.3 规范字段

| 字段 | STDIO | HTTP | 说明 |
| --- | --- | --- | --- |
| `command` | 必填 | 禁止 | 不经 shell 执行 |
| `args` | 可选 | 禁止 | 参数数组 |
| `env` | 可选 | 禁止 | 非敏感静态环境值 |
| `env_vars` | 可选 | 禁止 | 从允许的环境源转发 |
| `cwd` | 可选 | 禁止 | 必须位于允许的执行根范围内 |
| `url` | 禁止 | 必填 | 仅 `http`/`https` |
| `auth` | 禁止 | 可选 | `oauth`，未来可信源可扩展 |
| `bearer_token_env_var` | 禁止 | 可选 | Bearer token 的变量名 |
| `http_headers` | 禁止 | 可选 | 非敏感静态 Header |
| `env_http_headers` | 禁止 | 可选 | Header → 环境变量名 |
| `startup_timeout_sec` | 可选 | 可选 | 默认 10，范围 1–120 |
| `tool_timeout_sec` | 可选 | 可选 | 默认 60，范围 1–3600 |
| `enabled` | 可选 | 可选 | 默认 true |
| `required` | 可选 | 可选 | 默认 false |
| `enabled_tools` | 可选 | 可选 | allow list |
| `disabled_tools` | 可选 | 可选 | deny list，最后应用 |
| `default_tools_approval_mode` | 可选 | 可选 | Server 默认工具审批 |
| `tools.<name>.approval_mode` | 可选 | 可选 | 单工具覆盖 |

### 5.4 传输解析

传输类型由互斥字段决定，不再接受独立 `type`：

```text
存在 command 且不存在 url → STDIO
存在 url 且不存在 command → Streamable HTTP
同时存在或同时缺失 → 配置错误
type/transport = "sse" → LegacySseUnsupported
```

错误必须包含迁移说明：

```text
Legacy SSE transport is not supported. Configure the server's Streamable HTTP /mcp endpoint instead; an /sse URL cannot be converted automatically.
```

## 6. 认证与密钥管理

### 6.1 凭据解析顺序

Streamable HTTP 请求按以下顺序组装 Header：

1. 加载非敏感 `http_headers`。
2. 解析 `env_http_headers`；已解析的环境 Header 覆盖同名静态 Header。
3. 解析 `bearer_token_env_var`；仅在前两步没有提供 `Authorization` 时写入 Bearer Header。
4. 若没有已配置的 Bearer/Authorization，则读取系统 Keychain 中与 Server id + URL 绑定的 OAuth 凭据；过期 token 由 `rmcp` 刷新并回写。
5. 所有凭据来源均未解析到时允许无认证连接，由服务端响应决定是否需要认证。

环境变量缺失时忽略对应引用，不把变量值回写配置或前端状态；环境变量名或 Header 语法非法时作为永久配置错误处理。

### 6.2 OAuth

OAuth 使用 Authorization Code + PKCE：

```text
用户点击 Authenticate
  → 发现 authorization server metadata
  → 生成 state + code_verifier/challenge
  → 系统浏览器登录
  → localhost 临时 callback
  → 校验 state
  → 换取 token
  → token 写入操作系统安全凭证库（macOS Keychain 等）
  → 配置文件最多保存 auth = "oauth"，不保存 token 或 credential_ref
```

安全要求：

- `state`、callback ID 和 PKCE verifier 必须是一次性随机值。
- OAuth token 不进入日志、SQLite、Tauri event payload 或 React state。
- refresh token 更新采用 Keychain 原子替换。
- 登出时删除 Keychain 条目，桌面端随后触发对应 Server 重连。
- callback 当前绑定 `127.0.0.1:0`；固定端口和 URL override 仍属于待实现配置能力。
- `auth = "chatgpt"` 仅属于 Codex 可信第一方集成契约；Astro 会明确拒绝，不能冒充 ChatGPT 会话认证。

### 6.3 STDIO 环境

- 子进程先 `env_clear()`，再注入安全父环境白名单。
- `env` 只允许非敏感常量；疑似 token/password/secret 字段触发配置诊断。
- 敏感值使用 `env_vars`，运行时解析，不回写实际值。
- 当前本地执行器只支持 Codex `env_vars` 的字符串/本地环境形式；`source = "remote"` 等待远程 STDIO 执行器落地后再启用。
- Subagent 继承父 Session 已解析后的权限边界，但重新解析自己的允许变量集合。

## 7. 连接生命周期

### 7.1 状态机

```text
Disabled
  └─ enable → ResolvingCredentials
                  ├─ auth missing → AuthRequired
                  ├─ config error → Error
                  └─ ok → Connecting
                              ├─ timeout/error → Error
                              └─ initialize → Discovering
                                                ├─ required failure → BlockingError
                                                ├─ optional failure → Error
                                                └─ success → Connected

Connected
  ├─ config/policy change → Draining → Connecting
  ├─ transport closed → Disconnected → Backoff → Connecting
  ├─ event stream → Starting → Active → Ended
  ├─ disable → Draining → Disabled
  └─ shutdown → Closed
```

### 7.2 启动策略

- 所有启用 Server 并行连接，但使用全局并发上限，默认 4。
- 每个 Server 的 resolve credentials、connect、initialize、首次 discovery 共享启动 deadline。
- `required = false` 的失败不会阻止 Agent 回合，但工具不可见且 UI 显示错误。
- `required = true` 的失败会阻止回合进入 LLM，并返回结构化诊断。
- 配置指纹未变化且 transport 健康时复用连接。
- 权限 profile、项目根、凭据引用或已解析环境凭据值变化必须重连；连接指纹只保留哈希，不保存原始凭据。

### 7.3 重连策略

- 仅对可重试错误进行指数退避：1s、2s、4s、8s、最大 30s，并加入 jitter。
- 认证失败、配置错误、权限拒绝不自动重试。
- 用户点击 Reconnect 会清零退避，但不跳过权限或认证检查。
- 当前实现采用事件驱动重试：活跃任务在每次 MCP reload 时检查到期 Server；空闲 Agent 不启动仅用于重连的后台线程。UI 显示连续失败次数和下一次重试时间。
- Server 删除/禁用或 Hub shutdown 时必须取消连接、未完成调用和对应 event streams。
- Thread task 卸载不等于取消 process-owned event stream；订阅按
  `(thread_id, subscription_id, stream_attempt_id)` 继续归属，权限 generation 变化时 fail closed。

## 8. 初始化与 Server Instructions

连接成功后保存初始化结果：

```rust
pub struct McpServerCapabilities {
    pub protocol_version: String,
    pub implementation_name: String,
    pub implementation_version: String,
    pub instructions: Option<String>,
    pub tools: bool,
    pub resources: bool,
    pub prompts: bool,
}
```

`instructions` 注入规则：

1. 仅已连接且启用的 Server 可注入。
2. 作为独立的外部不可信 system 上下文段，不拼进用户消息。
3. 每条记录以 JSONL 标记 Server ID/名称/来源，换行与分隔符只作为字符串数据。
4. 在 Astro 工具与安全指引之后注入；Server instructions 不能覆盖权限、审批、隐私、沙箱和系统安全指令。
5. 保留开头内容，确保 Server 作者可把前 512 字符写成自包含摘要；单 Server 最多 16,384 字符，单 Agent 总计最多 65,536 字符。
6. 内容只存于连接内存快照，reload 后刷新，不持久化配置，不修改历史 user message。
7. 纳入共享 system prompt 字符预算，并在现有 `mcp` 上下文用量段中按 Server 展示。

## 9. 能力发现与注册

### 9.1 Tools

首次初始化后调用 `tools/list`，并监听 `notifications/tools/list_changed`：

```text
native: search
server: context7
model namespace: mcp__context7
model child: search
hub qualified key: mcp__context7__search
```

过滤顺序：

```text
server.enabled
  → Server 已连接且 transport 未关闭
  → enabled_tools allow list
  → disabled_tools deny list
  → 单工具 enabled 覆盖
  → enabled_tool_entries
  → ToolRegistry 元数据 + runtime 注册
  → StepContext 的 interaction mode / Deferred 投影
  → 执行前 Session/Agent 权限与 MCP approval
```

`disabled_tools` 总是在 `enabled_tools` 之后应用。未知新工具在存在 allow list 时默认不可见；不存在 allow list 时按 Server 默认策略处理。

### 9.2 Resources 与 Prompts

协议层必须保留：

- `resources/list`
- `resources/read`
- `resources/templates/list`
- `prompts/list`
- `prompts/get`

当前通过两个独立、只读的 broker tool 暴露：

- `mcp_resources`：`list` / `templates` / `read`。
- `mcp_prompts`：`list` / `get`。

两者仅在健康 Server 的 initialize capabilities 明确声明对应能力时注册，并把当前可用 Server ID 写入 tool schema 枚举与描述。调用必须显式提供 `server_id` 与 action，list 类操作每次只拉一页并返回 cursor。返回保留 MCP JSON 中的 MIME、URI、annotations、messages 和 pagination，统一标记为外部不可信 tool result，且进入现有 tool spill/截断/压缩链。不自动注入 system prompt，不将 prompt message 当作系统指令或授权。

## 10. 工具审批与权限

### 10.1 审批模式

```rust
pub enum McpApprovalMode {
    Auto,
    Prompt,
    Writes,
    Approve,
}
```

语义：

- `auto`：服从 Session 总体审批策略和工具 annotations。
- `prompt`：每次调用都请求审批。
- `writes`：只读工具自动执行，写入/未知工具请求审批。
- `approve`：配置层预批准，但仍受沙箱、网络、交互模式和硬拒绝规则约束。

审批解析优先级：

```text
系统硬拒绝
  → Session/父 Agent 权限交集
  → 单工具 approval_mode
  → Server default_tools_approval_mode
  → Session 默认审批策略
```

没有可信 `readOnlyHint` 或本地策略声明的工具，在 `writes` 模式下按可能写入处理。

Astro 当前对 `auto` 采用 fail-safe 解释：只有 Server 同时明确声明 `readOnlyHint = true`、`destructiveHint = false`、`openWorldHint = false` 时，MCP 策略层直接放行；其他情况交给 Session 的全局审批器。`prompt` 与 `writes` 需要审批时强制由用户确认，`approval_policy = never` 则拒绝。annotations 来自外部 Server，属于不可信提示，只能影响 MCP 策略门禁，不能扩大文件系统、网络、进程或沙箱权限。

### 10.2 执行边界

- STDIO Server 进程在当前 Session 沙箱中启动。
- HTTP Server 连接需要当前 profile 允许网络访问。
- 单次工具审批不能修改常驻连接的权限快照。
- MCP 返回内容属于外部不可信数据，只能作为 ToolResult 进入模型历史。
- 工具调用、审批决定、Server ID、耗时、错误类别进入审计；敏感参数必须脱敏。

## 11. 调用与输出

```text
ResponseItem tool call
  → ToolRouter 校验当前 Step 的 (namespace, name)
  → 回映射 Registry qualified_name
  → 解析 MCP approval policy
  → HITL/自动审批
  → ToolRegistry::dispatch
  → DynamicToolAdapter::handle
  → McpHub::resolve_tool_peer
  → tool_timeout deadline + Peer::call_tool(native_name)
  → 结构化 ToolOutput（原始 output_token_limit）
  → TransformToolResult / PostToolUse
  → 最终 output_token_limit + spill/compression
  → matching ResponseItem output + rollout/session history
```

输出要求：

- `structured_content` 优先保留为 JSON。
- Text、Image、Audio、ResourceLink 和 EmbeddedResource 保留原始类型。
- 单次媒体数量、单项大小和总输出大小均有限制。
- `is_error = true` 转换为可识别的工具错误，不能伪装成成功文本。
- 超时取消当前 request；若协议层无法取消，结果到达后丢弃并记录 late response。

Event stream 的首条通知必须是 `notifications/events/active`，并在 90 秒 activation deadline
内到达；否则启动失败。`notifications/events/terminated` 正常收敛流，其他断开产生带 attempt
identity 的 ended update，旧 attempt 的迟到通知不能覆盖新订阅。

## 12. Desktop 与 Tauri 设计

### 12.1 DTO

前后端 DTO 必须覆盖完整配置，禁止字段在 UI round-trip 中丢失：

```rust
pub struct McpServerDto {
    pub id: String,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub env: BTreeMap<String, String>,
    pub env_vars: Vec<EnvVarRefDto>,
    pub url: Option<String>,
    pub auth: Option<String>,
    pub bearer_token_env_var: Option<String>,
    pub http_headers: BTreeMap<String, String>,
    pub env_http_headers: BTreeMap<String, String>,
    pub startup_timeout_sec: u64,
    pub tool_timeout_sec: u64,
    pub enabled: bool,
    pub required: bool,
    pub enabled_tools: Option<Vec<String>>,
    pub disabled_tools: Vec<String>,
    pub default_tools_approval_mode: McpApprovalMode,
    pub status: McpServerStatusDto,
}
```

任何新增配置字段都必须具有 DTO round-trip 测试。

### 12.2 设置界面

Server 卡片至少展示：

- `connecting / connected / auth-required / error / disabled`
- 传输类型与 endpoint
- 工具数量与 instructions 摘要
- required、启动超时、调用超时
- 默认审批和单工具审批
- Authenticate / Sign out / Reconnect / Disable / Remove
- 最近错误、发生时间和可操作修复建议

Composer MCP 菜单只展示真实状态，不把“配置存在”等同于“已连接”。

### 12.3 错误处理

- Tauri command 使用结构化错误 DTO，不只返回字符串。
- React 保存必须等待结果；失败时回滚本地状态并显示提示。
- Refresh 必须返回每个 Server 的结果，不能吞掉单个连接错误。
- OAuth 等待期间允许取消，窗口关闭时清理 callback listener。

## 13. 可观测性

关键指标：

- `mcp_connection_attempts_total{server,result}`
- `mcp_connection_duration_ms{server}`
- `mcp_tool_calls_total{server,tool,result}`
- `mcp_tool_duration_ms{server,tool}`
- `mcp_auth_refresh_total{server,result}`
- `mcp_reconnect_total{server,reason}`
- `mcp_instruction_tokens{server}`

日志不得包含：Authorization、Cookie、OAuth code、access token、refresh token、完整敏感参数或环境变量值。

## 14. 迁移方案

### Phase 0：立即修复（已于 2026-08-17 完成）

- [x] 删除伪 SSE 类型、UI 选项和 `/sse` 启发式推断。
- [x] 旧 `sse` 配置返回明确迁移错误，不静默转成 STDIO 或 Streamable HTTP。
- [x] 修复 `tool_timeout` 在 Tauri DTO round-trip 中丢失的问题。
- [x] 增加连接启动超时，并把工具默认超时从 300s 调整为 60s。

### Phase 1：配置与生命周期

- [x] 引入 TOML 配置模型和全局/项目/Agent 合并。
- [x] 工具发现缓存仅补丁当前可写层已显式定义的 Server，不把继承配置自动摊平。
- [x] 增加 `required`、allow/deny、执行根内 `cwd`、启动核心状态和并行连接（上限 4）。
- [x] 增加可重试错误分类、带 jitter 的指数退避（最大 30s）、重试元数据和真实 Hub Reconnect；HTTP 401 进入独立 `auth-required` 状态。
- [x] `ListMcpServers` 按 Agent/项目作用域返回带稳定 Server id 的 Hub 真实状态；Tauri 与工具设置页每 5 秒刷新连接状态并展示最近错误，不再把配置存在视为已连接。
- [x] 配置硬切为 TOML；旧 JSON 不再读取或自动迁移。

### Phase 2：安全认证与审批

- [x] 实现本地 `env_vars`、`env_http_headers`、`bearer_token_env_var`，连接时解析且不持久化实际值。
- [x] 桌面端表单与 JSON 导入支持上述 Codex 字段，只展示环境变量名和 Header 名。
- [x] 凭据值纳入隐私保护的连接哈希，环境值变化可触发重连且不会进入状态/日志。
- [x] 实现 OAuth Authorization Code + PKCE、系统 Keychain、loopback callback 与登录/登出命令；token/code/state 不进入 React 状态、Tauri payload 或普通配置。
- [x] 接入 Server/Tool `auto / prompt / writes / approve` 审批模式；兼容旧 bool 工具开关和 Codex `[tools.<name>] approval_mode` table。
- [x] 持久化并展示 `readOnlyHint / destructiveHint / idempotentHint / openWorldHint`；annotations 仅参与 MCP 策略判断，不提升基础权限。
- [x] MCP 审批复用统一 HITL、自动审批和隐私裁剪审计；获批后仍继续执行只读、网络、沙箱与硬拒绝检查。
- [x] UI 在 `auth-required` 时展示 Authenticate，已认证连接展示 Logout，并在完成后触发真实 Hub Reconnect。
- [x] UI 支持 Server 默认审批模式与单工具继承/覆盖编辑，并展示关键风险提示。

### Phase 3：完整上下文能力

- [x] 接入 server instructions：initialize 提取、健康连接内存快照、外部不可信 JSONL system 注入、长度/总预算限制和 `mcp` 用量归类。
- [x] 实现 resources、resource templates、prompts 显式按需 broker：capability gate、单页 cursor、Server 选择、超时和外部不可信结果边界。
- [x] instructions 已接入上下文用量统计。
- [x] broker tool schema 已归入现有 `mcp` 上下文用量段，返回内容复用 tool spill/截断/压缩链。
- 增加 Server capability 摘要与 broker 调用的桌面可视化。

## 15. 测试与验收

### 15.1 单元测试

- 传输字段互斥和 legacy SSE 拒绝。
- 配置三层合并与权限不扩张。
- DTO 所有字段 round-trip。
- `enabled_tool_entries` 的每个普通 MCP tool 都同时具有 `ToolEntry` 和 `CoreToolRuntime`。
- 原生 `(namespace, name)` 可通过 Step `ToolRouter` 回映射到 qualified key，不允许 schema-only 路由。
- `output_token_limit` 在 Hub 原始输出和 Core hook 变换后各执行一次。
- allow/deny 顺序和审批优先级。
- secret redaction、Header 冲突和环境变量缺失。
- instructions 注入顺序、长度和安全边界。

### 15.2 集成测试

- STDIO initialize/list/call/list_changed/close。
- Streamable HTTP 无认证、Bearer、OAuth refresh。
- startup/tool timeout 与取消。
- required/optional Server 启动失败。
- transport 断开、退避重连、配置热替换。
- Agent/Subagent 权限继承与项目信任门禁。

### 15.3 前端测试

- 添加、编辑、删除、启停和保存失败回滚。
- AuthRequired → Authenticate → Connected。
- 连接错误详情和 Reconnect。
- legacy SSE transport 显示明确迁移错误。
- 窄屏、键盘导航和敏感值不可见。

### 15.4 验收标准

1. 配置中不存在 SSE 选项，旧 SSE 配置不会被静默误连。
2. UI 读写不会丢失任何后端配置字段。
3. 慢或故障 Server 不会无限阻塞 Agent 回合。
4. required Server 失败能够阻止回合并给出可操作诊断。
5. OAuth/Token 不出现在普通配置、日志和前端状态中。
6. Server instructions 可追踪地进入系统上下文，但无法覆盖安全指令。
7. 每个 MCP 工具调用都经过 enablement、权限、审批、审计和超时链路。

## 16. 关联文档

- [工具系统详细设计](02-工具系统详细设计.md)
- [Agent 生命周期详细设计](../01-核心引擎层/07-Agent生命周期详细设计.md)
- [Hooks 系统详细设计](../01-核心引擎层/08-Hooks系统详细设计.md)
- [安全边界详细设计](../06-安全与基础设施/04-安全边界详细设计.md)
- [Tauri 桌面端详细设计](../05-桌面端与交互/01-Tauri桌面端详细设计.md)
- [MCP 协议接口](../../03-系统设计阶段/05-接口设计/03-MCP协议接口.md)
