//! 新分发入口 — 基于 trait 系统的 `chat_stream_for_provider` 替代。
//!
//! 直接接受新 `Message` 和 `CompletionRequest`，无需 bridge。
//! 保留旧签名兼容函数 `chat_stream_new`（内联转换）。

use anyhow::Result;
use futures::StreamExt;
use serde_json::Value;

use crate::types::message::{
    AssistantContent, Message, ToolCall, ToolDefinition, UserContent,
};
use crate::types::request::{CompletionRequest, ProviderConfig, ThinkingConfig};
use crate::types::stream::{CompletionStream, StreamChunk};
use crate::trait_::{ChatMessage, ChatStream, ChatToolCall, ChatContentPart, ChatChunk, ToolCallDeltaChunk};

/// 新管线分发（新类型签名）。
///
/// 直接接受 `Vec<Message>` 和 `CompletionRequest`，返回新 `CompletionStream`。
pub async fn chat_stream_direct(
    provider: &str,
    request: CompletionRequest,
    config: &ProviderConfig,
) -> Result<CompletionStream> {
    let mut reg = crate::new_registry::NewRegistry::new();
    register_provider(&mut reg, provider, config);

    let dyn_model = reg
        .completion_model(provider)
        .ok_or_else(|| anyhow::anyhow!("未知 provider: {provider}"))?;

    dyn_model.stream(request).await
}

/// 新管线分发（旧签名兼容）。
///
/// 将旧 `ChatMessage` 内联转为新 `Message`，返回旧 `ChatStream`。
/// 不依赖 bridge 模块。
pub async fn chat_stream_new(
    provider: &str,
    messages: Vec<ChatMessage>,
    tools: Vec<Value>,
    config: &ProviderConfig,
) -> Result<ChatStream> {
    // 1. 旧消息 → 新消息（内联转换）
    let new_messages: Vec<Message> = messages
        .iter()
        .map(legacy_to_message_inline)
        .collect();

    // 2. 旧工具 → 新工具定义
    let new_tools: Vec<ToolDefinition> = tools
        .iter()
        .filter_map(|t| {
            let f = t.get("function").unwrap_or(t);
            Some(ToolDefinition {
                name: f.get("name")?.as_str()?.to_string(),
                description: f
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string(),
                parameters: f
                    .get("parameters")
                    .cloned()
                    .unwrap_or(serde_json::json!({"type": "object", "properties": {}})),
            })
        })
        .collect();

    // 3. 构造 CompletionRequest
    let request = CompletionRequest {
        model: config.model.clone(),
        messages: new_messages,
        tools: new_tools,
        temperature: Some(config.temperature),
        max_tokens: Some(config.max_tokens),
        thinking: if config.thinking_enabled {
            Some(ThinkingConfig {
                enabled: true,
                budget_tokens: None,
                effort: config.reasoning_effort.clone(),
            })
        } else {
            None
        },
        additional_params: config.additional_params.clone(),
        previous_interaction_id: config.previous_interaction_id.clone(),
    };

    // 4. 通过新管线获取 CompletionStream
    let new_stream = chat_stream_direct(provider, request, config).await?;

    // 5. 新 StreamChunk → 旧 ChatChunk（内联转换）
    let legacy_stream = new_stream.filter_map(|result| async move {
        match result {
            Ok(chunk) => stream_chunk_to_legacy_inline(&chunk).map(Ok),
            Err(e) => Some(Err(e)),
        }
    });

    Ok(Box::pin(legacy_stream))
}

// ── 内联转换函数（不依赖 bridge 模块） ──

