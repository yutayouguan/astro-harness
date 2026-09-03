# providers

多供应商模型与媒体适配层。Agent 对话只走 Responses API；Chat Completions、Anthropic Messages、Gemini Native 和 Interactions 仅保留给工具、媒体或其他非 Agent 调用。

## 两条调用边界

| 边界 | 入口 | 输入 | 协议规则 |
| --- | --- | --- | --- |
| Agent | `agent_responses_stream` / `agent_responses_prompt` | `Vec<agent_protocol::ResponseItem>` | 必须声明 Responses capability；不做 Chat fallback |
| 非 Agent 兼容入口 | `chat_stream`、媒体/embedding API | `ChatCompletionMessage` 或专用请求 | 可按调用方需要使用兼容协议 |

两条边界使用不同请求类型，不存在双输入或隐式降级：

- `ResponsesRequest`：Agent 权威请求，直接持有 `Vec<ResponseItem>`；
- `ChatCompletionRequest`：非 Agent 兼容请求，持有 `Vec<ChatCompletionMessage>`。

## 核心职责

- 检查 `ProviderProfile.supports_responses`，阻止不支持 Responses 的 Provider 进入 Agent target/fallback chain；
- 将 `ResponseItem`、原生工具定义和 instructions 发送到 `/responses`；
- Namespace 工具在 Responses 路径保留原生容器；非 Agent Function-only adapter 会省略不支持的 namespace，不再展平为伪 Function 名；
- 解析 text、reasoning、tool-call、usage 和终态流式事件；
- 为非 Agent 调用保留聊天、embedding、图像、音频、视频等能力；
- 提供 Provider profile、API key 探测、HTTP/SSE、媒体转换和错误归一化。

## 关键模块

| 路径 | 职责 |
| --- | --- |
| `src/dispatch.rs` | Agent Responses 入口及通用 provider/media 分发 |
| `src/profile.rs` | `ProviderProfile`、`ApiMode`、Responses capability |
| `src/types/request.rs` | `ResponsesRequest` 与 `ChatCompletionRequest` 两条请求边界 |
| `src/types/request_content.rs` | 非 Agent `ChatCompletionMessage`、请求内容块与工具定义 |
| `src/types/stream.rs` | `CompletionStream`、`StreamChunk`、`Usage` |
| `src/compat/responses.rs` | Responses 请求序列化与 SSE 解码 |
| `src/compat/completion.rs` | 非 Agent Chat Completions 兼容实现 |
| `src/registry.rs` | 协议 adapter 注册与选择 |
| `src/impls/` | 内置 Provider 实现 |
| `src/openai/`、`src/google/`、`src/minimax/` | 厂商专用媒体和扩展 API |

## Agent 请求

```rust
agent_responses_stream(
    provider,
    instructions,
    response_items,
    tools,
    &config,
).await
```

该入口会规范化 provider id、执行 capability gate，并构造只接受原生 Items 的 `ResponsesRequest`。Agent 辅助任务使用 `agent_responses_prompt()`，同样受该 gate 约束。

## 关键不变量

1. Agent 入口只接受 `Vec<ResponseItem>`，不接受 `ChatCompletionMessage`，不回退到 Chat Completions。
2. `ResponseItem` 的 call id、item type、顺序和 call/output 配对不得丢失。
3. `supports_responses` 是 Agent 可路由能力；`api_mode` 只是通用 adapter 选择，不能越过 capability gate。
4. fallback 只在首个可见 chunk 前切换，并且候选目标也必须支持 Responses。
5. Provider usage 统一归一化，但保留 reported 状态；reasoning 是 output 子集，cached input 是 input 子集。
6. `ResponsesModel`、`DynResponsesModel` 和 registry 的 `responses_model` 与 Chat compatibility model 分开声明。
7. 测试注入也直接接收 `ResponsesOverrideInput`，不得为测试把 Items 降级成 Chat Completions 消息。

## 文档

- [Provider 架构](ARCHITECTURE.md)
- [Responses Provider 参考](RESPONSES-API.md)
- [Responses 原生 Agent 运行时架构](../../docs/03-系统设计阶段/01-架构设计/12-Responses原生Agent运行时架构.md)

## 验证

```bash
cargo test -p providers
```
