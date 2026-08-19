//! 流式事件类型：Provider chunk → Agent 语义事件的类型层。
//!
//! `StreamedAssistantContent` 基于新 `StreamChunk` 统一模型。

use std::pin::Pin;

use futures::Stream;
use futures::StreamExt;
use providers::types::stream::StreamChunk;
use providers::Usage;
use types::ToolCallDelta;

/// 单次模型流式片段，对齐 Rig `StreamedAssistantContent` 并扩展 Reasoning 通道。
#[derive(Debug, Clone)]
pub enum StreamedAssistantContent {
    /// 可见 assistant 文本 token。
    Text(String),
    /// 推理/思考过程 token（部分 Provider 专用）。
    Reasoning(String),
    /// Google Interactions：`thought.signature`（无状态多轮回放必需）。
    ThoughtSignature(String),
    /// 工具调用的增量片段，需经 [`types::ToolCallAccumulator`] 合并。
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

/// Provider 层 assistant 内容流：每项为 `StreamedAssistantContent` 或错误。
pub type AssistantContentStream =
    Pin<Box<dyn Stream<Item = anyhow::Result<StreamedAssistantContent>> + Send>>;

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
