//! 流式事件类型：Provider chunk → Agent 语义事件的类型层。
//!
//! `StreamedAssistantContent` 基于新 `StreamChunk` 统一模型。

use std::pin::Pin;

use futures::Stream;
use futures::StreamExt;
use providers::streaming::Usage;
use providers::types::stream::StreamChunk;
use tools::ToolCallDelta;

/// 单次模型流式片段，对齐 Rig `StreamedAssistantContent` 并扩展 Reasoning 通道。
#[derive(Debug, Clone)]
pub enum StreamedAssistantContent {
    /// 可见 assistant 文本 token。
    Text(String),
    /// 推理/思考过程 token（部分 Provider 专用）。
    Reasoning(String),
    /// Google Interactions：`thought.signature`（无状态多轮回放必需）。
    ThoughtSignature(String),
    /// 工具调用的增量片段，需经 [`tools::ToolCallAccumulator`] 合并。
    ToolCallDelta(ToolCallDelta),
    /// 本轮或累计 token 用量，通常在流末尾出现。
    FinalUsage(Usage),
    /// Anthropic citations delta（引用信息）。
    Citations(Vec<serde_json::Value>),
    /// Google Interactions：interaction id。
    InteractionId(String),
}

impl StreamedAssistantContent {
    /// 从新 `StreamChunk` 转换。
    pub fn from_stream_chunk(chunk: StreamChunk) -> Option<Self> {
        match chunk {
            StreamChunk::Text(t) => Some(Self::Text(t)),
            StreamChunk::Thinking(t) => Some(Self::Reasoning(t)),
            StreamChunk::ThoughtSignature(s) => Some(Self::ThoughtSignature(s)),
            StreamChunk::ToolCallStart { index, id, name } => {
                Some(Self::ToolCallDelta(ToolCallDelta {
                    index,
                    id: Some(id),
                    name: Some(name),
                    arguments: None,
                    signature: None,
                }))
            }
            StreamChunk::ToolCallDelta { index, arguments } => {
                Some(Self::ToolCallDelta(ToolCallDelta {
                    index,
                    id: None,
                    name: None,
                    arguments: Some(arguments),
                    signature: None,
                }))
            }
            StreamChunk::Usage(u) => Some(Self::FinalUsage(Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_tokens: u.cache_read_tokens,
                cache_write_tokens: u.cache_write_tokens,
                reasoning_tokens: u.reasoning_tokens,
                request_count: u.request_count,
            })),
            StreamChunk::Citation(v) => Some(Self::Citations(vec![v])),
            StreamChunk::InteractionId(id) => Some(Self::InteractionId(id)),
            StreamChunk::Done { .. } | StreamChunk::Error(_) => None,
        }
    }
}

/// 多轮 Agent 流式事件，在 assistant 片段之上扩展工具结果与产品语义。
#[derive(Debug, Clone)]
pub enum MultiTurnStreamItem {
    /// 模型输出片段（文本、推理、工具 delta、usage）。
    Assistant(StreamedAssistantContent),
    /// 单次工具调用完成后的结果，供 UI 展示。
    ToolResult {
        /// 与 assistant tool_call 对应的 id。
        id: String,
        /// 工具 qualified name。
        name: String,
        /// 原始 arguments JSON 字符串。
        arguments_json: String,
        /// 工具返回文本（含错误前缀时仍原样传递）。
        result: String,
        /// 结构化媒体（生成图/音/视频）；空则前端可回落解析 result 文本。
        media: Vec<common::MediaAsset>,
    },
    /// 记忆工具成功变更，供右侧时间线展示。
    MemoryUpdate {
        /// 操作类型：`memory`（单一记忆工具）或其 action 描述。
        op: String,
        /// 结果预览（最长 240 字符）。
        content: String,
    },
    /// 本轮 API 请求前的上下文占用估算（分层 token 快照）。
    ContextUsage(crate::prompt::context_usage::ContextUsageSnapshot),
    /// AG-UI `RUN_STARTED`：一次用户发送对应一个 run。
    RunStarted { thread_id: String, run_id: String },
    /// AG-UI `ACTIVITY_SNAPSHOT`（如 A2UI surface）。
    Activity {
        message_id: String,
        activity_type: String,
        content_json: String,
        replace: bool,
    },
    /// AG-UI `RUN_FINISHED`：`outcome_type` 为 `success`、`interrupt` 或 `hitl_waiting`。
    /// `hitl_waiting`：同回合阻塞 HITL，流不随后发 Done。
    RunFinished {
        run_id: String,
        outcome_type: String,
        /// JSON array of Interrupt；success 时为空数组 `[]`。
        interrupts_json: String,
    },
    /// 不可恢复错误，之后必跟 `Done`。
    Error(String),
    /// 流正常或异常结束标记。
    Done,
}

/// Provider 层 assistant 内容流：每项为 `StreamedAssistantContent` 或错误。
pub type AssistantContentStream =
    Pin<Box<dyn Stream<Item = anyhow::Result<StreamedAssistantContent>> + Send>>;

/// 多轮 Agent 事件流：由 [`super::stream_multi_turn`] 暴露给 gRPC / UI 消费。
pub type MultiTurnStream = Pin<Box<dyn Stream<Item = anyhow::Result<MultiTurnStreamItem>> + Send>>;

/// 将新 `CompletionStream`（新类型）直接映射为 [`AssistantContentStream`]。
///
/// 直接 StreamChunk → StreamedAssistantContent，跳过旧类型中转。
pub(crate) fn map_new_provider_stream(
    stream: providers::types::CompletionStream,
) -> AssistantContentStream {
    Box::pin(stream.filter_map(|item| async move {
        match item {
            Ok(chunk) => match chunk {
                StreamChunk::Error(msg) => Some(Err(anyhow::anyhow!("{msg}"))),
                StreamChunk::Done { .. } => None,
                other => StreamedAssistantContent::from_stream_chunk(other).map(Ok),
            },
            Err(err) => Some(Err(err)),
        }
    }))
}
