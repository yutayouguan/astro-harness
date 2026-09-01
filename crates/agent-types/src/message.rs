//! 会话消息模型：角色、正文、工具调用与工具结果。
//!
//! 供 agent / memory / backend 等共享，序列化时角色名为小写。

use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::media::{MediaAsset, MediaKind};

/// 消息角色。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    /// 系统提示。
    System,
    /// 用户输入。
    User,
    /// 助手回复。
    Assistant,
    /// 工具执行结果。
    Tool,
}

/// 一条对话消息。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    /// 角色。
    pub role: Role,
    /// 正文（纯文本或多段）。
    pub content: MessageContent,
    /// 工具结果压缩后的发送视图；原文仍保留在 `content` 中。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compressed_content: Option<String>,
    /// 助手发起的工具调用列表。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// 工具结果对应的调用 id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    /// 结构化媒体附件（工具生成图/音/视频等）；发给 LLM 仍用 `content` 文本。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaAsset>,
    /// 助手推理文本（与 DB `reasoning` 列对应；运行时 hydrate 填充）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    /// Google Interactions `thought.signature`（存于 `reasoning_details.google_thought_signature`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thought_signature: Option<String>,
}

/// 消息正文：单段文本，或分段（多模态 text + image_url）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// 纯文本。
    Text(String),
    /// 多段内容（text / image_url）。
    Parts(Vec<ContentPart>),
}

/// 多段正文中的一段（text / image_url / audio_url / video_url）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentPart {
    /// 段类型：`text` / `image_url` / `audio_url` / `video_url`。
    #[serde(rename = "type")]
    pub kind: String,
    /// 文本载荷（`type=text`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// 图片 URL 或 `data:image/...;base64,...`（`type=image_url`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ContentImageUrl>,
    /// 音频 URL 或 data URL（`type=audio_url`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audio_url: Option<ContentMediaUrl>,
    /// 视频 URL 或 data URL（`type=video_url`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub video_url: Option<ContentMediaUrl>,
}

/// `image_url` 载荷。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentImageUrl {
    pub url: String,
}

/// `audio_url` / `video_url` 载荷。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContentMediaUrl {
    pub url: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub mime_type: String,
}

impl ContentPart {
    /// 文本段。
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            kind: "text".into(),
            text: Some(s.into()),
            image_url: None,
            audio_url: None,
            video_url: None,
        }
    }

    /// 图片段（data URL 或 http(s)）。
    pub fn image_url(url: impl Into<String>) -> Self {
        Self {
            kind: "image_url".into(),
            text: None,
            image_url: Some(ContentImageUrl { url: url.into() }),
            audio_url: None,
            video_url: None,
        }
    }

    /// 音频段。
    pub fn audio_url(url: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            kind: "audio_url".into(),
            text: None,
            image_url: None,
            audio_url: Some(ContentMediaUrl {
                url: url.into(),
                mime_type: mime_type.into(),
            }),
            video_url: None,
        }
    }

    /// 视频段。
    pub fn video_url(url: impl Into<String>, mime_type: impl Into<String>) -> Self {
        Self {
            kind: "video_url".into(),
            text: None,
            image_url: None,
            audio_url: None,
            video_url: Some(ContentMediaUrl {
                url: url.into(),
                mime_type: mime_type.into(),
            }),
        }
    }
}

impl Message {
    /// 构造用户消息。
    pub fn user(content: &str) -> Self {
        Message {
            role: Role::User,
            content: MessageContent::Text(content.to_string()),
            compressed_content: None,
            tool_calls: None,
            tool_call_id: None,
            media: Vec::new(),
            reasoning: None,
            thought_signature: None,
        }
    }

    /// 构造系统消息。
    pub fn system(content: &str) -> Self {
        Message {
            role: Role::System,
            content: MessageContent::Text(content.to_string()),
            compressed_content: None,
            tool_calls: None,
            tool_call_id: None,
            media: Vec::new(),
            reasoning: None,
            thought_signature: None,
        }
    }

    /// 构造助手纯文本消息。
    pub fn assistant(content: &str) -> Self {
        Message {
            role: Role::Assistant,
            content: MessageContent::Text(content.to_string()),
            compressed_content: None,
            tool_calls: None,
            tool_call_id: None,
            media: Vec::new(),
            reasoning: None,
            thought_signature: None,
        }
    }

    /// 构造带工具调用的助手消息。
    pub fn assistant_with_tools(content: &str, tool_calls: Vec<ToolCall>) -> Self {
        Message {
            role: Role::Assistant,
            content: MessageContent::Text(content.to_string()),
            compressed_content: None,
            tool_calls: Some(tool_calls),
            tool_call_id: None,
            media: Vec::new(),
            reasoning: None,
            thought_signature: None,
        }
    }

    /// 构造工具结果消息（无 call id）。
    pub fn tool(content: &str) -> Self {
        Message {
            role: Role::Tool,
            content: MessageContent::Text(content.to_string()),
            compressed_content: None,
            tool_calls: None,
            tool_call_id: None,
            media: Vec::new(),
            reasoning: None,
            thought_signature: None,
        }
    }

    /// 构造绑定到指定 `tool_call_id` 的工具结果消息。
    pub fn tool_with_id(tool_call_id: &str, content: &str) -> Self {
        Message {
            role: Role::Tool,
            content: MessageContent::Text(content.to_string()),
            compressed_content: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
            media: Vec::new(),
            reasoning: None,
            thought_signature: None,
        }
    }

    /// 构造带结构化媒体的工具结果消息。
    pub fn tool_with_media(tool_call_id: &str, content: &str, media: Vec<MediaAsset>) -> Self {
        Message {
            role: Role::Tool,
            content: MessageContent::Text(content.to_string()),
            compressed_content: None,
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
            media,
            reasoning: None,
            thought_signature: None,
        }
    }

