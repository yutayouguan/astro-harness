# agent-providers 详细设计

> 版本：v2.0
> 日期：2026-09-01
> 状态：当前实现基线

## 1. 设计边界

`agent-providers` 是多供应商协议与媒体适配层。其最重要的边界是区分：

- **Agent Responses**：Responses-only，输入为 canonical `agent_protocol::ResponseItem`；
- **非 Agent 兼容调用**：可保留 `ChatCompletionMessage` 和厂商原生协议。

因此 `ApiMode` 的多协议枚举属于通用 Provider 层，不是 Agent 模式选项。Agent target 必须通过 `supports_agent_responses()`；不支持 Responses 时在调用前返回 `UnsupportedCapability`。

## 2. 请求模型

Agent 与兼容路径使用两个互不兼容的请求类型：

| 类型/字段 | 语义 |
| --- | --- |
| `ResponsesRequest.instructions` | Agent 独立稳定指令 |
| `ResponsesRequest.input` | Agent 的 `Vec<ResponseItem>`，adapter 直接序列化 |
| `ResponsesRequest.tools` | 原生工具定义，不拼入 prompt |
| `ChatCompletionRequest.input` | 非 Agent 的 `Vec<ChatCompletionMessage>` 兼容入口 |

两个请求类型没有共享的消息输入字段，因此 Agent 无法在类型层误传 `Vec<ChatCompletionMessage>`。UI 使用的 `ConversationEntry` 也不能作为恢复后的模型历史。

## 3. Agent 请求链

```text
ProviderStreamer::stream_responses_with_contract
  -> to_response_items_with_context_history
  -> ProviderStreamer::stream_response
  -> try_stream_responses_with_fallback
  -> providers::agent_responses_stream
  -> ResponsesRequest.input
  -> Registry responses_model
  -> Responses adapter
```

生产 Agent 链路不会调用 `chat_stream()`。测试通过 `ResponsesOverride` / `ResponsesOverrideInput` 接收原生 Items，不存在测试专用的 Chat Completions 降级路径。

### 3.1 Target 过滤

primary 与 fallback targets 在进入请求前都必须支持 Responses。辅助任务缺省继承 primary chain，但仍使用 `agent_responses_prompt()` 并执行相同 capability gate。显式 `api_mode` 只能影响通用 provider 入口，不能把非 Responses Provider 带入 Agent。

### 3.2 首包前 fallback

只有在尚未交付首个可见 chunk 时，网络错误或可 failover 错误才能切换下一个 Responses target。一旦已经输出文本、reasoning 或工具调用分片，必须终止当前响应并向上报告错误，不能拼接另一 Provider 的结果。

## 4. 原生历史与工具配对

Agent 历史由 `ResponseItem` 表示并直接进入 Responses input：

- `ResponseItem::Message` content items；
- reasoning；
- function call/output；
- custom tool call/output；
- tool search call/output。

工具循环先保存 assistant call item，再执行工具并保存 matching output item。call id 是唯一关联键，工具名和文本位置都不能代替它。下一次请求必须保留二者的类型和顺序。

兼容清理只允许移除无对应 call 的孤立 output，或在确定的同一边界内恢复邻接；不得把旧 output 移过新的 assistant/tool call，也不得生成会掩盖数据损坏的假 call id。

## 5. Adapter 与 Registry

`Registry` 分别注册 `responses_model`、`chat_completion_model` 与媒体能力。Agent 入口先执行 capability gate，再把本次配置固定为 Responses；adapter 选择不能反向覆盖这一决定。`OpenAIResponsesCompatible` 与 Chat 的 `OpenAICompatible` 也是独立 trait。

Provider profile 描述：

- 默认 endpoint 和认证方式；
- 通用 `ApiMode`；
- `supports_responses`；
- thinking、视觉、工具和媒体能力；
- 环境变量与模型默认值。

custom provider 进入 Agent 路由时按 Responses endpoint 契约处理；配置方负责提供真正兼容的 endpoint，运行时不会替它降级到 Chat Completions。

## 6. Streaming 与 usage

所有 completion adapter 输出 `CompletionStream`。Responses adapter 将 SSE 映射到统一 chunk：text delta、reasoning delta、tool-call delta、usage 和 finish。`agent-core` 负责组装完整 `ResponseItem`，Provider 不把自由文本猜成工具调用。

Usage 归一化规则：

1. cached input 是 input 的子集；
2. reasoning 是 output 的子集；
3. Provider 原始 total 优先，缺失时才由分项重算；
4. 未报告和报告为零必须可区分；
5. 多请求合并只有在每次都报告某细项时才保留 reported 标志。

## 7. 非 Agent 能力

以下能力继续使用最合适的厂商协议，不受 Agent Responses-only 决策影响：

- embedding；
- 图像生成与编辑；
- TTS / ASR；
- 视频和音乐；
- 文件上传、voice clone 等厂商扩展；
- 明确调用 `chat_stream()` 的工具级兼容任务。

这些调用不得把其 `ChatCompletionMessage` 历史写回 Agent canonical rollout。

## 8. 错误与重试

- 认证、无效请求、内容过滤和不支持能力直接返回；
- 429 按 retry policy 处理，不偷偷换协议；
- 网络与 5xx 可在首包前按 fallback policy 切换；
- Responses 400 应保留服务端诊断，特别是 call/output 配对错误；
- 任意失败都不得自动重发到 `/chat/completions`。

## 9. 验证矩阵

| 范围 | 必须覆盖 |
| --- | --- |
| capability | 非 Responses target 被拒绝，fallback 也被过滤 |
| serialization | `ResponseItem` 类型、id、顺序原样进入 body |
| tools | function/custom/tool-search call/output 多轮配对 |
| stream | text、reasoning、tool delta、usage、finish |
| fallback | 仅首包前切换 |
| utility | Chat/媒体入口不污染 Agent 路由 |

## 10. 事实源

- `ProviderProfile` 同时持有 `ProviderKind`；内置 Provider 的能力声明与运行时构造均从该 profile 表选择，不得再新建独立的 provider-id 分支表。
- `crates/agent-providers/src/dispatch.rs`
- `crates/agent-providers/src/profile.rs`
- `crates/agent-providers/src/types/request.rs`
- `crates/agent-providers/src/compat/responses.rs`
- `crates/agent-core/src/streaming/provider.rs`
- `crates/agent-protocol/src/response_item.rs`

系统级说明见 [Responses 原生 Agent 运行时架构](../../03-系统设计阶段/01-架构设计/12-Responses原生Agent运行时架构.md)。
