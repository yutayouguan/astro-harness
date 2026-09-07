# Chat Completions API 与 Responses API：接口、差异与 Agent 架构选择

> 阶段：系统设计 / 接口设计
>
> 状态：基于当前实现的技术说明
>
> 更新：2026-09-01
>
> 适用范围：OpenAI API 选型、Astro Provider 接入、Agent 运行时设计

## 1. 结论先行

Chat Completions API 和 Responses API 都能完成文本生成、多模态输入、结构化输出和函数调用，但二者的抽象层级不同：

- **Chat Completions** 以 `messages[]` 为中心，本质上是“给定一段聊天记录，生成下一条 assistant message”。它成熟、简单、兼容面广，适合普通聊天、一次性生成和需要适配大量 OpenAI-compatible 服务的场景。
- **Responses** 以带类型的 `Item` 和事件为中心，本质上是“一次可包含推理、工具调用、工具结果和最终消息的模型执行”。它提供更完整的 Agent 原语，适合多轮工具循环、推理状态延续、原生工具、持久会话和可恢复执行。

因此，**Responses 更适合充当 Agent 的模型执行协议；Chat Completions 更适合作为通用生成接口和兼容协议**。这也是 Astro 当前的架构选择：生产 Agent 链路只接受 Responses，Chat Completions 留在非 Agent、工具、媒体和兼容调用边界。

这里的 “Chat 接口” 特指 `POST /v1/chat/completions`，不是更早的 Legacy Completions `POST /v1/completions`。

## 2. 两类 API 的共同基础

二者并不是彼此无关的两套产品。Responses 是 Chat Completions 之后面向 Agent 工作负载演进出的统一接口，两者共享很多基础能力：

1. 都通过 `model` 选择模型；
2. 都能接收角色化文本上下文；
3. 都支持流式输出；
4. 都能声明函数工具、设置 `tool_choice`，并允许并行工具调用；
5. 都能返回 token usage、错误和完成状态；
6. 都可以配合 Structured Outputs；
7. 都需要应用侧负责权限、业务工具执行、重试、审计和最终副作用。

两者最大的分歧不是“能不能调用工具”，而是**工具、推理和消息在协议里是一条消息的附属字段，还是彼此独立、可持久化和可回放的类型化对象**。

## 3. Chat Completions API 详解

### 3.1 请求模型

基础端点：

```text
POST /v1/chat/completions
```

典型请求如下：

```json
{
  "model": "<model>",
  "messages": [
    { "role": "system", "content": "You are a helpful assistant." },
    { "role": "user", "content": "查询上海天气" }
  ],
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "get_weather",
        "description": "查询指定城市天气",
        "parameters": {
          "type": "object",
          "properties": {
            "city": { "type": "string" }
          },
          "required": ["city"],
          "additionalProperties": false
        },
        "strict": true
      }
    }
  ],
  "tool_choice": "auto",
  "stream": true
}
```

核心字段可以按四组理解：

| 分组 | 代表字段 | 作用 |
| --- | --- | --- |
| 模型与采样 | `model`、`temperature`、token 上限等 | 控制模型和生成行为 |
| 对话上下文 | `messages` | 承载 system/developer/user/assistant/tool 消息 |
| 工具控制 | `tools`、`tool_choice`、`parallel_tool_calls` | 声明应用函数并控制调用策略 |
| 输出与传输 | `response_format`、`stream`、`stream_options` | 控制结构化输出、SSE 和 usage |

Chat Completions 没有独立的顶层 `instructions`。在 Astro 的非 Agent 兼容实现中，`ChatCompletionRequest.instructions` 会先降级为 system message，再与 `input` 合成 `messages[]`。

### 3.2 响应模型

非流式响应的中心是 `choices[]`：

```json
{
  "id": "chatcmpl_...",
  "object": "chat.completion",
  "choices": [
    {
      "index": 0,
      "message": {
        "role": "assistant",
        "content": null,
        "tool_calls": [
          {
            "id": "call_123",
            "type": "function",
            "function": {
              "name": "get_weather",
              "arguments": "{\"city\":\"上海\"}"
            }
          }
        ]
      },
      "finish_reason": "tool_calls"
    }
  ],
  "usage": {
    "prompt_tokens": 100,
    "completion_tokens": 20,
    "total_tokens": 120
  }
}
```

