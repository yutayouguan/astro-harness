# Codex 原生工具协议与 Tool Search 详细设计

> **Harness 当前基线（2026-09-02）**：模型直接获得内置工具和原生 `tool_search`。Astro 不再注册 `exec` / `wait`，也不再按模型选择 `CodeMode` / `CodeModeOnly`。本文 Code Mode 章节仅保留为历史设计记录，不代表当前运行时能力。

> 阶段：详细设计
>
> 状态：已实现
>
> 基准：Codex `e24190caa9ee355044a7d70177d48a556d766d35`（2026-08-26）
>
> 范围：Provider 工具协议、延迟工具发现、Responses 事件回放，以及历史 Code Mode 设计

## 1. 文档目标

本文描述 Astro 已落地的 Codex 工具协议对齐，重点回答三个问题：

1. 这些机制解决了什么实际问题；
2. 为什么不再把所有工具都压成普通 JSON Function；
3. 从工具注册、Provider 请求、流式事件、执行到历史回放，完整语义如何保持。

这不是计划稿。文中的类型、字段、分支与降级规则均对应当前仓库实现。

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
ToolEntry / MCP discovered tool
          |
          v
ToolRegistry -- exposure + namespace + grammar --> native JSON schemas
          |                                           |
          |                                           v
          |                              dispatch::parse_tool_definition
          |                                           |
          |                                           v
          |                              ResponsesRequest.tools
          |                                           |
          |                         +-----------------+------------------+
          |                         |                                    |
          |                         v                                    v
          |                  Responses API                     legacy providers
          |             preserve native variants          lower to JSON functions
          |                         |
          v                         v
StepContext / ToolRouter <- streamed call events + accumulated arguments
          |
          v
approval -> sandbox -> hooks -> concrete handler -> persisted tool result
          |
          v
