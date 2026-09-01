# Responses API Provider 参考

> 状态：当前实现基线
> 更新：2026-09-01

Agent 对话只支持 Responses API。本文记录 Provider 接入要求；Chat Completions 不是 Agent 的第二选项，只属于工具、媒体和独立兼容调用。

## 统一接入

Provider 成为 Agent target 必须同时满足：

1. 提供可用的 `/responses` 兼容端点；
2. profile 默认 `api_mode: ApiMode::Responses`；
3. profile 标记 `supports_responses: true`；
4. 通过 `agent_responses_stream()` 的原生 `ResponseItem` 多轮工具测试；
5. 保持 call/output id、类型与顺序；
6. 正确解析文本、reasoning、tool call、usage 和终态 SSE。

registry 可以同时保存 Responses、Chat 和媒体能力，但它们是独立槽位。Agent 只读取 `responses_model`；`attach_responses()` 只是挂载 Responses adapter，不替换、不升级 Chat model，也不存在失败后改发 Chat 的路径。

Agent 请求使用 `ResponsesRequest`：顶层 `instructions`、`Vec<ResponseItem>`、typed tools、tool choice、parallel tools、thinking 与 token 参数各自保留。非 Agent 调用使用独立的 `ChatCompletionRequest`，两者没有共享的消息输入字段。

## 当前内置能力

当前 profile 中标记为 Responses-capable 的 Provider 包括 OpenAI、DeepSeek、Azure OpenAI、百炼、MiniMax 和 Mimo。代码中的 `ProviderProfile.supports_responses` 是权威列表；文档列表只用于说明，新增或移除能力时必须同步更新测试。

| Provider | 端点约定 | 主要差异 |
| --- | --- | --- |
| OpenAI | `POST /v1/responses` | `store: false`、parallel tools、reasoning summary |
| DeepSeek | `POST /v1/responses` | OpenAI Responses 兼容；thinking 字段由 adapter 归一化 |
| Azure OpenAI | `POST /openai/v1/responses` | `api-key` header；deployment name 位于 model 字段 |
| 百炼 | `POST /compatible-mode/v1/responses` | thinking/cache 扩展参数；不支持 background |
| MiniMax | `POST /v1/responses` | reasoning details 与媒体 API 分离 |
| Mimo | `POST /v1/responses` | OpenAI Responses 兼容 |

## 暂不进入 Agent 路由

Anthropic、Google/Gemini、Ollama、OpenRouter、智谱、Moonshot、火山、混元、NVIDIA 等 profile 当前未声明 Responses capability。即使它们拥有 Chat Completions、Messages、Interactions 或原生协议实现，也不能作为 Agent primary/fallback target。

若上游后来提供 Responses，必须完成兼容验证后再打开 capability；不能仅因为端点名称存在就启用。

## 必测场景

- user → assistant final text；
- user → function call → function output → assistant；
- 并行多个 tool calls；
- custom tool 与 tool search call/output；
- reasoning + tool call 同轮；
- tool output 与 call 邻接、孤立 output 清理；
- 首个 chunk 前网络/5xx fallback；
- 400/401/429 不被错误降级到 Chat Completions；
- cached input、reasoning、reported total usage。
- 测试覆盖接缝直接接收 `ResponsesOverrideInput`，不得将 Items 投影为 Chat `Message`。

## 相关文档

- [Providers 架构](ARCHITECTURE.md)
- [Responses 原生 Agent 运行时架构](../../docs/03-系统设计阶段/01-架构设计/12-Responses原生Agent运行时架构.md)
