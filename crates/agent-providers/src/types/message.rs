//! 统一消息模型 — 多模态、工具调用、thinking 内建。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 消息角色。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    System,
    Developer,
    User,
    Assistant,
    Tool,
}

impl Role {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Developer => "developer",
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

/// 标准 JSON function 工具。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FunctionToolDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
    #[serde(default, skip_serializing_if = "is_false")]
    pub strict: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_loading: Option<bool>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Responses API 自定义语法工具。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FreeformToolDefinition {
    pub name: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub defer_loading: Option<bool>,
    pub format: Value,
}

/// 命名空间中的工具定义。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum NamespaceToolDefinition {
    #[serde(rename = "function")]
    Function(FunctionToolDefinition),
    #[serde(rename = "custom")]
    Freeform(FreeformToolDefinition),
}

/// Responses API 命名空间工具。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ToolNamespaceDefinition {
    pub name: String,
    pub description: String,
    pub tools: Vec<NamespaceToolDefinition>,
}

/// 统一工具定义（provider 无关）。
///
/// 序列化格式遵循 Responses API。仅支持普通函数调用的 provider
/// 通过 `function_definitions()` 展平兼容条目，忽略不支持的自定义/命名空间工具。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type")]
pub enum ToolDefinition {
    #[serde(rename = "function")]
    Function(FunctionToolDefinition),
    #[serde(rename = "custom")]
    Freeform(FreeformToolDefinition),
    #[serde(rename = "namespace")]
    Namespace(ToolNamespaceDefinition),
    #[serde(rename = "tool_search")]
    ToolSearch {
        execution: String,
        description: String,
        parameters: Value,
    },
    #[serde(rename = "web_search")]
    WebSearch {
        #[serde(flatten)]
        options: serde_json::Map<String, Value>,
    },
}

impl ToolDefinition {
    pub fn function(
        name: impl Into<String>,
        description: impl Into<String>,
        parameters: Value,
    ) -> Self {
        Self::Function(FunctionToolDefinition {
            name: name.into(),
            description: description.into(),
            parameters,
            strict: false,
            defer_loading: None,
        })
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Function(tool) => &tool.name,
            Self::Freeform(tool) => &tool.name,
            Self::Namespace(namespace) => &namespace.name,
            Self::ToolSearch { .. } => "tool_search",
            Self::WebSearch { .. } => "web_search",
        }
    }

    /// 展平为普通函数定义列表，供不支持 Responses API 自定义/命名空间工具的 provider 使用。
    pub fn function_definitions(&self) -> Vec<FunctionToolDefinition> {
        match self {
            Self::Function(tool) => vec![tool.clone()],
            Self::Namespace(namespace) => namespace
                .tools
                .iter()
                .filter_map(|tool| match tool {
                    NamespaceToolDefinition::Function(tool) => {
                        let mut tool = tool.clone();
                        tool.name = format!("{}.{}", namespace.name, tool.name);
                        Some(tool)
                    }
                    NamespaceToolDefinition::Freeform(_) => None,
                })
                .collect(),
            Self::Freeform(_) | Self::ToolSearch { .. } | Self::WebSearch { .. } => Vec::new(),
        }
    }
}

/// 统一消息。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub enum Message {
    System {
        content: String,
    },
    Developer {
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

    pub fn developer(content: impl Into<String>) -> Self {
        Self::Developer {
            content: content.into(),
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
            Self::Developer { .. } => Role::Developer,
            Self::User { .. } => Role::User,
            Self::Assistant { .. } => Role::Assistant,
            Self::Tool { .. } => Role::Tool,
        }
    }

    pub fn text_content(&self) -> &str {
        match self {
            Self::System { content } | Self::Developer { content } => content,
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

        let developer = Message::developer("Follow project policy.");
        assert_eq!(developer.role(), Role::Developer);
        assert_eq!(developer.text_content(), "Follow project policy.");

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
        assert_eq!(
            serde_json::to_string(&Role::Developer).unwrap(),
            "\"developer\""
        );
        assert_eq!(serde_json::to_string(&Role::User).unwrap(), "\"user\"");
        assert_eq!(
            serde_json::to_string(&Role::Assistant).unwrap(),
            "\"assistant\""
        );
        assert_eq!(serde_json::to_string(&Role::Tool).unwrap(), "\"tool\"");
    }

    #[test]
    fn responses_tool_variants_preserve_native_wire_shape() {
        let tools = vec![
            ToolDefinition::function(
                "lookup",
                "Lookup data",
                serde_json::json!({"type": "object"}),
            ),
            ToolDefinition::Freeform(FreeformToolDefinition {
                name: "apply_patch".into(),
                description: "Apply a patch".into(),
                defer_loading: None,
                format: serde_json::json!({
                    "type": "grammar",
                    "syntax": "lark",
                    "definition": "start: /.+/",
                }),
            }),
            ToolDefinition::Namespace(ToolNamespaceDefinition {
                name: "clock".into(),
                description: "Clock tools".into(),
                tools: vec![NamespaceToolDefinition::Function(FunctionToolDefinition {
                    name: "now".into(),
                    description: "Current time".into(),
                    parameters: serde_json::json!({"type": "object"}),
                    strict: true,
                    defer_loading: Some(true),
                })],
            }),
        ];

        let json = serde_json::to_value(&tools).unwrap();
        assert_eq!(json[0]["type"], "function");
        assert_eq!(json[1]["type"], "custom");
        assert_eq!(json[1]["format"]["syntax"], "lark");
        assert_eq!(json[2]["type"], "namespace");
        assert_eq!(json[2]["tools"][0]["defer_loading"], true);
    }

    #[test]
    fn namespace_flattens_only_function_tools_for_legacy_providers() {
        let namespace = ToolDefinition::Namespace(ToolNamespaceDefinition {
            name: "clock".into(),
            description: "Clock tools".into(),
            tools: vec![
                NamespaceToolDefinition::Function(FunctionToolDefinition {
                    name: "now".into(),
                    description: "Current time".into(),
                    parameters: serde_json::json!({"type": "object"}),
                    strict: false,
                    defer_loading: None,
                }),
                NamespaceToolDefinition::Freeform(FreeformToolDefinition {
                    name: "script".into(),
                    description: "Run script".into(),
                    defer_loading: None,
                    format: serde_json::json!({"type": "text"}),
                }),
            ],
        });

        let functions = namespace.function_definitions();
        assert_eq!(functions.len(), 1);
        assert_eq!(functions[0].name, "clock.now");
    }
}