next request: reconstruct the matching native call/output history pair
```

核心原则是：工具定义、工具调用事件、工具结果和历史回放必须使用同一种语义。只对请求 schema 做“原生化”而不改响应解析和历史，是不完整的。

## 4. Provider 无关的原生工具协议

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

### 4.4 Namespace

Namespace 使一组工具共享稳定的域名，例如：

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

模型看到的调用名是 `cron.add`，而 Astro 内部可继续使用 `cron_add`。`ToolRouter` 在 Step 快照中完成 wire name 到 registered name 的映射，避免为了协议形态强制重命名底层 handler。

Namespace 解决的不只是“名字更好看”，还包括：

- 降低同名工具冲突；
- 让 Provider / 模型看到结构化的能力边界；
- 可以在不改变内部注册名的情况下演进外部协议。

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

### 4.7 非 Agent 兼容调用的工具降级

> Agent primary、fallback 和辅助模型不进入本节路径：它们只使用 Responses 原生工具类型。以下 lowering 仅服务仍显式调用 Chat/Anthropic/Gemini adapter 的工具或独立 Provider 功能。

| 原生类型 | Chat Completions / Anthropic / Gemini 等兼容路径 |
| --- | --- |
| Function | 保持为 Function |
| Freeform | 降为带 `{ input: string }` 的 Function |
| Namespace Function | 编码为 `namespace__child` Function；ToolRouter 回映射到原 registered handler |
| Namespace Freeform | 编码为 `namespace__child` + `{ input: string }`；ToolRouter 使用相同回映射 |
| ToolSearch | 降为普通 `tool_search` Function，仍由客户端执行 |
| WebSearch | 不安全伪装，从通用 Function 列表中省略 |

这个降级层的原则是“可无损转换才转换”。例如 Freeform 包成 `input` 虽然不再有 grammar 约束，但仍能保留原始文本；Provider-hosted WebSearch 则没有通用客户端函数能保留其执行主体和引用语义，因此不做伪降级。

Namespace 的两种 wire name 属于明确的 Provider 边界：Responses API 原生路径继续使用 `namespace.child` 表达层级；Chat Completions 等 Function-only 路径使用 `namespace__child`，以满足常见的 `^[a-zA-Z0-9_-]+$` 名称约束。step-scoped ToolRouter 只为本轮已暴露的 Namespace 子工具登记这两个别名，并都指向同一 registered handler，因此既不通过全局字符清洗制造碰撞，也不扩大模型可调用的工具集合。

## 5. `tool_search` 延迟工具与 MCP 激活

### 5.1 解决的问题

大量内置工具和 MCP 工具全量注入会带来：

- 每轮固定 token 成本；
- 相似工具描述之间的选择干扰；
- MCP Server 连接越多，首轮请求越大；
- 热重载后已加载工具意外恢复为不可见。

Context usage 与延迟激活使用同一 `StepContext` 边界：本 step 采样前的快照只计入当时已可见 schema；`tool_search` 返回并激活 Deferred/MCP 工具后，新 schema 从下一 step 的本地分层估算开始计入。Provider 返回的 usage 只校准已完成 sampling 的 top-line，不倒推修改当时的工具暴露集。

### 5.2 当前实际给模型的工具

工具列表不是全局常量。每次 sampling 前，Registry 还会受 toolset 开关、`check_fn` 环境检测、Skill additive override、已激活 Deferred 集合、当前 MCP 连接和 `ToolMode` 影响。因此“实际列表”应以当前 Step 的 `ResponsesRequest.tools` 为准，而不是把注册表中的所有 handler 等同于模型可见工具。

在 toolset 启用、运行条件满足且尚未进行 Deferred 激活的默认状态下：

| 类别 | 当前 Direct 工具 |
| --- | --- |
| Shell / 文件 | `terminal`、`exec_command`、`apply_patch`、`get_context_remaining`、`new_context_window`、`tool_search` |
| Browser（本机有可用浏览器时） | `browser_open`、`browser_snapshot`、`browser_click`、`browser_type`、`browser_scroll`、`browser_wait`、`browser_screenshot`、`browser_close` |
| HITL | `ask_user`、`send_user_message_async`、`switch_mode` |
| 上下文 / 记忆 / Skill | `context_search`、`pin_context`、`memory`、`skills`、`todo` |
| 自动化 | `cron.add`、`cron.list`、`cron.remove`、`cron.enable`、`cron.disable`（内部注册名为 `cron_*`） |
| 展示 | `present` |
| Agent Threads | `spawn_agent`、`list_agents`、`send_message`、`followup_task`、`wait_agent`、`interrupt_agent` |

默认不直接注入、可由 `tool_search` 发现的内置 Deferred 工具是：

- 终端与环境：`write_stdin`、`code_exec`、`request_permissions`、`request_plugin_install`、`wait_for_environment`；
- Web：`web_search`、`web_fetch`；
- 媒体：`image_gen`、`image_analyze`、`audio_analyze`、`video_gen`、`video_analyze`、`speech_gen`、`music_gen`、`robotics`；
- MCP：当 `tool_search` 可用时，当前 MCP Hub 发现的 `mcp__{server}__{tool}` 也默认进入 Deferred。

`exec` / `wait` 已从注册表移除。模型始终直接获得当前启用的 Direct 工具和原生 `tool_search`；Deferred 工具由 `tool_search` 激活后进入下一次 sampling。

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
returns complete loadable definitions (defer_loading=true)
                           |
ToolRegistry.activate_deferred(name)
                           |
next sampling step includes the activated schemas
```

具体语义：

- `query` 必须非空；
- BM25 检索中工具名重复一次，提高精确名称命中权重；
- 检索文本由 `name + name + toolset + description` 组成；
- `limit` 默认 10，执行时最大 50；
- 返回的不是名称列表，而是含完整 `parameters` 的可加载 schema；
- 激活结果保存在 Session Registry 的 `activated_deferred` 集合中；
- MCP 每轮卸载并重注册时，会根据该集合恢复已激活状态。

### 5.5 MCP 为什么要默认延迟

`Session::attach_mcp_tools()` 会先判断 `tool_search` 是否可用。若可用，当前 MCP Hub 发现的工具以 Deferred 方式注册；若不可用，则回退为 Direct，避免“无搜索入口且工具不可见”的死锁。

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

`to_responses_input()` 先收集已有结果的 `call_id`，再重建调用和输出：

