# Responses API 原生工具协议与 Astro 工具协议详细设计

> **Astro 当前基线（2026-09-04）**：工具主链已按 Codex 的 Step-scoped tool plan 结构重构；Direct、CodeMode 与 CodeModeOnly 三种投影均已接入，Responses 定义、调用、路由、输出与恢复全链路保留结构化 `(namespace, name)`。

> 阶段：详细设计
>
> 状态：已实现（对应提交 `9f04e810`、`4563aad2`、`74f0c9b1`）
>
> Step-scoped tool plan 重构：`e3ac2c51`
>
> 全链路 fail-closed 审计：`cc1ddc63`、`1a841049`
>
> 参考实现基准：Codex `e24190caa9ee355044a7d70177d48a556d766d35`（2026-08-26）
>
> 范围：Responses API 原生工具协议、Astro 工具协议、延迟工具发现、Responses 事件回放、Direct / CodeModeOnly 模式合同与 QuickJS Code Mode runtime

## 1. 文档目标

本文描述 Responses API 原生工具协议以及 Astro 在注册、暴露、路由、执行和历史回放上的工具协议。Codex 仅作为 OpenAI 协议的参考实现，不作为协议名称或协议归属。本文重点回答三个问题：

1. 这些机制解决了什么实际问题；
2. 为什么不再把所有工具都压成普通 JSON Function；
3. 从工具注册、Provider 请求、流式事件、执行到历史回放，完整语义如何保持。

文中的类型、字段、分支与降级规则均对应当前仓库。

## 2. 原始问题

传统工具层往往只有一种表示：

```json
{
  "type": "function",
  "function": {
    "name": "some_tool",
    "parameters": { "type": "object" }
  }
}
```

当工具类型只有普通 JSON 参数时，这种形式足够。但 Codex 工具链同时需要：

- 原始文本或语法约束输入，例如 `apply_patch` 和 `exec`；
- 有层次的工具组，例如 `cron.add` / `cron.list`；
- 不在首次请求中注入的延迟工具；
- 由 Provider 执行的原生网络搜索；
- 在 JavaScript cell 中组合多个工具，而不是每一步都让模型重新采样。

如果强行把它们全部展平为 Function，会出现四类语义丢失：

- 原始文本被二次 JSON 转义，patch 或脚本容易损坏；
- namespace 只剩字符串命名约定，Provider 无法理解分组；
- 所有 MCP 工具每轮全量进入上下文，token 和选择噪声随工具数线性增长；
- Responses API 的 call/output 类型在历史中不成对，导致请求被拒绝或模型丢失工具状态。

## 3. 总体架构

```text
CoreToolRuntime / ToolExecutor
  -> ToolRegistry
  -> build_tool_router / finalize_tool_router
  -> ToolRouter { registry, model_visible_specs, routes }
  -> StepContext { tool_router, history, prompt_context }
  -> build_prompt()
  -> Prompt { instructions, input, tools }
  -> ResponsesRequest
  -> Responses API
  -> ResponseItem
  -> ToolRouter::build_tool_call()
  -> approval / sandbox / hooks
  -> ToolRouter::dispatch()
  -> ToolRegistry::dispatch()
  -> CoreToolRuntime::handle()
  -> ResponseItem output / durable history
```

核心原则是：Registry 中的可执行 Runtime、本 Step 的模型可见 schema、Provider 请求和响应调用必须来自同一份冻结快照。`ToolRouter` 同时持有 `registry` 和 `model_visible_specs`，`StepContext` 再持有该 Router，因此 MCP 热重载或 Skill 激活不会改变已发出 sampling request 的执行边界。

`Prompt.tools` 是 Agent Core 对本次请求的权威投影；Provider 层只负责将它解析为 `ResponsesRequest.tools`。返回的原生 item 必须由 `ToolRouter::build_tool_call()` 重建结构化工具身份，再交回同一 Router 内的 Registry 执行。

### 3.1 链路的 fail-closed 规则

- `build_tool_router` 拒绝没有 `CoreToolRuntime` 的模型可见 schema，防止“可见但不可执行”。
- 两个 registered name 不得投影到同一 `(namespace, name)`；冲突在 Step 构建时报错，不依赖 HashMap 迭代顺序选择 handler。
- Provider 将 `Prompt.tools` 解析为 `ToolDefinition` 时不得使用静默 `filter_map`；任一无效 schema 都终止请求。
- `ToolRouter::build_tool_call()` 仅接受 `execution: "client"` 的 `tool_search_call`；Provider 执行的搜索不得被本地重复执行。
- Function arguments 无法解析为 JSON 时设置 `args_parse_error`，在进入审批或 handler 前返回参数错误。
- 用新 `ToolEntry` 覆盖旧元数据时必须同时使旧 Runtime 失效，直到新 Runtime 显式绑定。

## 4. Responses API 原生工具协议

### 4.1 为什么需要 tagged union

`ResponsesRequest.tools` 不再是“一组看起来像 Function 的 JSON”，而是 `ToolDefinition` 的有类型联合：

| 变体 | Responses wire type | 输入形式 | 主要用途 |
| --- | --- | --- | --- |
| `Function` | `function` | JSON object | 标准结构化参数工具 |
| `Freeform` | `custom` | 原始字符串 + grammar | patch、脚本等文本 DSL |
| `Namespace` | `namespace` | 子工具集合 | 显式表达工具所属域 |
| `ToolSearch` | `tool_search` | JSON object | 客户端发现并加载延迟工具 |
| `WebSearch` | `web_search` | Provider 选项 | Provider 托管的搜索能力 |

代码位置：

- `crates/agent-providers/src/types/request_content.rs`：`ToolDefinition` 及其子类型；
- `crates/agent-providers/src/types/request.rs`：`ResponsesRequest.tools`；
- `crates/agent-providers/src/dispatch.rs`：从 Registry JSON 解析为强类型定义；
- `crates/agent-tools/src/engine/registry.rs`：生成模型可见 schema。