应用执行工具后，需要把调用和结果继续放回消息历史：

```json
[
  {
    "role": "assistant",
    "content": null,
    "tool_calls": [
      {
        "id": "call_123",
        "type": "function",
        "function": {
          "name": "get_weather",
          "arguments": "{\"city\":\"上海\"}"
        }
      }
    ]
  },
  {
    "role": "tool",
    "tool_call_id": "call_123",
    "content": "{\"temperature\":25,\"condition\":\"晴\"}"
  }
]
```

这里的模型是“消息中心”的：文本、拒绝、reasoning 兼容字段和 `tool_calls` 通常附着在 assistant message 上，工具结果则表示为 `role: "tool"` 的 message。

### 3.3 流式协议

开启 `stream: true` 后，Chat Completions 返回 SSE chunk。消费端通常读取：

- `choices[0].delta.content`：文本增量；
- `choices[0].delta.tool_calls[]`：工具名、调用 ID 和参数增量；
- `choices[0].finish_reason`：`stop`、`tool_calls`、`length` 等终态；
- `usage`：通常需要配合 `stream_options.include_usage`。

它的优点是消费逻辑直观；代价是不同类型的输出集中在 `choice.delta` 内，供应商还可能加入 `reasoning_content`、`reasoning_details` 等非标准扩展，兼容层必须做额外归一化。

### 3.4 多轮与状态

Chat Completions 的经典做法是由应用保存完整 transcript，并在每次请求中重发 `messages[]`。这带来很强的本地控制能力，但上下文裁剪、工具配对、分支、恢复和 token 成本管理都由应用负责。

它非常适合：

- 简单聊天和一次性文本生成；
- 只使用少量函数工具的轻量应用；
- 需要同大量 OpenAI-compatible 网关或本地模型兼容；
- 已经围绕 message transcript 建立成熟基础设施的系统。

## 4. Responses API 详解

### 4.1 请求模型

基础端点：

```text
POST /v1/responses
```

典型请求如下：

```json
{
  "model": "<model>",
  "instructions": "You are a helpful assistant.",
  "input": [
    {
      "role": "user",
      "content": [
        { "type": "input_text", "text": "查询上海天气" }
      ]
    }
  ],
  "tools": [
    {
      "type": "function",
      "name": "get_weather",
      "description": "查询指定城市天气",
      "parameters": {
        "type": "object",
        "properties": {
          "city": { "type": "string" }
        },
        "required": ["city"],
        "additionalProperties": false
      },
      "strict": true
    }
  ],
  "tool_choice": "auto",
  "parallel_tool_calls": true,
  "reasoning": {
    "effort": "high",
    "summary": "auto"
  },
  "store": false,
  "stream": true
}
```

Responses 的输入既可以是简单字符串，也可以是 `input` Item 数组。稳定行为说明可独立放在顶层 `instructions`，而消息、推理、工具调用和工具结果作为不同 Item 进入 `input`。

### 4.2 Item：Responses 的核心抽象

Responses 不把一次输出限制成一条 assistant message，而是返回 `output[]`。输入和输出共享一组可回放的 typed Item；一次模型输出中常见的类型包括：

- `message`：assistant 输出消息；
- `reasoning`：推理相关条目或摘要；
- `function_call`：JSON Schema 函数调用；
- `custom_tool_call`：自由格式工具调用；
- `tool_search_call`：延迟工具发现；
- `web_search_call`、`image_generation_call` 等平台原生工具条目。

应用随后把 `function_call_output`、`custom_tool_call_output` 或 `tool_search_output` 等结果 Item 追加到下一次 `input`。因此，它们属于统一 Item 历史的一部分，但通常不是模型首次响应的 `output`。

一个响应可以同时包含 reasoning、一个或多个工具调用以及消息输出。调用和结果通过 `call_id` 配对，而不是靠数组位置、工具名或文本猜测。

示意响应：

```json
{
  "id": "resp_...",
  "object": "response",
  "status": "completed",
  "output": [
    {
      "id": "rs_...",
      "type": "reasoning",
      "summary": []
    },
    {
      "id": "fc_...",
      "type": "function_call",
      "call_id": "call_123",
      "name": "get_weather",
      "arguments": "{\"city\":\"上海\"}"
    }
  ],
  "usage": {
    "input_tokens": 100,
    "output_tokens": 20,
    "total_tokens": 120
  }
}
```

