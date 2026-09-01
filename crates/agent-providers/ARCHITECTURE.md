# Providers 架构

> 状态：当前实现基线
> 更新：2026-09-01

## 1. 设计目标

Provider crate 同时服务两类需求，但不混淆其协议边界：

1. **Agent completion**：只使用 Responses API，以 `ResponseItem` 为原生历史。
2. **工具与媒体能力**：允许使用厂商原生或兼容协议，例如 Chat Completions、Anthropic Messages、Gemini Native、Interactions、embedding、TTS、图像和视频 API。

`ApiMode` 中仍出现多种协议，不表示 Agent 可在它们之间切换。Agent 的资格由 `supports_agent_responses()` 单独决定。

## 2. 类型边界

```rust
pub struct CompletionRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<Message>,
    pub response_input: Option<Vec<agent_protocol::ResponseItem>>,
    pub tools: Vec<ToolDefinition>,
    // tool choice, thinking, token and provider-specific parameters...
}
```

字段语义：

| 字段 | 所属路径 | 规则 |
| --- | --- | --- |
| `instructions` | Agent 与通用 completion | 稳定指令，不与历史消息混排 |
| `response_input` | Agent | canonical Responses input；adapter 直接序列化 |
| `input` | 非 Agent | `Message` 兼容输入；Agent 请求必须为空 |
| `tools` | 独立 schema | 保留 function/custom/namespace/tool_search/web_search 类型 |

Agent 内部可以为了 UI、搜索或特定辅助计算生成 `Message` 视图，但该视图不回写 canonical history，也不用于发送下一次 Agent 请求。

## 3. Agent 路径

```text
agent-core Vec<ResponseItem>
  -> agent_responses_stream
  -> normalize provider id
  -> supports_agent_responses
  -> CompletionRequest { response_input: Some(...), input: [] }
  -> Registry completion model
  -> Responses adapter
  -> POST /responses + SSE
  -> CompletionStream
```

`agent_responses_prompt()` 是标题、压缩、记忆回顾、审批等一轮式 Agent 辅助任务的便捷入口。它构造原生 user `ResponseItem`，不经过 Chat message lowering。

### 3.1 Capability gate

内置 Provider 只有在 `ProviderProfile.supports_responses = true` 时才能参与 Agent 路由。custom provider 由 custom Responses 配置进入该入口。找不到 capability 时返回 `UnsupportedCapability`，而不是静默改发 `/chat/completions`。

主目标与所有 fallback 候选都执行相同检查。显式保存的 `api_mode = chat_completions` 不能改变 Agent-only 约束。

### 3.2 原生工具历史

Responses adapter 必须保留下列关系：

```text
FunctionCall(call_id=A)       -> FunctionCallOutput(call_id=A)
CustomToolCall(call_id=B)     -> CustomToolCallOutput(call_id=B)
ToolSearchCall(id=C)          -> ToolSearchOutput(id=C)
```

模型发出的 call item 在工具执行前进入历史；对应 output 完成后追加。下一次 sampling 使用同一组原生项。禁止用 assistant/tool `Message` 猜测或重新生成 call id。

历史修复只能处理确知的边界情况，例如丢弃没有对应 call 的孤立 output；不得跨过后续 assistant call 重排旧结果。

## 4. 通用 Provider 路径

`chat_stream()` 和 `chat_stream_direct()` 为非 Agent 调用保留。调用者可通过 `CompletionRequest.input` 使用 `Message`，registry 再按 `ApiMode` 选择 Chat Completions、Anthropic、Responses、Interactions 或 Gemini Native adapter。

这条路径主要服务：

- 媒体工具内部的文本辅助调用；
- provider 连通性或兼容性检查；
- 尚未迁移到 Agent runtime 的独立能力；
- 各厂商专用 API。

通用路径不得被 `agent-core` 的主模型或辅助模型绕过 Agent Responses 入口使用。

## 5. 流式协议

Provider adapter 输出统一 `CompletionStream`。`StreamChunk` 携带：

- 文本与 reasoning delta；
- 原生 tool-call delta；
- usage；
- finish/error 状态。

`agent-core` 负责把 delta 累积成 canonical `ResponseItem`、持久化完成项并驱动工具循环。自由文本不参与工具识别。

fallback 只允许在首个可见 chunk 前发生；一旦已有模型输出，不切换 Provider 拼接另一条响应。

## 6. Usage 归一化

所有 adapter 将用量映射为统一 `Usage`，同时保留字段是否由 Provider 实际报告：

- Responses 的 cached input 已包含在 input 中；
- reasoning 已包含在 output 中；
- 未上报的细项不能伪装为 Provider 报告的零；
- 多请求聚合仅在所有请求均报告某字段时保持其 reported 标记。

## 7. 新增 Agent Provider

要让新 Provider 进入 Agent 路由：

1. 实现或复用 Responses adapter；
2. 验证 instructions、`ResponseItem` 输入、reasoning、工具调用、工具输出、usage 和流终态；
3. 在 profile 中将默认 `api_mode` 设为 `Responses`；
4. 仅在测试通过后设置 `supports_responses = true`；
5. 添加工具多轮、call/output 邻接、首包前 fallback 和 4xx 错误回归测试。

只实现 Chat Completions 或其他协议的 Provider 可以继续服务非 Agent 能力，但不能标记为 Agent-capable。

## 8. 事实源

| 契约 | 路径 |
| --- | --- |
| Agent/通用入口 | `src/dispatch.rs` |
| Provider capability | `src/profile.rs` |
| 请求类型 | `src/types/request.rs` |
| Responses adapter | `src/compat/responses.rs` |
| Registry | `src/registry.rs` |
| 流式类型 | `src/types/stream.rs` |
| canonical history | `../agent-protocol/src/response_item.rs` |
