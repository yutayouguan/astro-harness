use agent_protocol::{ContentItem, ResponseItem};
use serde_json::Result;
use types::message::{ContentPart, Message, MessageContent, Role, ToolCall};
use types::{MediaKind, MediaRef};

use crate::RolloutItem;

#[derive(Debug, Clone, PartialEq)]
pub struct ReconstructedMessage {
    pub message: Message,
    pub tool_name: Option<String>,
}

/// Convert a runtime chat projection into Codex-native response items.
pub fn response_items_from_message(
    message: &Message,
    tool_name: Option<&str>,
) -> Result<Vec<ResponseItem>> {
    match message.role {
        Role::Tool => tool_output_items(message, tool_name),
        Role::Assistant => assistant_items(message),
        Role::User | Role::System => Ok(vec![ResponseItem::Message {
            id: None,
            role: role_name(&message.role).to_string(),
            content: content_items(message),
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        }]),
    }
}

pub fn reconstruct_messages(items: &[RolloutItem]) -> Result<Vec<ReconstructedMessage>> {
    let response_items = items.iter().filter_map(|item| match item {
        RolloutItem::ResponseItem(item) => Some(item.clone()),
        _ => None,
    });
    reconstruct_response_items(response_items)
}

/// Build the legacy/UI message projection from native Responses items.
///
/// Agent execution must retain and consume the original [`ResponseItem`] list;
/// this projection exists only for compatibility consumers such as the
/// conversation index and current desktop timeline.
pub fn reconstruct_response_items(
    items: impl IntoIterator<Item = ResponseItem>,
) -> Result<Vec<ReconstructedMessage>> {
    let mut messages = Vec::new();
    let mut pending_assistant: Option<Message> = None;

    let flush_assistant = |messages: &mut Vec<ReconstructedMessage>,
                           pending: &mut Option<Message>| {
        if let Some(message) = pending.take() {
            messages.push(ReconstructedMessage {
                message,
                tool_name: None,
            });
        }
    };

    for item in items {
        match &item {
            ResponseItem::Message { role, content, .. } if role == "assistant" => {
                let incoming = message_from_content(Role::Assistant, content);
                let pending = pending_assistant.get_or_insert_with(|| Message::assistant(""));
                append_message_content(pending, &incoming);
            }
            ResponseItem::Message { role, content, .. } => {
                flush_assistant(&mut messages, &mut pending_assistant);
                let role = match role.as_str() {
                    "user" => Role::User,
                    "system" | "developer" => Role::System,
                    _ => continue,
                };
                messages.push(ReconstructedMessage {
                    message: message_from_content(role, content),
                    tool_name: None,
                });
            }
            ResponseItem::Reasoning {
                summary, content, ..
            } => {
                let text = reasoning_text(summary, content.as_deref());
                if !text.is_empty() {
                    let pending = pending_assistant.get_or_insert_with(|| Message::assistant(""));
                    pending.reasoning = Some(text);
                }
            }
            ResponseItem::FunctionCall {
                name,
                arguments,
                call_id,
                ..
            } => {
                let pending = pending_assistant.get_or_insert_with(|| Message::assistant(""));
                pending
                    .tool_calls
                    .get_or_insert_with(Vec::new)
                    .push(ToolCall {
                        id: call_id.clone(),
                        name: name.clone(),
                        arguments: serde_json::from_str(arguments)
                            .unwrap_or_else(|_| serde_json::Value::String(arguments.clone())),
                        signature: None,
                    });
            }
            ResponseItem::ToolSearchCall {
                call_id: Some(call_id),
                arguments,
                ..
            } => {
                let pending = pending_assistant.get_or_insert_with(|| Message::assistant(""));
                pending
                    .tool_calls
                    .get_or_insert_with(Vec::new)
                    .push(ToolCall {
                        id: call_id.clone(),
                        name: "tool_search".into(),
                        arguments: arguments.clone(),
                        signature: None,
                    });
            }
            ResponseItem::CustomToolCall {
                call_id,
                name,
                input,
                ..
            } => {
                let pending = pending_assistant.get_or_insert_with(|| Message::assistant(""));
                pending
                    .tool_calls
                    .get_or_insert_with(Vec::new)
                    .push(ToolCall {
                        id: call_id.clone(),
                        name: name.clone(),
                        arguments: serde_json::Value::String(input.clone()),
                        signature: None,
                    });
            }
            ResponseItem::FunctionCallOutput {
                call_id,
                name,
                output,
                ..
            } => {
                flush_assistant(&mut messages, &mut pending_assistant);
                messages.push(tool_message(
                    call_id.as_deref(),
                    output_text(output),
                    name.clone(),
                ));
            }
            ResponseItem::CustomToolCallOutput {
                call_id,
                name,
                output,
                ..
            } => {
                flush_assistant(&mut messages, &mut pending_assistant);
                messages.push(tool_message(
                    Some(call_id),
                    output_text(output),
                    name.clone(),
                ));
            }
            ResponseItem::ToolSearchOutput { call_id, tools, .. } => {
                flush_assistant(&mut messages, &mut pending_assistant);
                messages.push(tool_message(
                    call_id.as_deref(),
                    serde_json::to_string(tools)?,
                    Some("tool_search".into()),
                ));
            }
            _ => {}
        }
    }
    flush_assistant(&mut messages, &mut pending_assistant);
    Ok(messages)
}