应用执行函数后，可以在下一次请求中追加：

```json
{
  "type": "function_call_output",
  "call_id": "call_123",
  "output": "{\"temperature\":25,\"condition\":\"晴\"}"
}
```

对于 reasoning 模型，不能只保留最终文本和函数调用；手动管理上下文时，应把响应中的 reasoning 等相关 Item 一并带回后续请求。

### 4.3 流式协议

Responses 的 SSE 是带类型的事件流，而不是统一的 `choices[].delta`。常见事件包括：

- `response.created`；
- `response.output_item.added`；
- `response.output_text.delta`；
- `response.reasoning_summary_text.delta`；
- `response.function_call_arguments.delta`；
- `response.output_item.done`；
- `response.completed`；
- `response.incomplete`、`response.failed`、`error`。

这种协议需要更复杂的状态机，但它能明确表达“哪个 Item 在哪个阶段发生了什么”，更适合进度 UI、工具审计、断点恢复和事件溯源。

### 4.4 三种对话状态策略

Responses 提供三种常见模式：

1. **应用自行保存和重放 Item**：控制力最强，适合本地优先、可审计和需要自定义压缩的系统；
2. **`previous_response_id`**：由服务端衔接上一响应，调用简单，但顶层 `instructions` 仍应按需要重新发送；
3. **Conversations API**：把长期会话交给服务端持久对象管理。

需要注意：`previous_response_id` 简化的是状态传递，不代表历史输入 token 免费；官方文档说明链中的历史输入仍按 input token 计费。

Responses 默认可存储响应。对本地优先、Zero Data Retention 或严格合规场景，应显式评估 `store: false`，并在需要延续 reasoning 时保存和重放官方返回的加密 reasoning Item。

### 4.5 原生工具与统一输出

Responses 除了自定义函数，还可直接接入 OpenAI 托管的 web search、file search、computer use、code interpreter、image generation 和 remote MCP 等工具。托管工具可以在一次 response 执行中继续推进；自定义函数仍需要应用执行并回传结果。

Structured Outputs 在 Responses 中使用 `text.format`，而不是 Chat Completions 的 `response_format`。只读取最终文本时可使用 SDK 的 `output_text` helper；涉及工具、推理或多模态输出时，应遍历 `output[]` 并按 `type` 分派。

## 5. 逐项对比

| 维度 | Chat Completions | Responses |
| --- | --- | --- |
| 核心抽象 | `messages[]` 与 assistant message | `input[]` / `output[]` typed Items |
| 端点 | `/v1/chat/completions` | `/v1/responses` |
| 顶层指令 | 通常放入 system/developer message | 独立 `instructions` |
| 普通输出 | `choices[i].message` | `output[]` 中的 `message` Item |
| 多候选 | 支持 `n`/多个 `choices` 的传统模型 | 单次只产生一条 generation；多候选需多请求 |
| 函数定义 | `tools[].function.{name,...}` | `tools[].{type,name,...}` |
| 函数 strict 默认 | 未指定时默认非 strict | 未指定时会尝试 strict 规范化，不兼容时退回 best effort |
| 函数调用 | assistant message 的 `tool_calls[]` | 独立 `function_call` Item |
| 工具结果 | `role: "tool"` + `tool_call_id` | `function_call_output` + `call_id` |
| 推理表示 | message 扩展字段，兼容实现差异大 | 独立 reasoning Item 与 summary 事件 |
| 流式输出 | `choices[].delta` | 细粒度 typed SSE events |
| 会话状态 | 应用重发 `messages[]` | 手动 Item 重放、`previous_response_id` 或 Conversations |
| 平台原生工具 | 主要由应用自行集成 | 原生支持多类托管工具与 remote MCP |
| Structured Outputs | `response_format` | `text.format` |
| 多模态 | 以 message content parts 表达 | 以 typed input/output content 表达 |
| 状态存储 | 官方当前说明为新账户默认存储；可设 `store: false` | 默认存储；可设 `store: false` |
| 协议复杂度 | 较低 | 较高，但信息保真度和可观测性更强 |
| 兼容生态 | 最广 | 快速扩展中，但第三方实现完整度不一 |
| 典型定位 | 聊天和通用生成兼容层 | Agent 执行与新能力主接口 |