- 只有存在对应 Tool Result 的 Assistant ToolCall 才会回放；
- `apply_patch` / `exec` 恢复为 custom pair；
- `tool_search` 结果解析为 tools 数组，恢复 `status: completed` 和 `execution: client`；
- 其余工具恢复为 function pair；
- 没有结果的孤立调用不进入下一次请求，避免生成无效协议历史。

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
      "type": "function",
      "name": "mcp__calendar__list_events",
      "defer_loading": true,
      "parameters": { "type": "object" }
    }
  ]
}
```

## 7. 历史设计：Code Mode `exec` / `wait`

> 当前不再注册或暴露这两个工具；以下内容仅记录原实现方案。

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

保留的兼容实现使用进程内 QuickJS 执行隔离 JavaScript cell。每个 cell 都在专用线程中创建新的 runtime/context，并设置内存、栈和中断限制；不依赖用户电脑安装 Node.js。模型代码不获得宿主全局对象，可用边界只有：

| 能力 | 用途 |
| --- | --- |
| `tools.<normalized_name>(input)` | 调用 Astro 工具 |
| `ALL_TOOLS` | 列出可用嵌套工具的名称和描述 |
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

### 7.4 `yield` / `wait` 生命周期

`exec` 在三种情况下返回：

- 脚本结束：`Script completed`；
- 脚本异常：`Script failed`；
- 显式 `yield_control()` 或达到等待时限：`Script running with cell ID ...`。

第三种情况下，cell 仍由 Session 级 `CodeModeService` 持有。后续 `wait` 可以：

- 发送 `resume` 并继续读取新事件；
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

## 9. 当前 Direct 策略

### 9.1 语义

Astro 始终使用 Direct 工具集：

- 常用内置工具以原生 Function / Freeform / Namespace schema 直接暴露；
- `tool_search` 以 Responses 原生 ToolSearch schema 暴露；
- Deferred 工具经 `tool_search` 发现后在后续 sampling step 可见；
- `exec` / `wait` 不注册、不发给模型。

### 9.2 模型选择

工具暴露不再跟随 Codex 模型目录的 `tool_mode`；GPT-5.6 与其他模型都使用同一 Direct 路径。

### 9.3 兼容边界

底层历史 Code Mode 运行时暂保留以便平滑清理，但不存在模型选择入口，工具注册表也不再包含 `exec` / `wait`。

## 10. 关键不变量

1. 工具 schema 通过 `ResponsesRequest.tools` 传输，不拼入 system prompt。
2. Responses 原生类型在请求、SSE 事件、执行和历史回放之间保持对称。
3. Deferred 仅表示可见性，不表示授权。
4. `tool_search` 激活在下一次 sampling step 生效，MCP 热重载不丢失状态。
5. 模型直调只能命中当前 Step 真正可见的路由。
6. 历史只回放已有匹配结果的 tool call，避免孤立调用破坏 Provider 请求。

## 11. 实现映射

| 职责 | 当前实现 |
| --- | --- |
| Provider 工具联合类型 | `crates/agent-providers/src/types/request_content.rs` |
| Registry schema 生成与暴露策略 | `crates/agent-tools/src/engine/registry.rs` |
| `tool_search` 搜索与激活 | `crates/agent-tools/src/builtin/shell/tool_search.rs` |
| MCP 动态注册 | `crates/agent-core/src/runtime/mod.rs::attach_mcp_tools` |
| Responses 请求 / SSE / 历史 | `crates/agent-providers/src/openai/responses.rs` |
| Step 级可见/可路由快照 | `crates/agent-core/src/runtime/step_context.rs` / `tool_router.rs` |

## 12. 验证与回归

直接覆盖本设计的测试：

- `crates/agent-tools/tests/tool_search_alignment.rs`
  - 原生 schema 形态；
  - Deferred 搜索和激活；
  - MCP 重注册后保留激活状态。
- `crates/agent-tools/tests/code_mode_alignment.rs`
  - Direct 工具和原生 `tool_search` 保持可见；
  - `exec` / `wait` 不进入模型 schema。
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
2. 历史 Code Mode 核心代码尚待后续删除，但已从模型配置和工具注册路径下线。

这些边界不影响当前的原生 schema、ToolSearch 和 Responses 回放。