fn assistant_items(message: &Message) -> Result<Vec<ResponseItem>> {
    let mut items = Vec::new();
    if let Some(reasoning) = message
        .reasoning
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        items.push(ResponseItem::Reasoning {
            id: None,
            summary: Vec::new(),
            content: Some(vec![serde_json::json!({
                "type": "reasoning_text",
                "text": reasoning,
            })]),
            encrypted_content: None,
            internal_chat_message_metadata_passthrough: None,
        });
    }
    if !message.content_text().is_empty() || message.tool_calls.as_ref().is_none_or(Vec::is_empty) {
        items.push(ResponseItem::Message {
            id: None,
            role: "assistant".into(),
            content: content_items(message),
            phase: None,
            internal_chat_message_metadata_passthrough: None,
        });
    }
    for call in message.tool_calls.iter().flatten() {
        if call.name == "tool_search" {
            items.push(ResponseItem::ToolSearchCall {
                id: None,
                call_id: Some(call.id.clone()),
                status: Some("completed".into()),
                execution: "client".into(),
                arguments: call.arguments.clone(),
                internal_chat_message_metadata_passthrough: None,
            });
        } else {
            items.push(ResponseItem::FunctionCall {
                id: None,
                name: call.name.clone(),
                namespace: None,
                arguments: serde_json::to_string(&call.arguments)?,
                encrypted_function_args: None,
                call_id: call.id.clone(),
                internal_chat_message_metadata_passthrough: None,
            });
        }
    }
    Ok(items)
}

fn tool_output_items(message: &Message, tool_name: Option<&str>) -> Result<Vec<ResponseItem>> {
    let call_id = message.tool_call_id.clone();
    if tool_name == Some("tool_search") {
        let tools = serde_json::from_str::<Vec<serde_json::Value>>(&message.content_text())
            .unwrap_or_default();
        return Ok(vec![ResponseItem::ToolSearchOutput {
            id: None,
            call_id,
            status: "completed".into(),
            execution: "client".into(),
            tools,
            internal_chat_message_metadata_passthrough: None,
        }]);
    }
    Ok(vec![ResponseItem::FunctionCallOutput {
        id: None,
        call_id,
        name: tool_name.map(str::to_string),
        namespace: None,
        output: serde_json::Value::String(message.content_text()),
        internal_chat_message_metadata_passthrough: None,
    }])
}