### 4.2 Function

Function 用于参数可由 JSON Schema 完整表达的工具。Responses API 下使用扁平形式：

```json
{
  "type": "function",
  "name": "exec_command",
  "description": "Run a command",
  "strict": false,
  "parameters": {
    "type": "object",
    "properties": {
      "cmd": { "type": "string" }
    },
    "required": ["cmd"]
  }
}
```

它解决的是“参数结构与自然语言描述分离”问题。Provider 可在生成阶段约束参数，宿主也可在执行前再做 schema 校验。

### 4.3 Freeform / custom

Freeform 工具不强迫输入进入 JSON object，而是保留原始文本并附带语法：

```json
{
  "type": "custom",
  "name": "apply_patch",
  "description": "Apply a patch to workspace files",
  "format": {
    "type": "grammar",
    "syntax": "lark",
    "definition": "start: patch ..."
  }
}
```

对 `apply_patch` 这类自由文本工具来说，这种设计有三个好处：

- 不需要把多行 patch / JavaScript 包在 JSON 字符串中；
- grammar 在采样边界约束格式，而不是依赖 prompt 约定；
- 历史使用 `custom_tool_call` / `custom_tool_call_output` 成对回放，不会被误当作 Function。

### 4.4 Responses API 原生 Namespace

Namespace 是 Responses API 的原生工具变体，不是在 Function 名称前拼接字符串的宿主约定。它作为 `tools` 数组中的顶层容器，用 `name` 声明稳定的能力域，用 `tools[]` 承载该域内的 Function 或 Custom 子工具。OpenAI 官方示例同时允许在子工具上设置 `defer_loading: true`，再由 `tool_search` 按需发现。

