//! Anthropic 旧兼容模块（Batch / Token Count / Tools / Defaults）。
//!
//! 聊天流式已迁移到 [`crate::impls::anthropic`]。
//! 探测已迁移到 [`crate::impls::anthropic`]。

pub mod batch;
pub mod defaults;
pub mod token_count;
pub mod tools;

pub use tools::openai_tools_to_anthropic;
