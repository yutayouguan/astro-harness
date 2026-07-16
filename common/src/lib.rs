//! 跨 crate 共享类型：消息、工具描述与统一错误。

pub mod auxiliary_target;
pub mod chat_target;
pub mod error;
pub mod message;
pub mod text;
pub mod title;
pub mod tool;

pub use auxiliary_target::{AuxiliaryTargetChain, AuxiliaryTask};
pub use chat_target::*;
pub use title::sanitize_title;

pub use text::{truncate_tool_result, truncate_utf8, MAX_TOOL_RESULT_BYTES};
