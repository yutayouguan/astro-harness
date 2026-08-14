//! 统一消息模型 — 多模态、工具调用、thinking 内建。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Tool => "tool",
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// 用户消息中的一个内容块。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UserContent {
    Text {
        text: String,
    },
    Image {
        url: String,
    },
    Audio {
        url: String,
        mime_type: String,
    },
    Video {
        url: String,
        mime_type: String,
    },
    Document {
        url: String,
        mime_type: String,
    },
    ToolResult {
        tool_call_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

/// 助手消息中的一个内容块。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AssistantContent {
    Text {
        text: String,
    },
    ToolCall(ToolCall),
    Thinking {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        signature: Option<String>,
    },
}

/// 单次工具调用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// 工具定义（provider-agnostic JSON Schema）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

/// 统一消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System {
        content: String,
    },
    User {
        content: Vec<UserContent>,
    },
    Assistant {
        content: Vec<AssistantContent>,
    },
    Tool {
        tool_call_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Self::System {
            content: content.into(),
        }
    }

    pub fn user_text(text: impl Into<String>) -> Self {
        Self::User {
            content: vec![UserContent::Text { text: text.into() }],
        }
    }

    pub fn user(content: Vec<UserContent>) -> Self {
        Self::User { content }
    }

    pub fn assistant_text(text: impl Into<String>) -> Self {
        Self::Assistant {
            content: vec![AssistantContent::Text { text: text.into() }],
        }
    }

    pub fn assistant(content: Vec<AssistantContent>) -> Self {
        Self::Assistant { content }
    }

    pub fn tool_result(
        tool_call_id: impl Into<String>,
        content: impl Into<String>,
        is_error: bool,
    ) -> Self {
        Self::Tool {
            tool_call_id: tool_call_id.into(),
            content: content.into(),
            is_error,
        }
    }

    pub fn role(&self) -> Role {
        match self {
            Self::System { .. } => Role::System,
            Self::User { .. } => Role::User,
            Self::Assistant { .. } => Role::Assistant,
            Self::Tool { .. } => Role::Tool,
        }
    }

    pub fn text_content(&self) -> &str {
        match self {
            Self::System { content } => content,
            Self::Tool { content, .. } => content,
            Self::User { content } => content
                .iter()
                .find_map(|c| match c {
                    UserContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .unwrap_or(""),
            Self::Assistant { content } => content
                .iter()
                .find_map(|c| match c {
                    AssistantContent::Text { text } => Some(text.as_str()),
                    _ => None,
                })
                .unwrap_or(""),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_constructors() {
        let sys = Message::system("You are helpful.");
        assert_eq!(sys.role(), Role::System);
        assert_eq!(sys.text_content(), "You are helpful.");

        let user = Message::user_text("Hello");
        assert_eq!(user.role(), Role::User);
        assert_eq!(user.text_content(), "Hello");

        let asst = Message::assistant_text("Hi there");
        assert_eq!(asst.role(), Role::Assistant);
        assert_eq!(asst.text_content(), "Hi there");

        let tool = Message::tool_result("call_1", "result", false);
        assert_eq!(tool.role(), Role::Tool);
        assert_eq!(tool.text_content(), "result");
    }

    #[test]
    fn multimodal_user_message() {
        let msg = Message::user(vec![
            UserContent::Text {
                text: "Look at this".into(),
            },
            UserContent::Image {
                url: "data:image/png;base64,abc".into(),
            },
            UserContent::Document {
                url: "file.pdf".into(),
                mime_type: "application/pdf".into(),
            },
        ]);
        assert_eq!(msg.role(), Role::User);
        if let Message::User { content } = &msg {
            assert_eq!(content.len(), 3);
        }
    }

    #[test]
    fn assistant_with_thinking_and_tool_call() {
        let msg = Message::assistant(vec![
            AssistantContent::Thinking {
                text: "Let me think...".into(),
                signature: Some("sig123".into()),
            },
            AssistantContent::Text {
                text: "The answer is 42.".into(),
            },
            AssistantContent::ToolCall(ToolCall {
                id: "call_1".into(),
                name: "read_file".into(),
                arguments: serde_json::json!({"path": "foo.rs"}),
                signature: None,
            }),
        ]);
        assert_eq!(msg.text_content(), "The answer is 42.");
    }

    #[test]
    fn role_serialization() {
        assert_eq!(serde_json::to_string(&Role::System).unwrap(), "\"system\"");
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), "\"tool\"");
    }
}
