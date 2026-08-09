//! 流式响应累积器：将 `AssistantContentStream` 收集为结构化结果。
//!
//! headless（无头/cron）路径直接使用本模块；streaming 路径因需实时推送 UI 事件
//! 而自行累积，但数据结构对齐。

use futures::StreamExt;
use providers::Usage;
use tools::{ParsedToolCall, ToolCallAccumulator};

use super::types::{AssistantContentStream, StreamedAssistantContent};

/// 单轮 LLM 流式响应的累积结果。
pub(crate) struct AccumulatedResponse {
    pub text: String,
    pub reasoning: String,
    #[allow(dead_code)]
    pub thought_signature: Option<String>,
    pub calls: Vec<ParsedToolCall>,
    pub usage: Option<Usage>,
}

/// 消费整个 `AssistantContentStream`，收集文本、推理、工具调用与用量。
///
/// 适用于 headless 等不需要逐 token 推送的场景。
pub(crate) async fn collect_response(
    mut stream: AssistantContentStream,
) -> anyhow::Result<AccumulatedResponse> {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut thought_signature = None;
    let mut tool_acc = ToolCallAccumulator::new();
    let mut usage: Option<Usage> = None;

    while let Some(item) = stream.next().await {
        match item? {
            StreamedAssistantContent::Text(t) => text.push_str(&t),
            StreamedAssistantContent::Reasoning(r) => reasoning.push_str(&r),
            StreamedAssistantContent::ThoughtSignature(s) => thought_signature = Some(s),
            StreamedAssistantContent::ToolCallDelta(d) => tool_acc.push(&d),
            StreamedAssistantContent::FinalUsage(u) => usage = Some(u),
            StreamedAssistantContent::Citations(_) | StreamedAssistantContent::InteractionId(_) => {
            }
        }
    }

    let native_calls = tool_acc.finish();
    let calls = tools::resolve_tool_calls(native_calls, &text);
    Ok(AccumulatedResponse {
        text,
        reasoning,
        thought_signature,
        calls,
        usage,
    })
}