### 5.1 不是简单字段改名

从 Chat Completions 迁移到 Responses 至少包含四类变化：

1. 请求从 `/chat/completions` 改到 `/responses`；
2. 上下文从 message transcript 改为输入 Item 或服务端状态引用；
3. 消费端从 `choices[].delta` 改为按 event type 处理；
4. 持久化层必须保留 reasoning、call、output 等 Item，不能只存最终文本。

如果只替换 endpoint，然后继续把所有对象压成 message，很容易丢失 reasoning 状态、工具类型、`call_id` 或事件顺序，也就失去了 Responses 对 Agent 最有价值的部分。

## 6. 为什么 Responses 更适合 Agent 开发

### 6.1 Agent 的基本单位不是“消息”，而是“动作”

Agent 一轮执行通常是：理解任务 → 推理 → 选择工具 → 等待结果 → 再推理 → 继续调用或回答。Chat Completions 可以模拟这个循环，但工具调用只是 assistant message 的附属结构；Responses 则把 reasoning、call、output、message 都提升为一等 Item，协议模型与 Agent 的真实状态机更一致。

### 6.2 工具链可精确关联和恢复

独立 `function_call` 与 `function_call_output` 通过 `call_id` 关联，天然适合并行调用、持久化、恢复和错误诊断。系统无需从文本、工具名或消息位置反推出调用关系。

### 6.3 推理上下文可以保真延续

推理模型在工具调用前后需要延续中间状态。Responses 能把 reasoning Item（包括无状态模式下的加密内容）作为上下文的一部分重放；如果只保存 assistant 文本，就可能让下一轮失去模型完成任务所需的信息。

### 6.4 事件流更适合执行可观测性

typed SSE 可以把“响应已创建、Item 已加入、参数正在生成、Item 已完成、整次响应已结束”分别暴露出来。对于工具进度、取消、审批、失败定位和 UI 时间线，这比解析一个复合 delta 更可靠。

### 6.5 原生工具减少重复编排

Web search、file search、computer use、code interpreter、image generation 和 remote MCP 等能力有统一入口。应用仍负责本地工具、安全与业务副作用，但不必为所有托管能力重复设计一套传输和循环协议。

### 6.6 状态策略更灵活

同一套接口既支持服务端延续，也支持完全由应用管理的无状态 Item 重放。Agent 平台可以按隐私、延迟、成本、分支和恢复需求选择，而不是被迫采用单一 transcript 模式。

### 6.7 新模型能力优先进入 Responses

OpenAI 当前明确建议新项目使用 Responses，并将它定位为未来 Agent 开发方向。对长期维护的 Agent 平台而言，围绕 typed Items 建模比持续为 message 增加供应商私有字段更可持续。

官方迁移指南还给出了两项内部测试结果：相同 prompt 与配置下，reasoning 模型经 Responses 在 SWE-bench 上约提升 3%，缓存利用率相对 Chat Completions 提升约 40%—80%，并可因此降低成本。它们说明 Responses 能利用更完整的推理和缓存上下文，但不是 Astro 的实测结论；项目仍应使用自己的任务集、Provider 和上下文策略做基准测试。

## 7. Astro 中的实际实现

### 7.1 两条清晰边界

Astro 当前不是“Agent 自动在 Chat 与 Responses 之间降级”，而是两条用途不同的路径：

```text
生产 Agent 路径
PromptContract + canonical ResponseItem history
  -> ProviderStreamer::stream_response
  -> try_stream_responses_with_fallback
  -> providers::agent_responses_stream
  -> ResponsesRequest.input: Vec<ResponseItem>
  -> OpenAIResponsesModel
  -> POST .../responses

非 Agent / 兼容路径
Vec<ChatCompletionMessage>
  -> providers::chat_stream
  -> ChatCompletionRequest.input: Vec<ChatCompletionMessage>
  -> OpenAICompletionModel 或其他协议 adapter
  -> POST .../chat/completions（当选择 Chat adapter 时）
```

