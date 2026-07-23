//! Anthropic Token Counting API。
//!
//! 端点：`POST /v1/messages/count_tokens`

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};

use super::defaults;
use super::tools::openai_tools_to_anthropic;
use crate::http_stream::{resolve_base, trim_slash};
use crate::types::ProviderConfig;
use crate::trait_::ChatMessage;
use crate::types::message::{
    AssistantContent, Message, ToolCall as NewToolCall, UserContent,
};

/// 旧 `ChatMessage` → 新 `Message`（内联转换，不依赖 bridge）。
fn legacy_to_message(old: &ChatMessage) -> Message {
    match old.role.as_str() {
        "system" => Message::system(&old.content),
        "tool" => Message::Tool {
            tool_call_id: old.tool_call_id.clone().unwrap_or_default(),
            content: old.content.clone(),
            is_error: old.is_error,
        },
        "assistant" => {
            let mut content = Vec::new();
            if let (Some(reasoning), sig) = (&old.reasoning, &old.thought_signature) {
                content.push(AssistantContent::Thinking {
                    text: reasoning.clone(),
                    signature: sig.clone(),
                });
            } else if let Some(sig) = &old.thought_signature {
                content.push(AssistantContent::Thinking {
                    text: String::new(),
                    signature: Some(sig.clone()),
                });
            }
            if !old.content.is_empty() {
                content.push(AssistantContent::Text {
                    text: old.content.clone(),
                });
            }
            if let Some(ref calls) = old.tool_calls {
                for c in calls {
                    content.push(AssistantContent::ToolCall(NewToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                        signature: c.signature.clone(),
                    }));
                }
            }
            if content.is_empty() {
                content.push(AssistantContent::Text {
                    text: String::new(),
                });
            }
            Message::Assistant { content }
        }
        _ => {
            let mut parts = Vec::new();
            if let Some(ref old_parts) = old.parts {
                for p in old_parts {
                    match p {
                        crate::trait_::ChatContentPart::Text { text } => {
                            parts.push(UserContent::Text { text: text.clone() });
                        }
                        crate::trait_::ChatContentPart::ImageUrl { url } => {
                            parts.push(UserContent::Image { url: url.clone() });
                        }
                        crate::trait_::ChatContentPart::AudioUrl { url, mime_type } => {
                            parts.push(UserContent::Audio {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                        crate::trait_::ChatContentPart::VideoUrl { url, mime_type } => {
                            parts.push(UserContent::Video {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                        crate::trait_::ChatContentPart::DocumentUrl { url, mime_type } => {
                            parts.push(UserContent::Document {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                    }
                }
            }
            if parts.is_empty() {
                parts.push(UserContent::Text {
                    text: old.content.clone(),
                });
            }
            Message::User { content: parts }
        }
    }
}

/// 统计消息和工具的 token 数。
pub async fn anthropic_count_tokens(
    client: &Client,
    messages: &[ChatMessage],
    tools: &[Value],
    config: &ProviderConfig,
) -> Result<u32> {
    let base = trim_slash(&resolve_base(config, "claude"));
    let url = format!("{base}/v1/messages/count_tokens");

    // 旧 ChatMessage → 新 Message（内联转换，不依赖 bridge）
    let new_msgs: Vec<Message> = messages
        .iter()
        .map(legacy_to_message)
        .collect();
    let (system, api_messages) =
        crate::impls::anthropic::to_anthropic_messages_public(&new_msgs);

    let mut body = json!({
        "model": config.model,
        "messages": api_messages,
    });
    if !system.is_null() {
        body["system"] = system;
    }
    let anthropic_tools = openai_tools_to_anthropic(tools);
    if !anthropic_tools.is_empty() {
        body["tools"] = Value::Array(anthropic_tools);
    }

    let resp = client
        .post(&url)
        .header("x-api-key", &config.api_key)
        .header("anthropic-version", defaults::ANTHROPIC_VERSION)
        .header("anthropic-beta", defaults::ANTHROPIC_BETA)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await
        .context("连接 Anthropic Count Tokens 失败")?;
    let status = resp.status();
    let v: Value = resp
        .json()
        .await
        .context("解析 Count Tokens 响应失败")?;
    if !status.is_success() {
        let msg = v
            .pointer("/error/message")
            .and_then(|m| m.as_str())
            .unwrap_or("未知错误");
        anyhow::bail!("Anthropic Count Tokens HTTP {status}: {msg}");
    }

    v.get("input_tokens")
        .and_then(|t| t.as_u64())
        .map(|t| t as u32)
        .context("Count Tokens 响应缺少 input_tokens")
}
