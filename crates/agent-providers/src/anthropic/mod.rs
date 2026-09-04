//! Anthropic 旧兼容模块（Tools / Defaults）。
//!
//! 聊天流式已迁移到 [`crate::impls::anthropic`]。
//! 探测已迁移到 [`crate::impls::anthropic`]。

pub mod defaults;
pub mod tools;

pub use tools::openai_tools_to_anthropic;
