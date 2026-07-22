//! Anthropic Messages API 协议实现。
//!
//! 包含聊天流式、消息转换、SSE 解析、工具格式转换、连通性探测。

pub mod chat;
pub mod defaults;
pub mod messages;
pub mod sse;
pub mod tools;
pub mod verify;

pub use chat::anthropic_chat_stream;
pub use tools::openai_tools_to_anthropic;

pub type ClaudeProvider = crate::vendors::profile_backed::ProfileBackedProvider;