/// 旧 `ChatMessage` → 新 `Message`（内联版本）。
fn legacy_to_message_inline(old: &ChatMessage) -> Message {
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
                    content.push(AssistantContent::ToolCall(ToolCall {
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
            // user or unknown → user
            let mut parts = Vec::new();
            if let Some(ref old_parts) = old.parts {
                for p in old_parts {
                    match p {
                        ChatContentPart::Text { text } => {
                            parts.push(UserContent::Text { text: text.clone() });
                        }
                        ChatContentPart::ImageUrl { url } => {
                            parts.push(UserContent::Image { url: url.clone() });
                        }
                        ChatContentPart::AudioUrl { url, mime_type } => {
                            parts.push(UserContent::Audio {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                        ChatContentPart::VideoUrl { url, mime_type } => {
                            parts.push(UserContent::Video {
                                url: url.clone(),
                                mime_type: mime_type.clone(),
                            });
                        }
                        ChatContentPart::DocumentUrl { url, mime_type } => {
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

/// 新 `StreamChunk` → 旧 `ChatChunk`（内联版本）。
fn stream_chunk_to_legacy_inline(chunk: &StreamChunk) -> Option<ChatChunk> {
    match chunk {
        StreamChunk::Text(t) => Some(ChatChunk {
            token: Some(t.clone()),
            ..Default::default()
        }),
        StreamChunk::Thinking(t) => Some(ChatChunk {
            reasoning: Some(t.clone()),
            ..Default::default()
        }),
        StreamChunk::ThoughtSignature(s) => Some(ChatChunk {
            thought_signature: Some(s.clone()),
            ..Default::default()
        }),
        StreamChunk::ToolCallStart { index, id, name } => Some(ChatChunk {
            tool_call_deltas: vec![ToolCallDeltaChunk {
                index: *index,
                id: Some(id.clone()),
                name: Some(name.clone()),
                arguments: None,
                signature: None,
            }],
            ..Default::default()
        }),
        StreamChunk::ToolCallDelta { index, arguments } => Some(ChatChunk {
            tool_call_deltas: vec![ToolCallDeltaChunk {
                index: *index,
                id: None,
                name: None,
                arguments: Some(arguments.clone()),
                signature: None,
            }],
            ..Default::default()
        }),
        StreamChunk::Usage(u) => Some(ChatChunk {
            usage: Some(crate::streaming::Usage {
                input_tokens: u.input_tokens,
                output_tokens: u.output_tokens,
                cache_read_tokens: u.cache_read_tokens,
                cache_write_tokens: u.cache_write_tokens,
                reasoning_tokens: u.reasoning_tokens,
                request_count: u.request_count,
            }),
            ..Default::default()
        }),
        StreamChunk::Citation(v) => Some(ChatChunk {
            citations: Some(vec![v.clone()]),
            ..Default::default()
        }),
        StreamChunk::Done { finish_reason } => Some(ChatChunk {
            finish_reason: Some(finish_reason.clone()),
            ..Default::default()
        }),
        StreamChunk::Error(msg) => Some(ChatChunk {
            finish_reason: Some(format!("error:{msg}")),
            ..Default::default()
        }),
        StreamChunk::InteractionId(id) => Some(ChatChunk {
            interaction_id: Some(id.clone()),
            ..Default::default()
        }),
    }
}

/// 根据 provider id 注册到新注册表。
fn register_provider(
    reg: &mut crate::new_registry::NewRegistry,
    provider: &str,
    config: &ProviderConfig,
) {
    let key = &config.api_key;
    let base = config.base_url.as_deref().filter(|s| !s.trim().is_empty());
    let model = &config.model;

    match provider {
        "anthropic" | "claude" => reg.register_anthropic(key, base, model),
        "google" => reg.register_google(key, base, model),
        // OpenAI 兼容厂商 — 使用泛型方法
        "openai" => register_compat::<crate::impls::openai::OpenAI>(reg, key, base, model),
        "deepseek" => register_compat::<crate::impls::deepseek::DeepSeek>(reg, key, base, model),
        "azure" => register_compat::<crate::impls::azure::Azure>(reg, key, base, model),
        "zhipu" => register_compat::<crate::impls::zhipu::Zhipu>(reg, key, base, model),
        "moonshot" => register_compat::<crate::impls::moonshot::Moonshot>(reg, key, base, model),
        "ollama" => register_compat::<crate::impls::ollama::Ollama>(reg, key, base, model),
        "nvidia" => register_compat::<crate::impls::nvidia::Nvidia>(reg, key, base, model),
        "bailian" => register_compat::<crate::impls::bailian::Bailian>(reg, key, base, model),
        "volcengine" => register_compat::<crate::impls::volcengine::Volcengine>(reg, key, base, model),
        "openrouter" => register_compat::<crate::impls::openrouter::OpenRouter>(reg, key, base, model),
        "minimax" | "minmax" => register_compat::<crate::impls::minimax_new::MiniMaxNew>(reg, key, base, model),
        "hunyuan" => register_compat::<crate::impls::hunyuan::Hunyuan>(reg, key, base, model),
        // 未知 → OpenAI 兼容回退
        _ => register_compat::<crate::impls::openai::OpenAI>(reg, key, base, model),
    }
}

fn register_compat<Ext>(
    reg: &mut crate::new_registry::NewRegistry,
    api_key: &str,
    base_url: Option<&str>,
    model: &str,
)
where
    Ext: crate::compat::OpenAICompatible
        + crate::traits::ProviderExt
        + crate::traits::Capabilities<Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Ext>>>
        + Copy
        + 'static,
{
    reg.register_openai_compat::<Ext>(api_key, base_url, model);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_all_providers() {
        let config = ProviderConfig::default();
        let providers = [
            "openai", "anthropic", "claude", "deepseek", "google",
            "azure", "zhipu", "moonshot", "ollama", "nvidia",
            "bailian", "volcengine", "openrouter", "minimax", "hunyuan",
        ];
        for id in providers {
            let mut reg = crate::new_registry::NewRegistry::new();
            register_provider(&mut reg, id, &config);
            assert!(
                reg.completion_model(id).is_some(),
                "provider {id} should have completion model"
            );
        }
    }

    #[test]
    fn legacy_conversion_roundtrip() {
        let old = ChatMessage::text("system", "Be helpful");
        let new = legacy_to_message_inline(&old);
        assert_eq!(new.text_content(), "Be helpful");
    }
}
