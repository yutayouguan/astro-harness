//! 跨 crate 共享类型：消息、工具描述与统一错误。

pub mod chat_target;
pub mod error;
pub mod message;
pub mod text;
pub mod tool;

pub use chat_target::*;

pub use text::{truncate_tool_result, truncate_utf8, MAX_TOOL_RESULT_BYTES};
