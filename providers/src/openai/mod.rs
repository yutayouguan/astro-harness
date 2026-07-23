//! OpenAI 旧兼容模块（媒体 / Responses / Defaults）。
//!
//! 聊天流式已迁移到 [`crate::impls::openai`] 和 [`crate::impls::azure`]。
//! 探测已迁移到 [`crate::impls::openai`] 和 [`crate::impls::azure`]。

pub mod defaults;
pub mod embeddings_http;
pub mod image_http;
pub mod media_compat;
pub mod responses;
pub mod tts_http;

pub use defaults::{DEFAULT_API_BASE, DEFAULT_VISION_MODEL};