`agent_responses_stream()` 会先调用 `supports_agent_responses()`，不满足能力的 Provider 在发出网络请求前即被拒绝；随后它把 `api_mode` 固定为 `responses`，并将原生历史直接放入 `ResponsesRequest.input`。因此，通用 Provider 配置中的 `api_mode` 不能绕过 Agent capability gate。

### 7.2 Astro 的请求映射

| 语义 | `ChatCompletionRequest` | `ResponsesRequest` |
| --- | --- | --- |
| `instructions` | 降级成 system message | 顶层 `instructions` |
| 输入 | `Vec<ChatCompletionMessage>` 序列化成 `messages[]` | `Vec<ResponseItem>` 直接序列化 |
| `tools` | 外层 `function` 包装 | 扁平 typed tool 定义 |
| `tool_choice` | 指定函数时为 `function.name` | 指定函数时为顶层 `name` |
| `max_tokens` | 当前兼容层发为 `max_tokens` | 映射为 `max_output_tokens` |
| `thinking` | 由厂商 hook 映射兼容字段 | 映射为 `reasoning.effort/summary` |

OpenAI adapter 当前还显式设置 `store: false`、允许 parallel tools 并请求 reasoning summary。这与 Astro 本地优先、rollout 可恢复的架构一致：服务端状态不是本地会话事实源。

### 7.3 Canonical history

Astro 使用 `agent_protocol::ResponseItem` 保存原生历史，主要覆盖：

- message 与多模态 content；
- reasoning 与 encrypted content；
- function/custom/tool-search call 及 output；
- web search、image generation；
- compaction 与上下文压缩条目。

Provider 边界的 `to_native_responses_input()` 直接序列化这些 Item，只移除 Astro 本地 metadata；当工具结果已经压缩时，只替换发送给模型的 `output` 视图，不破坏持久化的原始值。

这使 rollout、工具执行和下一次模型采样共享同一组事实，而不需要先投影为 Chat message 再猜测还原。

### 7.4 流式归一化

Astro 的 Responses parser 会把官方 typed events 映射为内部 `StreamChunk`：

- 文本 → `Text`；
- reasoning summary → `Thinking`；
- function/custom/tool-search call → `ToolCallStart` / `ToolCallDelta`；
- 完成的原生 Item → `ResponseItemDone`；
- usage → `Usage`；
- completed/incomplete/failed → `Done` 或 `Error`。

Chat parser 则从 `choices[].delta` 中抽取文本、reasoning 兼容字段、tool calls、usage 与 finish reason。两条 parser 最终只共享 `CompletionStream` 输出抽象；请求类型、模型 trait、registry 槽位和测试注入点均相互独立。

### 7.5 当前 Provider 策略

当前 profile 中声明为 Agent Responses-capable 的内置 Provider 是 OpenAI、DeepSeek、Azure OpenAI、OpenRouter、百炼、MiniMax 和 Mimo。权威来源始终是 `ProviderProfile.supports_responses`；第三方只提供名义上的 `/responses` endpoint，不足以证明其 reasoning、并行工具、custom tool、tool search、usage 和终态事件都兼容。

当前注册成功的 custom provider 也会被视为可进入 Agent 路由；这代表配置方声明兼容，不等于 Astro 已验证其行为。生产环境仍应为每个 custom endpoint 单独执行契约测试。

### 7.6 本轮对齐审计结果

| 检查项 | 当前结果 |
| --- | --- |
| Agent 请求类型 | `ResponsesRequest`，唯一历史字段为 `Vec<ResponseItem>` |
| Agent 模型 trait | `ResponsesModel` / `DynResponsesModel` |
| Registry 槽位 | `responses_model`，与 `chat_completion_model` 独立 |
| 厂商差异 trait | `OpenAIResponsesCompatible`，不再依赖 Chat 的 `OpenAICompatible` |
| 生产 Agent 入口 | `agent_responses_stream` / `agent_responses_prompt` |
| 测试注入 | `ResponsesOverrideInput { instructions, items }`，不再把 Items 还原成 Chat Completions 消息 |
| Chat Completions | 仅保留在 workflow、媒体与显式非 Agent 兼容调用中 |
| 隐式协议降级 | Agent 路径不存在；不支持 Responses 时返回 `UnsupportedCapability` |