fn role_name(role: &Role) -> &'static str {
    match role {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn content_items(message: &Message) -> Vec<ContentItem> {
    let output = matches!(message.role, Role::Assistant);
    let mut items = match &message.content {
        MessageContent::Text(text) => vec![text_item(text.clone(), output)],
        MessageContent::Parts(parts) => parts
            .iter()
            .filter_map(|part| match part.kind.as_str() {
                "text" => part.text.clone().map(|text| text_item(text, output)),
                "image_url" => part
                    .image_url
                    .as_ref()
                    .map(|image| ContentItem::InputImage {
                        image_url: image.url.clone(),
                        detail: None,
                    }),
                "audio_url" => part
                    .audio_url
                    .as_ref()
                    .map(|audio| ContentItem::InputAudio {
                        audio_url: audio.url.clone(),
                    }),
                "video_url" => part.video_url.as_ref().map(|video| ContentItem::InputText {
                    text: video.url.clone(),
                }),
                _ => None,
            })
            .collect(),
    };
    for media in &message.media {
        let url = match &media.reference {
            MediaRef::DataUrl(value) | MediaRef::RemoteUri(value) => value,
            MediaRef::WorkspacePath(_) => continue,
        };
        let item = match media.kind {
            MediaKind::Image => ContentItem::InputImage {
                image_url: url.clone(),
                detail: None,
            },
            MediaKind::Audio => ContentItem::InputAudio {
                audio_url: url.clone(),
            },
            MediaKind::Video | MediaKind::File => continue,
        };
        let duplicate = items.iter().any(|existing| match existing {
            ContentItem::InputImage { image_url, .. } => image_url == url,
            ContentItem::InputAudio { audio_url } => audio_url == url,
            _ => false,
        });
        if !duplicate {
            items.push(item);
        }
    }
    items
}

fn text_item(text: String, output: bool) -> ContentItem {
    if output {
        ContentItem::OutputText { text }
    } else {
        ContentItem::InputText { text }
    }
}

fn message_from_content(role: Role, content: &[ContentItem]) -> Message {
    let mut parts = Vec::new();
    for item in content {
        match item {
            ContentItem::InputText { text } | ContentItem::OutputText { text } => {
                parts.push(ContentPart::text(text.clone()));
            }
            ContentItem::InputImage { image_url, .. } => {
                parts.push(ContentPart::image_url(image_url.clone()));
            }
            ContentItem::InputAudio { audio_url } => {
                parts.push(ContentPart::audio_url(audio_url.clone(), "audio/*"));
            }
        }
    }
    let content = if parts.len() == 1 && parts[0].kind == "text" {
        MessageContent::Text(parts[0].text.clone().unwrap_or_default())
    } else {
        MessageContent::Parts(parts)
    };
    Message {
        role,
        content,
        compressed_content: None,
        tool_calls: None,
        tool_call_id: None,
        media: Vec::new(),
        reasoning: None,
        thought_signature: None,
    }
}

fn append_message_content(target: &mut Message, incoming: &Message) {
    let incoming = incoming.content_text();
    if incoming.is_empty() {
        return;
    }
    let current = target.content_text();
    target.content = MessageContent::Text(if current.is_empty() {
        incoming
    } else {
        format!("{current}\n{incoming}")
    });
}

fn reasoning_text(summary: &[serde_json::Value], content: Option<&[serde_json::Value]>) -> String {
    content
        .unwrap_or(summary)
        .iter()
        .filter_map(|value| value.get("text").and_then(serde_json::Value::as_str))
        .collect::<Vec<_>>()
        .join("\n")
}

fn output_text(output: &serde_json::Value) -> String {
    match output {
        serde_json::Value::String(text) => text.clone(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn tool_message(
    call_id: Option<&str>,
    content: String,
    tool_name: Option<String>,
) -> ReconstructedMessage {
    let message = call_id
        .filter(|id| !id.is_empty())
        .map(|id| Message::tool_with_id(id, &content))
        .unwrap_or_else(|| Message::tool(&content));
    ReconstructedMessage { message, tool_name }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_search_round_trip_preserves_native_item_kind() {
        let assistant = Message::assistant_with_tools(
            "",
            vec![ToolCall {
                id: "call-1".into(),
                name: "tool_search".into(),
                arguments: serde_json::json!({"query": "image"}),
                signature: None,
            }],
        );
        let output = Message::tool_with_id("call-1", "[{\"name\":\"image_gen\"}]");
        let items = response_items_from_message(&assistant, None)
            .unwrap()
            .into_iter()
            .chain(response_items_from_message(&output, Some("tool_search")).unwrap())
            .map(RolloutItem::ResponseItem)
            .collect::<Vec<_>>();

        assert!(matches!(
            items[0],
            RolloutItem::ResponseItem(ResponseItem::ToolSearchCall { .. })
        ));
        assert!(matches!(
            items[1],
            RolloutItem::ResponseItem(ResponseItem::ToolSearchOutput { .. })
        ));
        let rebuilt = reconstruct_messages(&items).unwrap();
        assert_eq!(
            rebuilt[0].message.tool_calls.as_ref().unwrap()[0].name,
            "tool_search"
        );
        assert_eq!(rebuilt[1].tool_name.as_deref(), Some("tool_search"));
    }
}