    /// 构造带图片 parts 的用户消息（OpenAI 兼容多模态）。
    pub fn user_with_images(text: &str, image_data_urls: &[String]) -> Self {
        if image_data_urls.is_empty() {
            return Self::user(text);
        }
        let mut parts = Vec::new();
        let t = text.trim();
        if !t.is_empty() {
            parts.push(ContentPart::text(t));
        }
        for url in image_data_urls {
            let u = url.trim();
            if !u.is_empty() {
                parts.push(ContentPart::image_url(u));
            }
        }
        if parts.is_empty() {
            return Self::user(text);
        }
        // 若无文本仅有图，补一句默认提示，避免部分模型拒空 text
        if parts.iter().all(|p| p.kind != "text") {
            parts.insert(0, ContentPart::text("请结合附图回答。"));
        }
        Message {
            role: Role::User,
            content: MessageContent::Parts(parts),
            compressed_content: None,
            tool_calls: None,
            tool_call_id: None,
            media: image_data_urls
                .iter()
                .map(|u| u.trim())
                .filter(|u| !u.is_empty())
                .map(|u| {
                    let mime = u
                        .strip_prefix("data:")
                        .and_then(|rest| rest.split(';').next())
                        .unwrap_or("image/*")
                        .to_string();
                    MediaAsset::data_url(MediaKind::Image, u, mime)
                })
                .collect(),
            reasoning: None,
            thought_signature: None,
        }
    }

    /// 取可读文本：`Text` 全文；`Parts` 取首个 text 段（兼容旧调用）。
    pub fn content_str(&self) -> &str {
        match &self.content {
            MessageContent::Text(s) => s,
            MessageContent::Parts(parts) => {
                parts.iter().find_map(|p| p.text.as_deref()).unwrap_or("")
            }
        }
    }

    /// 拼接 Parts 中全部文本段（FTS / 记录用）。
    pub fn content_text(&self) -> String {
        match &self.content {
            MessageContent::Text(s) => s.clone(),
            MessageContent::Parts(parts) => parts
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    /// 面向模型的语义消费者可见的文本。
    ///
    /// 只有工具结果可以用压缩后的 provider 视图替代原文。其他
    /// role 可能将 `compressed_content` 用作内部元数据，因此其
    /// 语义内容必须始终来自 `content`。
    pub fn provider_view_text(&self) -> Cow<'_, str> {
        if self.role == Role::Tool {
            if let Some(compressed) = self
                .compressed_content
                .as_deref()
                .filter(|content| !content.trim().is_empty())
            {
                return Cow::Borrowed(compressed);
            }
        }
        match &self.content {
            MessageContent::Text(content) => Cow::Borrowed(content),
            MessageContent::Parts(_) => Cow::Owned(self.content_text()),
        }
    }
}

/// 一次工具调用请求（助手侧）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// 调用 id，与工具结果对应。
    pub id: String,
    /// 工具名。
    pub name: String,
    /// JSON 参数。
    pub arguments: serde_json::Value,
    /// Google Interactions / Gemini 3：`function_call.signature`，无状态回放时必须原样回传。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// 一次工具执行结果（工具侧）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// 对应的调用 id。
    pub tool_call_id: String,
    /// 工具名。
    pub name: String,
    /// 结果正文（LLM 可读摘要；可含路径文案）。
    pub content: String,
    /// 结构化媒体附件。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub media: Vec<MediaAsset>,
    /// 是否为错误结果。
    pub is_error: bool,
}

impl ToolResult {
    pub fn text(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            tool_call_id: tool_call_id.into(),
            name: name.into(),
            content: content.into(),
            media: Vec::new(),
            is_error: false,
        }
    }

    pub fn with_media(mut self, media: Vec<MediaAsset>) -> Self {
        self.media = media;
        self
    }

    pub fn error(mut self) -> Self {
        self.is_error = true;
        self
    }
}

/// `reasoning_details` 中存放 Google Interactions `thought.signature` 的键。
pub const GOOGLE_THOUGHT_SIGNATURE_KEY: &str = "google_thought_signature";

/// 将 `thought.signature` 合并进 `reasoning_details`（供会话落盘）。
pub fn merge_google_thought_signature(
    details: Option<serde_json::Value>,
    signature: Option<&str>,
) -> Option<serde_json::Value> {
    let sig = signature.map(str::trim).filter(|s| !s.is_empty())?;
    let mut obj = match details {
        Some(serde_json::Value::Object(m)) => m,
        _ => serde_json::Map::new(),
    };
    obj.insert(
        GOOGLE_THOUGHT_SIGNATURE_KEY.into(),
        serde_json::Value::String(sig.to_string()),
    );
    Some(serde_json::Value::Object(obj))
}

/// 从 `reasoning_details` 读出 Google `thought.signature`。
pub fn google_thought_signature_from_details(
    details: &Option<serde_json::Value>,
) -> Option<String> {
    details
        .as_ref()?
        .get(GOOGLE_THOUGHT_SIGNATURE_KEY)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{Message, ToolCall};

    #[test]
    fn messages_compare_structurally_through_nested_tool_calls() {
        let message = Message::assistant_with_tools(
            "run it",
            vec![ToolCall {
                id: "call-1".into(),
                name: "exec_command".into(),
                arguments: serde_json::json!({"command": "pwd"}),
                signature: Some("sig-1".into()),
            }],
        );
        let equal = message.clone();
        let mut changed = message.clone();
        changed.tool_calls.as_mut().unwrap()[0].arguments = serde_json::json!({"command": "ls"});

        assert_eq!(message, equal);
        assert_ne!(message, changed);
    }
}