`ModelTarget`、`model_targets` 表示模型调用目标/凭证链，不暗示 Chat Completions wire contract。其值会先经过 Responses capability gate，再进入 Agent 主链。

## 8. 对 Astro 的建议

### 8.1 保持 Responses-only 的 Agent 硬边界

不要在 Responses 请求失败后偷偷改发 Chat Completions。协议降级会丢失 Item 类型和推理上下文，还可能把认证、限流或无效请求误判成“协议不支持”。fallback 应只在支持 Responses 且通过契约验证的 target 之间发生。

### 8.2 把 capability 从布尔值升级为可验证矩阵

`supports_responses` 适合做入口 gate，但长期应进一步记录并验证：

- text / vision input；
- reasoning Item 与 encrypted reasoning；
- function、custom、tool search；
- parallel tool calls；
- structured outputs；
- usage 细项；
- `store`、background、conversation/previous response；
- 各类终态和错误语义。

Provider 启用前运行自动 conformance suite，比依据官网“兼容 OpenAI”字样开启更可靠。

### 8.3 明确本地状态与服务端状态的主从关系

Astro 应继续以本地 rollout 和 canonical `ResponseItem` 为事实源，默认 `store: false`；将 `previous_response_id` 或 Conversations 作为可选加速/托管能力，而不是恢复正确性的前提。若将来开放服务端状态，必须说明数据保留、删除、分支、跨 Provider fallback 和离线恢复行为。

### 8.4 对原始事件做可脱敏观测

建议为开发模式保留：request id、response id、event type、item id、call id、output index、usage、终态和 Provider 原始错误；敏感正文、密钥和工具结果必须脱敏或摘要化。这样能快速区分模型错误、协议错误、工具错误和本地状态机错误。

### 8.5 把工具执行设计成幂等、可重放

Responses 提高了调用关联的精度，但不会自动解决副作用重复执行。应以 `call_id + attempt` 建立去重键，对付款、删除、发送等高风险工具记录审批和执行结果，恢复时优先复用已完成 output，而不是再次执行。

### 8.6 建立双协议回归样本，但分开验收

Chat Completions 兼容测试应覆盖 message/tool-call lowering 和主流网关；Responses Agent 测试应覆盖完整 Item 生命周期、reasoning 重放、并行调用、压缩、取消和恢复。可以共享业务 fixture，不应共享会掩盖协议差异的 wire snapshot。

### 8.7 用真实任务衡量收益

迁移效果不应只看首 token 延迟。建议同时记录：任务成功率、工具参数正确率、平均工具轮数、恢复成功率、输入/输出/reasoning/cached token、端到端延迟、首包前 fallback 比例和每个成功任务成本。

## 9. 风险与限制

选择 Responses 不等于 Agent 自动完成。Harness 仍要负责：

- 工具注册、权限、沙箱和实际执行；
- tool call/output 的持久化与幂等；
- 上下文选择、压缩和 token 预算；
- 取消、超时、重试与 fallback；
- 事件落盘、恢复和 UI 投影；
- Prompt 注入防御、审计与合规。

同时还应接受以下现实：

1. Responses 的客户端状态机明显比 Chat delta parser 复杂；
2. 第三方“OpenAI-compatible”实现对 Responses 的兼容程度差异很大；
3. 服务端持久状态会引入隐私、保留期和供应商锁定问题；
4. `previous_response_id` 不消除历史 token 费用；
5. Responses 不提供 Chat Completions 的单请求多候选 `n`；
6. 只读取 `output_text` 会丢失 Agent 所需的 reasoning 和 tool Items。

## 10. 未来发展预测

以下是架构判断，不是已经承诺的产品事实。

### 10.1 “消息 API”会继续存在，但不再主导 Agent 内核

未来 1—3 年，Chat Completions 很可能继续承担兼容、简单生成和轻量聊天需求，不会立即消失；但新的推理、工具和长任务能力会更优先围绕 Responses 一类执行协议建设。

### 10.2 typed Item/event 会成为 Agent 协议的共同方向

只要 Agent 需要并行工具、审批、中断、恢复和多模态输出，单一 message 就会越来越臃肿。不同厂商即使字段不完全一致，也会趋向“类型化输入项 + 类型化输出项 + 生命周期事件”的结构。