官方参考：[OpenAI Tools - Tool search](https://developers.openai.com/api/docs/guides/tools#tool-search)。

#### 4.4.1 定义和调用的 wire shape

一个 `cron` 工具组的定义形如：

```json
{
  "type": "namespace",
  "name": "cron",
  "description": "Tools in the cron namespace.",
  "tools": [
    { "type": "function", "name": "add", "parameters": {} },
    { "type": "function", "name": "list", "parameters": {} }
  ]
}
```

原生调用事件不应把它压成 `cron.add` 或 `cron__add` 一个字段，而应保留二元组身份：

```json
{
  "type": "function_call",
  "call_id": "call_123",
  "namespace": "cron",
  "name": "add",
  "arguments": "{\"schedule\":\"every:1h\",\"task\":\"sync\"}"
}
```

因此工具的规范身份是 `(namespace, name)`，概念上可写为 `cron.add`，但点号形式不是 Responses 原生事件中的单一 `name` 字段。`call_id` 继续用于将 call 与 output 成对，`namespace` 则必须在原生历史中保留，不能在投影为 UI 或兼容消息时丢失。

#### 4.4.2 Codex 的参考实现

Codex 在内部直接使用结构化工具名：

```rust
pub struct ToolName {
    pub name: String,
    pub namespace: Option<String>,
}
```

其关键规则是：

- 顶层 Function / Custom 的默认 namespace 是 `functions`；`None`、空字符串和 `functions` 在路由时等价。
- `ToolSpec::Namespace(ResponsesApiNamespace)` 原生序列化为 `type: "namespace"`，子工具保持自己的 `name`、schema 和 `defer_loading`。
- Responses Lite 会把普通 Function / Custom 聚合到 `functions` namespace；完整 Responses 路径则可保留原始顶层变体。
- 模型返回 call 后，路由器从独立的 `namespace` 和 `name` 重建 `ToolName`，再与注册表中的规范键匹配。
- Code Mode 需要 JavaScript 可调用标识符时，才把非默认 namespace 临时展平为 `namespace__tool`；这是 Code Mode adapter，不是 Responses 协议的规范身份。

Codex 当前的代表性 namespace 包括：

| Namespace | 用途 | 示例子工具 |
| --- | --- | --- |
| `functions` | 默认顶层 Function / Custom | `exec_command`、`apply_patch` |
| `clock` | 时间与等待 | `curr_time`、`sleep` |
| `collaboration` | Multi-Agent V2，可配置改名 | `spawn_agent`、`send_message`、`wait_agent` |
| `multi_agent_v1` | 旧版 Multi-Agent | `spawn_agent`、`send_input` |
| `mcp__<server>` | MCP Server 工具组 | Server 暴露的原生工具名 |
| `mcp__codex_apps__<connector>` | Codex Apps / Connector | Connector 的业务工具 |

在 Codex 工具 UI 或调用语法中看到的 `functions.collaboration.spawn_agent` 不是三层 Responses namespace：`functions` 是宿主的工具调用通道，真正的 Responses 工具身份是 `namespace = collaboration` 和 `name = spawn_agent`。

Codex 参考源码（本节核对 checkout `a0dcfe2ada`，2026-09-03）：

- `codex-rs/protocol/src/tool_name.rs`：`ToolName` 和默认 `functions` namespace；
- `codex-rs/tools/src/responses_api.rs`：`ResponsesApiNamespace` 及子工具聚合；
- `codex-rs/tools/src/tool_spec.rs`：Responses / Responses Lite 的序列化；
- `codex-rs/core/src/tools/router.rs`：从 call 的 `namespace + name` 重建路由键；
- `codex-rs/tools/src/code_mode.rs`：Code Mode 的 `namespace__tool` 适配。

#### 4.4.3 Namespace 解决的问题

Namespace 不只是“名字更好看”，它同时解决：

1. **同名冲突**：`calendar.list`、`files.list` 和 `agents.list` 可以并存，不必为全局唯一名不断增加前缀。
2. **语义分组**：namespace 自身的 description 先告诉模型能力域，子工具再表达具体动作，降低大型工具集的选择干扰。
3. **延迟发现**：子工具可标记 `defer_loading`，由 `tool_search` 按 namespace 和描述检索，降低首轮 token、请求体积和 Prompt Cache 波动。
4. **所有权与路由**：宿主可以将 `mcp__github.search` 和 `mcp__linear.search` 稳定路由到不同 Server，并在历史、审批、日志和统计中保留来源。
5. **内外命名解耦**：对外使用 `(namespace, child)`，对内 handler 可继续使用已稳定的注册名，协议演进无需重命名整条执行链。

Namespace **不是安全边界**。它表达组织、唯一身份、发现与路由；工具是否可见、可调、需审批或可写仍由 StepContext、ToolRouter、exposure、权限 profile 和执行策略决定。

#### 4.4.4 Astro 映射与当前边界

Astro 的 `ToolEntry.namespace` 为空时表示默认域；非空条目由 `ToolRegistry` 按 namespace 合并为原生 schema。例如内部注册名 `cron_list` 对外投影为 `(cron, list)`。`ToolName` 把 `None`、空字符串和 Responses 默认 namespace `functions` 视为同一默认域。

| 边界 | 表示 |
| --- | --- |
| Responses 原生定义 / 调用 | `namespace: "cron"` + `name: "list"` |
| 文档与人类可读记法 | `cron.list` |
| Astro 内部注册名 | `cron_list` |
| Code Mode JavaScript 标识符 | 由当前 Step 快照临时规范化，不作为 Responses 调用名 |

MCP 同样使用原生身份：模型看到 `namespace: "mcp__calendar"` + `name: "list_events"`；`McpHub` 内部仍用 `mcp__calendar__list_events` 作为唯一执行键。内部键不会出现在模型直调 schema 或 call 的 `name` 字段中。

> **当前实现基线（2026-09-03）**：`ParsedToolCall` 保留分离的 `namespace` 和 `name`，`ToolRouter.routes` / `model_routes` 以结构化 `ToolName` 为键。模型直调、Code Mode 嵌套调用、MCP approval、sandbox 偏好、hook、执行分发、`function_call_output`、SQLite 索引与 UI 投影共用同一身份。顶层伪造 `name: "cron.list"` 或 `name: "cron__list"` 不会命中 `(namespace: "cron", name: "list")` 路由。

当前内置 namespace：

| Namespace | 暴露策略 | 子工具 |
| --- | --- | --- |
| `astro_browser` | 默认 Deferred；可通过全局加载策略切换，仍受本机浏览器可用性和 Plan 模式过滤 | `open`、`snapshot`、`click`、`type`、`scroll`、`wait`、`screenshot`、`tabs`、`tab_open`、`tab_switch`、`tab_close`、`back`、`forward`、`reload`、`downloads`、`close` |
| `media` | Deferred，由 `tool_search` 发现；Skill 可放宽对应 toolset 开关 | `image_gen`、`video_gen`、`speech_gen`、`music_gen` |
| `cron` | Direct | `add`、`list`、`remove`、`enable`、`disable` |
| `workflow` | 按工作流配置，默认 Deferred | 已启用 Workflow 的 Agent 工具名，另有 `get_run` / `cancel_run` |
| `mcp__{server}` | 有 `tool_search` 时 Deferred，否则 Direct | Server 返回的原生工具名 |

`image_analyze`、`audio_analyze`、`video_analyze` 和 `robotics` 不归入 `media`；理解/感知工具与生成工具的副作用、路由和授权语义不同。

#### 4.4.5 Desktop Catalog 投影

后端 `ToolCatalogItem` / `ToolFunctionInfo` 不再把内部注册名冒充为 API 调用名：

| 字段 | 语义 | Browser 示例 |
| --- | --- | --- |
| `id` | toolset 开关 id | `browser` |
| `name` | 模型可见调用名 | `astro_browser.open` |
| `namespace` | Responses namespace | `astro_browser` |
| `registeredName` | Rust Registry / handler 内部名 | `browser_open` |

`functions[]` 同样携带这三种身份。Desktop `ToolsPanel` 使用 `id` 切换 toolset，
用 `name` 展示真实调用名，并分开显示 namespace 与 registered name。搜索同时覆盖三者，
因此 UI 与 Provider schema 对齐，又不会改变现有工具开关的存储键。

浏览器使用应用自有 `astro_browser` 命名空间，避免 Azure/OpenAI Responses 的
`browser` 保留域冲突（例如 `browser.back` 会被拒绝）。子工具名保持 `open`、`back`
等短名，内部执行键与 Plan 模式权限仍使用 `browser_*`，toolset 开关仍为 `browser`。

默认 16 个 Browser 工具均为 Deferred。Desktop 详情页可选择全局工具组加载策略
`auto` / `always` / `on_demand`，保存到 `desktop.tool_loading`。Registry 每次采样前
从内置 exposure 恢复并叠加配置；Auto 不会继承上次 Always 的修改。执行器、内部名、
审批和权限保持不变，已发 Step 的 Registry 快照不变。

### 4.5 ToolSearch

ToolSearch 是一种原生工具类型，不是将工具列表塞进 system prompt 的文本技巧：

```json
{
  "type": "tool_search",
  "execution": "client",
  "description": "Search deferred tools by keyword...",
  "parameters": {
    "type": "object",
    "properties": {
      "query": { "type": "string" },
      "limit": { "type": "integer" }
    }
  }
}
```

`execution: "client"` 表示 Provider 产生搜索调用，但搜索和工具加载由 Astro 执行。这使 Provider 协议、本地工具注册表和 MCP 连接状态保持解耦。

### 4.6 WebSearch

WebSearch 表示由 Provider 托管的搜索：

```json
{
  "type": "web_search",
  "search_context_size": "medium"
}
```

`options` 在序列化时被展开到顶层，以便保留 Provider 新增选项，避免 Astro 每次都需要升级结构体。

需要区分两个同名概念：

- `ToolDefinition::WebSearch` 是 Provider 原生托管协议；
- Astro 当前内置的 `web_search` 是客户端执行的 Deferred Function，由 Brave/Bing 实现。

当前代码已能解析和传输原生 `WebSearch`，但 Registry 尚未注册 Provider-hosted WebSearch 实例。这是有意保留的能力边界：本地搜索可跨 Provider 稳定工作，托管搜索则必须先由具体 Provider profile 声明支持。

### 4.7 非 Agent 兼容调用的工具边界

> Agent primary、fallback 和辅助模型不进入本节路径：它们只使用 Responses 原生工具类型。以下 lowering 仅服务仍显式调用 Chat/Anthropic/Gemini adapter 的工具或独立 Provider 功能。

| 原生类型 | Chat Completions / Anthropic / Gemini 等兼容路径 |
| --- | --- |
| Function | 保持为 Function |
| Freeform | 降为带 `{ input: string }` 的 Function |
| Namespace Function | 省略；不丢弃 namespace 后伪装成全局 Function |
| Namespace Freeform | 省略；不将原生层级降级为字符串前缀 |
| ToolSearch | 降为普通 `tool_search` Function，仍由客户端执行 |
| WebSearch | 不安全伪装，从通用 Function 列表中省略 |

这个降级层的原则是“可无损转换才转换”。例如 Freeform 包成 `input` 虽然不再有 grammar 约束，但仍能保留原始文本；Provider-hosted WebSearch 则没有通用客户端函数能保留其执行主体和引用语义，因此不做伪降级。

Namespace 不再做 Function-only lowering。这会牺牲不支持 namespace 的非 Agent adapter 上的部分工具可用性，但避免把不同协议身份错认为同一全局 Function。Agent primary、fallback 和辅助任务只走 Responses，不受该限制。

## 5. `tool_search` 延迟工具与 MCP 激活

### 5.1 解决的问题

大量内置工具和 MCP 工具全量注入会带来：

- 每轮固定 token 成本；
- 相似工具描述之间的选择干扰；
- MCP Server 连接越多，首轮请求越大；
- 热重载后已加载工具意外恢复为不可见。

Context usage 与延迟激活使用同一 `StepContext` 边界：本 step 采样前的快照只计入当时已可见 schema；`tool_search` 返回并激活 Deferred/MCP 工具后，新 schema 从下一 step 的本地分层估算开始计入。Provider 返回的 usage 只校准已完成 sampling 的 top-line，不倒推修改当时的工具暴露集。

### 5.2 当前实际给模型的工具

工具列表不是全局常量。每次 sampling 前，Registry 还会受 toolset 开关、`check_fn` 环境检测、Skill additive override、已发现 Deferred 集合和当前 MCP 连接影响。因此“实际列表”应以当前 Step 的 `ResponsesRequest.tools` 与可信 `tool_search_output` 为准，而不是把注册表中的所有 handler 等同于模型可见工具。

在 toolset 启用、运行条件满足且尚未进行 Deferred 激活的默认状态下：

| 类别 | 当前 Direct 工具 |
| --- | --- |
| Shell / 文件 | `exec_command`、`apply_patch`、`get_context_remaining`、`new_context_window`、`tool_search` |
| HITL | `ask_user`、`request_user_input_async`、`switch_mode` |
| 上下文 / 记忆 / Skill | `context_search`、`pin_context`、`memory`、`skills`、`todo` |
| 自动化 | `cron.add`、`cron.list`、`cron.remove`、`cron.enable`、`cron.disable`（内部注册名为 `cron_*`） |
| 展示 | `present` |
| Agent Threads | `spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent` |

默认不直接注入、可由 `tool_search` 发现的内置 Deferred 工具是：

- 终端与环境：`write_stdin`、`code_exec`、`request_permissions`、`request_plugin_install`、`wait_for_environment`；
- Web：`web_search`、`web_fetch`；
- Browser：`astro_browser` 下全部 16 个工具（仍需本机浏览器可用）；
- 媒体生成：`media.image_gen`、`media.video_gen`、`media.speech_gen`、`media.music_gen`；
- 媒体理解：`image_analyze`、`audio_analyze`、`video_analyze`、`robotics`；
- MCP：当 `tool_search` 可用时，当前 MCP Hub 发现的 `(mcp__{server}, {tool})` 原生子工具也默认进入 Deferred。
- Workflow：`agent_tool.exposure=deferred` 的已启用工作流以 `workflow.<name>` 进入搜索；不支持 `tool_search` 的模型不会将它们 eager 降级。

`exec` / `wait` 以 `DirectModelOnly` 注册，但只有 CodeMode 或 CodeModeOnly 投影会把它们发给模型。Direct 模式仍直接获得普通 Direct 工具和原生 `tool_search`；Deferred 工具由 `tool_search` 激活后进入下一次 sampling。

`request_user_input_async` 是非阻塞的结构化提问工具：`questions[]` 每项包含
`title` 和可选 `options`，工具立即返回 `{"accepted":true}`，不 park 当前 turn。
Core 把问题写为 durable `AgentMessageItem` 的 `questions` 字段，Desktop 只保留最新且
未被后续 user message 回答的问题组可交互。旧工具名已移除，不再注册或迁移。

### 5.3 暴露状态

`ToolExposure` 将“是否可执行”与“是否直接给模型看”分开：

| 状态 | 首次采样可见 | `tool_search` 可发现 | 内部路由 |
| --- | --- | --- | --- |
| `Direct` | 是 | 否 | 是 |
| `Deferred` | 否 | 是 | 是 |
| `Hidden` | 否 | 否 | 否 |
| `DirectModelOnly` | 是 | 否 | 是，不在用户工具 UI 展示 |
| `DeferredModelOnly` | 否 | 是 | 是，不在用户工具 UI 展示 |

### 5.4 搜索和激活流程

```text
MCP/builtin tool registration
        |
        +-- Direct ----------------------> next model request
        |
        +-- Deferred --> BM25 index
                           |
model emits tool_search_call(query, limit)
                           |
client searches name x2 + toolset + description
                           |
returns complete native definitions (defer_loading=true)
                           |
next Step 从可信 tool_search_output 提取 ToolName
                           |
next sampling step routes only discovered schemas
```

具体语义：

- `query` 必须非空；
- BM25 检索中工具名重复一次，提高精确名称命中权重；
- 检索文本由 `name + name + toolset + description` 组成；
- `limit` 默认 10，执行时最大 50；
- 返回的不是名称列表，而是含完整 `parameters` 的原生可加载 schema；同一 namespace 的命中子工具会合并到一个容器；
- 激活事实保存在 durable `ResponseItem::ToolSearchOutput`，不另建易漂移的 Session 状态；
- 新 Step 从已完成的 `tool_search_output` 重建 `HashSet<ToolName>`，MCP 每轮重注册也不会丢失可路由身份。

### 5.5 MCP 为什么要默认延迟

`Session::attach_mcp_tools()` 会先判断 `tool_search` 是否可用。若可用，当前 MCP Hub 发现的工具以 Deferred 方式注册；若不可用，则回退为 Direct，避免“无搜索入口且工具不可见”的死锁。

### 5.6 MCP 单工具输出预算

`[mcp_servers.<id>.tools.<tool>]` 可声明正整数 `output_token_limit`。预算与
`enabled` / `approval_mode` 独立，并被固化到 Step-scoped `ToolEntry`：

```toml
[mcp_servers.docs.tools.search]
approval_mode = "approve"
output_token_limit = 3000
```

Astro 以 4 bytes/token 加 20% 序列化余量换算文本上限，并与全局 64 KiB 上限取
更严格值。限制在 MCP 原始/结构化/错误输出进入 Core 前执行，且在
`TransformToolResult` / `PostToolUse` 之后再执行一次，防止 hook 扩展内容绕过预算。
Desktop MCP DTO 使用 `toolOutputTokenLimits` 保持该配置的 round-trip。

这里的“激活”只是改变下一次 sampling 的可见性，不是授权。MCP 工具调用时仍会进入：

```text
MCP approval policy -> HITL when required -> ToolRouter -> McpHub -> result
```

因此“搜索到”不等于“获准执行”。

## 6. Responses API 事件与历史回放

### 6.1 为什么必须类型对称

Responses API 使用不同的 call/output item 表示不同工具。下一轮采样时，历史必须恢复原始类型：

| 工具 | call item | output item |
| --- | --- | --- |
| Function | `function_call` | `function_call_output` |
| Freeform（`apply_patch` / `exec`） | `custom_tool_call` | `custom_tool_call_output` |
| ToolSearch | `tool_search_call` | `tool_search_output` |

如果将 `custom_tool_call` 的返回值重放为 `function_call_output`，对 Provider 来说这是一个不匹配的协议对；如果将 `tool_search_output.tools` 压成普通文本，模型无法把搜索结果当作后续可调用的工具定义。

### 6.2 流式接收

`openai/responses.rs` 对三种调用都生成统一的 `StreamChunk`：

1. `response.output_item.added`：生成 `ToolCallStart`，保留 `output_index` 和 `call_id`；
2. Function 的 `response.function_call_arguments.delta`：累积 JSON 参数分片；
3. `response.output_item.done`：
   - Function 读取 `arguments`；
   - custom 读取原始 `input`，再编码为统一的字符串参数；
   - tool search 读取 `arguments` object；
4. `response.completed`：只要 output 中含任一工具调用，统一产生 `finish_reason = "tool_calls"`。

这让上层 `ToolCallAccumulator` 无需为每种 Provider 重写执行循环，但又不丢失 Responses 的原生工具类型。

### 6.3 历史重建

`sanitized_response_items()` 在发送前检查 call/output 边界：

- 丢弃找不到 call 的孤立 output；
- 为没有 output 的中断 call 生成稳定 id 的 `aborted` output，避免重试时反复改变请求；
- 合成 Function output 保留原 call 的 `name` 和 `namespace`；
- `apply_patch` / `exec` 保持 custom pair，`tool_search` 保持 `status` / `execution` / native tools 数组；
- 所有存活项按原始顺序进入下一次 Responses input，不经 Chat message 往返转换。

### 6.4 一个 ToolSearch 往返示例

```jsonc
// Provider -> Astro
{
  "type": "tool_search_call",
  "call_id": "search_1",
  "execution": "client",
  "arguments": { "query": "calendar events", "limit": 5 }
}
```

```jsonc
// Astro -> Provider（下一轮 input）
{
  "type": "tool_search_output",
  "call_id": "search_1",
  "status": "completed",
  "execution": "client",
  "tools": [
    {
      "type": "namespace",
      "name": "mcp__calendar",
      "description": "Tools in the mcp__calendar namespace.",
      "tools": [
        {
          "type": "function",
          "name": "list_events",
          "defer_loading": true,
          "parameters": { "type": "object" }
        }
      ]
    }
  ]
}
```

## 7. Code Mode `exec` / `wait`

> 本节描述已实现的 QuickJS runtime 合同。Provider 根据模型目录与 feature flag 的结果选择 Direct、CodeMode 或 CodeModeOnly 投影。

### 7.1 解决的问题

直接工具模式中，“读文件→解析→再搜索→汇总”需要多次模型采样。每次往返都会带来延迟、token 消耗和中间状态丢失风险。

Code Mode 把确定性编排下沉到一个 JavaScript cell：

```javascript
const result = await tools.some_tool({ query: "..." });
store("result", result);
text(result);
```

模型只需要生成一段程序，宿主负责执行、审批、等待和收集输出。

### 7.2 `exec` 是 Freeform，`wait` 是 Function

`exec` 的输入是原始 JavaScript，因此使用 Lark grammar 限定两种形态：

```text
start: pragma_source | plain_source
pragma_source: PRAGMA_LINE NEWLINE SOURCE
plain_source: SOURCE
```

可选 pragma：

```javascript
// @exec: {"yield_time_ms": 10000, "max_output_tokens": 1000}
const result = await tools.exec_command({ cmd: "git status --short" });
text(result);
```

`wait` 是普通 Function，参数为：

| 字段 | 必填 | 语义 |
| --- | --- | --- |
| `cell_id` | 是 | 要恢复或终止的 cell |
| `yield_time_ms` | 否 | 本次最多等待时间，默认 10,000 ms |
| `max_tokens` | 否 | 本次返回的最大输出预算，默认 10,000 |
| `terminate` | 否 | `true` 时终止 cell |

### 7.3 Cell 运行时

当前实现使用进程内 QuickJS 执行隔离 JavaScript cell。每个 cell 都在专用线程中创建新的 runtime/context，并设置内存、栈和中断限制；不依赖用户电脑安装 Node.js。模型代码不获得宿主全局对象，可用边界只有：

| 能力 | 用途 |
| --- | --- |
| `tools.<normalized_name>(input)` | 调用 Astro 工具 |
| `ALL_TOOLS` | 列出可用嵌套工具的名称和自描述声明；对象形状固定为 `{name, description}` |
| `text(value)` | 追加文本输出 |
| `image(value, detail)` / `audio(value)` | 输出媒体事件 |
| `generatedImage(value)` | 输出生成图片事件 |
| `store(key, value)` / `load(key)` | 在同一 Session 的 cell 之间共享 JSON 可序列化状态 |
| `notify(value)` | 向宿主发送内容事件 |
| `yield_control()` | 交还控制权，等待 `wait` 恢复 |
| `exit()` | 正常结束 cell |
| `setTimeout` / `clearTimeout` | 显式异步等待 |

QuickJS host function 和 Rust 调度器通过类型化异步通道通信：

```text
JavaScript await tools.x(input)
        |
        v
RuntimeEvent::ToolCall {id, name, input}
        |
        v
Rust ToolRouter -> approval/sandbox/hook -> handler
        |
        v
oneshot result: Ok(value) | Err(error)
        |
        v
resolve/reject JavaScript Promise
```

### 7.4 分层工具声明

CodeModeOnly 不把所有业务工具 Schema 注入 Provider 请求。声明按热路径分层披露：

1. 业务工具的精简 TypeScript 声明保存在 `ALL_TOOLS[].description`，模型可在 cell 内筛选后读取。
2. Deferred 工具不进入初始 `exec` 描述，但仍存在于 cell 的 `ALL_TOOLS`。
3. `ALL_TOOLS` 每项只含 `name` 和 `description`；description 末尾携带同形 TypeScript 调用声明，不再提供独立的 `getToolSchema()`。

Function 工具描述示例：

```ts
declare const tools: {
  exec_command(args: {
    command: string;
    cwd?: string;
  }): Promise<unknown>;
};
```

Freeform 工具描述示例：

```ts
declare const tools: {
  apply_patch(input: string): Promise<unknown>;
};
```

延迟工具发现示例：

```javascript
const candidates = ALL_TOOLS.filter(({ name, description }) =>
  /calendar|schedule/i.test(`${name} ${description}`)
);
if (candidates.length === 0) throw new Error("no matching tool");
text(candidates.slice(0, 5)); // description 已含调用声明
```

声明在 Rust 侧与 Step 的嵌套路由一起冻结到 `ToolRouter`，执行 `exec` 时不再读取实时 Registry。Function parameters 先经过 `sanitize_tool_schema()` 清理，再转换为 TypeScript；Freeform 工具使用 `input: string`。`Hidden`、`exec`、`wait` 和 `tool_search` 不进入 cell 目录。`ALL_TOOLS` 数组及条目均冻结，脚本不能篡改后续查询结果。

JavaScript 调用名、QuickJS host 回调名和嵌套路由键使用同一规范化标识符，例如
`astro_browser.snapshot` → `tools.astro_browser_snapshot(...)`，
`mcp__review.read-page` → `tools.mcp__review_read_page(...)`。
该标识符只用于 Code Mode 内部调用；原生 Responses 的 `(namespace, name)` 和
Rust 内部 registered name 不变。规范化产生重名时，Step 构建直接报错，不能按注册顺序
任意选择执行器。重载工具设置不会改变已发 Step 的声明或执行器。

这一设计消除了“先查名称、再调用另一个 Schema API”的重复协议，但不会掩盖延迟披露的成本：模型若事先不知道某个 Deferred 工具，仍需先输出筛选到的 description，再经过一次模型采样生成实际调用。常用 Direct 工具通过预置声明避免这次额外往返。

### 7.5 `yield` / `wait` 生命周期

`exec` 在三种情况下返回：

- 脚本结束：`Script completed`；
- 脚本异常：`Script failed`；
- 显式 `yield_control()` 或达到等待时限：`Script running with cell ID ...`。

第三种情况下，cell 仍由 Session 级 `CodeModeService` 持有。后续 `wait` 可以：

- 发送 `resume` 并继续读取新事件；
- 若上次只是达到等待时限而非显式 `yield_control()`，`resume` 是无害空操作，`wait` 继续轮询原 cell；
- 再次超时后返回同一 cell id；
- 通过 `terminate: true` 设置中断信号并移除 cell；
- 完成或失败时从 `cells` map 中清理。

`store/load` 与 cell 生命周期分离：它们存储在 Session 级 map，新 cell 启动时获取快照，因此可以跨 cell 复用结果。

## 8. 安全、审批、Hooks 和计数

### 8.1 两层安全边界

Code Mode 不是“JavaScript 拥有所有权限”，而是两层边界：

1. **cell 运行时边界**
   - QuickJS 由 Rust 二进制内嵌，不做 Node 或外部运行时探测；
   - 每个 cell 使用独立 runtime/context，并限制内存、栈及可中断执行；
   - 不注册文件系统、网络、进程或模块加载 host API；
   - 不向模型脚本暴露 `require` / `process` / 文件系统 / 网络 API。
2. **嵌套工具边界**
   - JavaScript 只能发出结构化 `tool_call`；
   - Rust Host 依旧经过 `ToolRouter`、交互模式、MCP 审批、浏览器审批、写权限、危险命令分类和 sandbox policy；
   - cell 无法直接绕过工具 handler 访问系统。

这样即使脚本能组合很多操作，权限仍与普通工具调用一致。

### 8.2 调用来源不能混淆

`ToolInvocationSource` 区分两种来源：

- `Model`：只能调用本次 `StepContext` 真正对模型可见的工具；
- `CodeMode`：可调用已纳入快照路由、但不直接向模型暴露的工具。

这个区分防止模型在 `CodeModeOnly` 下伪造普通 tool call 绕过 `exec`，同时允许受信任的 cell 使用延迟或 CodeMode-only 工具。

### 8.3 Hooks

- `exec` 进入 `PreToolUse`，可被 Block 或 Modify；
- `exec` 结果通过 `finalize_tool_call_result()`，因此继续应用 `TransformToolResult` 和 `PostToolUse`；
- 每个嵌套工具回到普通执行器，保留其自身 hook 语义；
- `wait` 是纯 cell 控制操作，不重复执行 `exec` 的变换 hook。

### 8.4 工具轮次与可观测性

- `exec` / `wait` 作为模型发起的控制工具，显式消耗当前用户回合的 tool-round budget；
- 调用会写入 `home::record_tool_call` 和 usage 数据；
- 嵌套调用经过同一路由和审计链；
- 取消会终止 cell，不把孤儿进程留在会话外。

## 9. Direct / CodeMode / CodeModeOnly 模式合同

### 9.1 Provider 可见面

| 模式 | Provider `tools` 中直接可见 | 业务工具发现 | 调用路径 |
| --- | --- | --- | --- |
| `Direct` | Direct Function / Freeform / Namespace + `tool_search`；每个工具带完整 Schema | Deferred 经 Provider 原生 `tool_search` 按需返回 | 模型直接产生原生 tool call |
| `CodeMode` | Direct 工具、`tool_search`、`exec`、`wait` | Deferred 可经 `tool_search` 直调，也可由 QuickJS 间接调用 | 模型可混合使用原生 tool call 与 JavaScript |
| `CodeModeOnly` | 仅 `exec` / `wait` | 业务工具从 `ALL_TOOLS[].description` 检索声明 | 模型产生 JavaScript，cell 通过 `tools.<name>(input)` 嵌套调用 |

`ToolExposure` 与模式正交：它描述工具条目在注册表中的披露策略，模式决定该策略如何投影到本次 Provider step。两者都不是授权；执行仍必须经过工具 gate、approval 和 sandbox。

### 9.2 模式决策与快照

- 模式按“模型目录 `tool_mode` > `[features]` > `Direct`”解析，不允许模型输出自行切换。
- `[features] code_mode_only = true` 优先于 `code_mode = true`；模型目录显式值仍可覆盖两者。
- CodeMode 控制工具不可用时可降级到 Direct；CodeModeOnly 不降级并终止当前 step。
- 每个 sampling step 冻结一份 `StepContext` 工具快照，避免 MCP 热重载或 Skill 加载改变已生成调用的路由边界。
- `CodeModeOnly` 必须 fail closed：QuickJS runtime、`exec`/`wait` 规格或快照构建失败时终止当次 step，不得悄然退回 Direct 并扩大模型可见面。
- `Direct` 不需要 QuickJS，Code Mode runtime 故障不影响 Direct 模型。

### 9.3 请求与执行不变量

1. Direct 模型调用必须命中 `model_visible_specs`。
2. CodeModeOnly 模型不得伪造业务工具的顶层 call；顶层只接受 `exec` / `wait`。
3. cell 嵌套调用只能命中该 step 冻结的可路由工具快照，不允许按名称直接穿透全局 Registry。
4. `ALL_TOOLS` 与 `tools` 必须来自同一份快照，避免“看见 A 声明、却调用 B handler”。
5. 无论哪种模式，tool result 都使用同一持久化、spill、Hook 和审计链。

### 9.4 当前接线状态

| 能力 | 状态 |
| --- | --- |
| Direct Function / Freeform / Namespace schema | 已接入 |
| Responses 分离 `namespace + name` 的调用键规范化 | 已接入 |
| Direct `tool_search` + Deferred 路由 | 已接入 |
| QuickJS cell、`exec/wait` 调度、自描述 `ALL_TOOLS` | 已接入 |
| 模型目录 `tool_mode` 覆盖 | 已接入 |
| `[features]` 默认模式选择 | 已接入 |
| Provider 三模式工具投影 | 已接入 |

配置示例：

```toml
[features]
code_mode = true
# code_mode_only = true
```

模型目录中的对象可设置 `"tool_mode": "direct" | "code_mode" | "code_mode_only"`；该值优先于上述 feature flag。

## 10. 关键不变量

1. 工具 schema 通过 `ResponsesRequest.tools` 传输，不拼入 system prompt。
2. Responses 原生类型在请求、SSE 事件、执行和历史回放之间保持对称。
3. Deferred 仅表示可见性，不表示授权。
4. `tool_search` 激活在下一次 sampling step 生效，MCP 热重载不丢失状态。
5. 模型直调只能命中当前 Step 真正可见的路由。
6. 历史丢弃孤立 output，并为中断 call 合成稳定的 `aborted` output；合成项不得丢失 namespace。
7. CodeModeOnly 的 `ALL_TOOLS` 对象形状固定为 `{name, description}`；description 是包含 TypeScript 调用声明的自描述合同。
8. Namespace 工具的规范身份是 `(namespace, name)`；点号只用于人类可读投影，Code Mode 标识符只用于 cell 内部，二者都不得作为模型顶层直调别名。
9. Namespace 不扩大权限；调用仍必须同时命中当前 StepContext 的模型可见路由与执行策略。
10. Provider 工具定义解析、Runtime 绑定、规范身份冲突和 client/server 执行权必须 fail closed。

## 11. 实现映射

| 职责 | 当前实现 |
| --- | --- |
| 类型擦除工具运行时 | `crates/agent-tools/src/engine/executor.rs::CoreToolRuntime` / `ToolExecutor` |
| Provider 工具联合类型 | `crates/agent-providers/src/types/request_content.rs` |
| 规范工具身份 | `crates/agent-types/src/tool_entry.rs::ToolName` |
| Registry runtime/schema 注册与执行 | `crates/agent-tools/src/engine/registry.rs::ToolRegistry` |
| `tool_search` 搜索与激活 | `crates/agent-tools/src/builtin/shell/tool_search.rs` |
| MCP namespace / 内部执行键 | `crates/agent-mcp/src/names.rs`、`hub.rs`、`crates/agent-core/src/runtime/mod.rs::attach_mcp_tools` |
| Step 工具计划构建 | `crates/agent-core/src/runtime/tool_router.rs::build_tool_router` / `finalize_tool_router` |
| Step 级 Registry / schema 快照 | `crates/agent-core/src/runtime/tool_router.rs::ToolRouter` / `step_context.rs` |
| `Prompt.tools` 构建与 Provider 投影 | `crates/agent-core/src/streaming/provider.rs::build_prompt` / `Prompt` |
| Responses 请求 / SSE / 历史 | `crates/agent-providers/src/openai/responses.rs` |
| ResponseItem 到可执行调用 | `crates/agent-core/src/runtime/tool_router.rs::ToolRouter::build_tool_call` |
| Registry 执行分发 | `ToolRouter::dispatch` → `ToolRegistry::dispatch` → `CoreToolRuntime::handle` |
| call/output 持久化与 namespace 投影 | `crates/agent-core/src/runtime/recording.rs`、`crates/agent-protocol/src/response_item.rs` |
| Desktop 工具目录 DTO 与投影 | `crates/agent-tools/src/engine/catalog.rs`、`apps/desktop/src/lib/tools/agentToolCatalog.ts`、`ToolsPanel.tsx` |
| QuickJS cell 与 `ALL_TOOLS` | `crates/agent-core/src/runtime/code_mode.rs` |
| JSON Schema 到 TypeScript 声明 | `crates/agent-tools/src/engine/code_mode.rs` |
| Code Mode 嵌套工具快照与调度 | `crates/agent-core/src/streaming/tools_exec.rs` |

## 12. 验证与回归

直接覆盖本设计的测试：

- `crates/agent-tools/tests/tool_search_alignment.rs`
  - 原生 schema 形态；
  - Deferred 搜索和激活；
  - MCP namespace 搜索结果与下一 Step 路由。
- `crates/agent-core/src/runtime/tool_router.rs` / `runtime/mod.rs` 单元测试
  - 分离 `namespace + name` 可执行；
  - `namespace.child` / `namespace__child` 无法伪装模型直调；
  - 无 Runtime 的可见 metadata 与重复规范身份在构建阶段 fail closed；
  - 无效 Function JSON 不进入 handler，server-executed Tool Search 不被本地重复执行；
  - output 历史保留 namespace。
- `crates/agent-core/tests/streaming_test.rs`
  - `namespaced_tool_round_trips_from_prompt_to_runtime_and_history` 覆盖 Registry 注册、`Prompt.tools`、Responses call、Runtime 执行和 namespaced output 历史的端到端闭环。
- `crates/agent-providers/src/dispatch.rs` 单元测试
  - 任一无效工具定义都终止请求，不使用 `filter_map` 静默丢弃。
- `crates/agent-tools/tests/code_mode_alignment.rs`
  - Direct 工具和原生 `tool_search` 保持可见；
  - `exec` / `wait` 不进入模型 schema。
- `crates/agent-core/src/runtime/code_mode.rs` 单元测试
  - QuickJS 嵌套工具调用与 `yield/wait` 恢复；
  - `ALL_TOOLS` 保持 `{name, description}` 形状，description 携带 TypeScript 调用声明；
  - 定时器、宿主全局隐藏和忙循环中断。
- `crates/agent-providers/src/openai/responses.rs` 单元测试
  - custom call/output 回放；
  - `exec` custom 回放；
  - ToolSearch call/output 回放；
  - custom/tool_search SSE 事件解析。

建议回归命令：

```bash
CARGO_TARGET_DIR=/tmp/astro-tool-align cargo check -p agent
CARGO_TARGET_DIR=/tmp/astro-tool-align cargo test -p types --lib
CARGO_TARGET_DIR=/tmp/astro-tool-align cargo test -p providers --lib
CARGO_TARGET_DIR=/tmp/astro-tool-align cargo test -p tools --test tool_search_alignment --test code_mode_alignment
```

## 13. 当前边界与后续演进

以下内容必须与“已实现”能力区分：

1. `ToolDefinition::WebSearch` 已具备原生传输能力，但当前没有 Provider profile 将它注册到模型工具列表；当前实际搜索走本地 Deferred Function。
2. CodeModeOnly 的业务工具只存在于嵌套路由，不进入模型直调集合；这两个集合必须继续分别维护。

上述两项不影响当前的原生 Namespace schema、ToolSearch、Responses 回放和 Code Mode runtime 合同。
