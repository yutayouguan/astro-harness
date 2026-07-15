//! 会话消息模型：角色、正文、工具调用与工具结果。
//!
//! 供 agent / memory / backend 等共享，序列化时角色名为小写。

use serde::{Deserialize, Serialize};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// 角色。
    pub role: Role,
    /// 正文（纯文本或多段）。
    pub content: MessageContent,
    /// 助手发起的工具调用列表。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    /// 工具结果对应的调用 id。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

/// 消息正文：单段文本，或分段（多模态 text + image_url）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    /// 纯文本。
    Text(String),
    /// 多段内容（text / image_url）。
    Parts(Vec<ContentPart>),
}

/// 多段正文中的一段（对齐 OpenAI 兼容 vision：`type` + `text` 或 `image_url.url`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentPart {
    /// 段类型：`text` / `image_url`。
    #[serde(rename = "type")]
    pub kind: String,
    /// 文本载荷（`type=text`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// 图片 URL 或 `data:image/...;base64,...`（`type=image_url`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_url: Option<ContentImageUrl>,
}

/// `image_url` 载荷。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContentImageUrl {
    pub url: String,
}

impl ContentPart {
    /// 文本段。
    pub fn text(s: impl Into<String>) -> Self {
        Self {
            kind: "text".into(),
            text: Some(s.into()),
            image_url: None,
        }
    }

    /// 图片段（data URL 或 http(s)）。
    pub fn image_url(url: impl Into<String>) -> Self {
        Self {
            kind: "image_url".into(),
            text: None,
            image_url: Some(ContentImageUrl { url: url.into() }),
        }
    }
}

impl Message {
    /// 构造用户消息。
    pub fn user(content: &str) -> Self {
        Message {
            role: Role::User,
            content: MessageContent::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// 构造系统消息。
    pub fn system(content: &str) -> Self {
        Message {
            role: Role::System,
            content: MessageContent::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// 构造助手纯文本消息。
    pub fn assistant(content: &str) -> Self {
        Message {
            role: Role::Assistant,
            content: MessageContent::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// 构造带工具调用的助手消息。
    pub fn assistant_with_tools(content: &str, tool_calls: Vec<ToolCall>) -> Self {
        Message {
            role: Role::Assistant,
            content: MessageContent::Text(content.to_string()),
            tool_calls: Some(tool_calls),
            tool_call_id: None,
        }
    }

    /// 构造工具结果消息（无 call id）。
    pub fn tool(content: &str) -> Self {
        Message {
            role: Role::Tool,
            content: MessageContent::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// 构造绑定到指定 `tool_call_id` 的工具结果消息。
    pub fn tool_with_id(tool_call_id: &str, content: &str) -> Self {
        Message {
            role: Role::Tool,
            content: MessageContent::Text(content.to_string()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.to_string()),
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
            tool_calls: None,
            tool_call_id: None,
        }
    }

    /// 取可读文本：`Text` 全文；`Parts` 取首个 text 段（兼容旧调用）。
    pub fn content_str(&self) -> &str {
        match &self.content {
            MessageContent::Text(s) => s,
            MessageContent::Parts(parts) => parts
                .iter()
                .find_map(|p| p.text.as_deref())
                .unwrap_or(""),
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
}

/// 一次工具调用请求（助手侧）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// 调用 id，与工具结果对应。
    pub id: String,
    /// 工具名。
    pub name: String,
    /// JSON 参数。
    pub arguments: serde_json::Value,
}

/// 一次工具执行结果（工具侧）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// 对应的调用 id。
    pub tool_call_id: String,
    /// 工具名。
    pub name: String,
    /// 结果正文。
    pub content: String,
    /// 是否为错误结果。
    pub is_error: bool,
}
