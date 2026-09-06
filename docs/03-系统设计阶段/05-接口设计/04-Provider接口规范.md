# Provider 接口规范

> 阶段：系统设计
> 状态：当前实现基线
> 更新：2026-09-04

## 1. 边界

Provider 层不拥有 Agent turn、工具执行、审批、Hook 或持久化生命周期。它只负责：

- 将已组装的请求序列化为厂商协议；
- 发起 HTTP/SSE 请求；
- 将响应解析为 `CompletionStream` / `StreamChunk`；
- 根据 profile 公布能力。

Agent 和非 Agent 调用使用两组不可混用的类型。历史的 `CompletionRequest`
已删除，不得重新引入包含两套输入的通用请求对象。

## 2. Agent Responses 接口

```rust
#[async_trait]
pub trait ResponsesModel: Send + Sync {
    async fn stream(&self, prompt: ResponsesRequest) -> anyhow::Result<CompletionStream>;
}

pub struct ResponsesRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<agent_protocol::ResponseItem>,
    pub tools: Vec<ToolDefinition>,
    pub tool_choice: Option<ToolChoice>,
    pub parallel_tool_calls: Option<bool>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub thinking: Option<ThinkingConfig>,
    pub additional_params: serde_json::Value,
}
```

Agent 入口是 `agent_responses_stream()` 和 `agent_responses_prompt()`。它们必须：

1. 校验 `supports_agent_responses(provider)`；
2. 将 `api_mode` 固定为 `responses`；
3. 把 canonical `Vec<ResponseItem>` 直接放入 `ResponsesRequest.input`；
4. 保留原生 tool definition、call id、reasoning、metadata 和 item 顺序；
5. 拒绝不支持 Responses API 的 target，不降级到 Chat Completions。

`instructions` 是 Responses 顶层指令；不应改写成 system message。`tools` 是独立的
tagged union；不应编码进指令文本。

`persistent` reasoning 是受模型目录门控的本地 effort。只有 OpenAI adapter 声明
`SUPPORTS_PERSISTENT_REASONING`；请求必须携带内部 persistent instructions，adapter 将
wire effort 映射为 `disabled`、把指令合并进顶层 instructions，并在发送前移除内部键。
其他 Responses adapter 必须返回不支持错误，不能把 `persistent` 原样发送或静默改成普通档位。

## 3. 非 Agent 兼容接口

```rust
#[async_trait]
pub trait ChatCompletionModel: Send + Sync {
    async fn stream(&self, request: ChatCompletionRequest)
        -> anyhow::Result<CompletionStream>;
}

pub struct ChatCompletionRequest {
    pub model: String,
    pub instructions: String,
    pub input: Vec<Message>,
    // tools, sampling and provider-specific fields
}
```

`ChatCompletionRequest` 只服务工具、媒体、连通性检查或明确的非 Agent 调用。
`chat_stream()` / `chat_stream_direct()` 不得被 `agent-core` 主模型、fallback 或 Agent
辅助任务调用。

## 4. 工具配对契约

Responses adapter 必须保留原生对应关系：

```text
FunctionCall(call_id=A)       -> FunctionCallOutput(call_id=A)
CustomToolCall(call_id=B)     -> CustomToolCallOutput(call_id=B)
ToolSearchCall(call_id=C)     -> ToolSearchOutput(call_id=C)
```

工具输出不得合并到 assistant message，不得重生成 call id。下一次 sampling 发送同一组
原生 item，避免出现 `No tool output found for tool call ...`。对缺失 output 的中断历史，
prompt 边界可追加确定性 `aborted` output，但不改写持久化历史。

## 5. 流式与 fallback

`CompletionStream` 是响应流名称，不是请求 DTO。`StreamChunk` 表达 text、reasoning、
tool-call delta、usage 和终态。Agent fallback 只允许在首个可见 chunk 前切换，且所有候选
target 都必须通过 Responses capability gate。

## 6. 其他能力

Embedding、TTS、ASR、图像、视频和音乐使用各自的专用 request/model trait。这些能力可以
调用厂商原生协议，但不能成为 Agent 对话的协议回退通道。

## 7. 代码位置

| 契约 | 位置 |
| --- | --- |
| 请求类型 | `crates/agent-providers/src/types/request.rs` |
| 模型 trait | `crates/agent-providers/src/traits/models.rs` |
| Agent/Chat 分发 | `crates/agent-providers/src/dispatch.rs` |
| Responses wire adapter | `crates/agent-providers/src/compat/responses.rs` |
| Chat 兼容 adapter | `crates/agent-providers/src/compat/completion.rs` |
| capability profile | `crates/agent-providers/src/profile.rs` |