### 10.3 状态管理会从 transcript 走向可分支执行图

长期会话不会只是不断增长的消息数组，而会结合 response lineage、conversation、compaction、缓存和本地 event sourcing。分支、回滚、共享前缀和跨设备恢复将成为标准 Agent 基础设施。

### 10.4 托管工具与本地工具会形成混合执行

搜索、文件检索、代码解释器等通用能力会更多由模型平台托管；企业数据、桌面操作和高风险副作用仍主要在本地或私有环境执行。协议的关键将是统一身份、权限、调用 ID、审计和结果回传，而不是所有工具都运行在同一位置。

### 10.5 Provider “兼容”会从 endpoint 兼容升级为行为兼容

未来真正有价值的兼容声明会包含事件顺序、reasoning 延续、工具类型、错误语义、usage 精度和状态策略，而不只是接受 `/v1/responses` JSON。Astro 的 capability gate 和 conformance tests 应朝这个方向演进。

### 10.6 Agent 竞争会转向 Harness 质量

当模型和 Responses 类协议逐渐标准化，差异化会更多来自 Harness：上下文选择、工具治理、恢复能力、可观测性、评估、成本控制和人机协作。API 只是执行入口，不是完整 Agent 产品。

## 11. 选型建议

| 场景 | 建议 |
| --- | --- |
| 新建 OpenAI Agent、多轮工具或 reasoning 应用 | 优先 Responses |
| Astro 生产 Agent 主链路 | 只使用通过 capability 与契约测试的 Responses Provider |
| 普通聊天、摘要、分类、一次性生成 | 两者均可；新 OpenAI 项目仍优先 Responses |
| 需要广泛兼容本地模型和旧网关 | 保留 Chat Completions 适配层 |
| 强本地隐私、可恢复、可分支 | Responses + `store: false` + 本地完整 Item/rollout |
| 已有大型 message 基础设施 | 分业务流渐进迁移，不做一次性替换 |
| 需要单请求多个候选输出 | Chat Completions，或对 Responses 发起多个请求 |

## 12. 参考资料与代码事实源

OpenAI 官方资料：

- [Migrate to the Responses API](https://platform.openai.com/docs/guides/migrate-to-responses)
- [Chat Completions create reference](https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create)
- [Responses create reference](https://developers.openai.com/api/reference/resources/responses/methods/create)
- [Function calling](https://developers.openai.com/api/docs/guides/function-calling)
- [Conversation state](https://developers.openai.com/api/docs/guides/conversation-state)
- [Streaming API responses](https://developers.openai.com/api/docs/guides/streaming-responses)

Astro 当前代码：

- [`crates/agent-providers/src/dispatch.rs`](../../../crates/agent-providers/src/dispatch.rs)：Agent Responses 入口、capability gate 和通用分发；
- [`crates/agent-providers/src/compat/completion.rs`](../../../crates/agent-providers/src/compat/completion.rs)：Chat Completions 请求；
- [`crates/agent-providers/src/compat/responses.rs`](../../../crates/agent-providers/src/compat/responses.rs)：Responses 请求；
- [`crates/agent-providers/src/openai/responses.rs`](../../../crates/agent-providers/src/openai/responses.rs)：ResponseItem 序列化与 SSE 解析；
- [`crates/agent-providers/src/profile.rs`](../../../crates/agent-providers/src/profile.rs)：`ApiMode` 与 Provider capability；
- [`crates/agent-providers/src/types/request.rs`](../../../crates/agent-providers/src/types/request.rs)：独立的 Responses / Chat 请求类型；
- [`crates/agent-protocol/src/response_item.rs`](../../../crates/agent-protocol/src/response_item.rs)：canonical `ResponseItem`；
- [`crates/agent-core/src/streaming/provider.rs`](../../../crates/agent-core/src/streaming/provider.rs)：Agent 侧 Responses 调用链；
- [Responses 原生 Agent 运行时架构](../01-架构设计/12-Responses原生Agent运行时架构.md)：系统级设计基线。

最终原则可以概括为：**Chat Completions 组织对话，Responses 组织执行；Agent 需要的恰恰是可观察、可关联、可恢复的执行。**
