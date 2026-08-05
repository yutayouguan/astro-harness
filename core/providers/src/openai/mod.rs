//! OpenAI 协议模块（Embeddings / Image Gen / TTS / Responses API / Defaults）。
//!
//! 聊天流式在 [`crate::impls::openai`]，本模块提供非聊天 HTTP 端点。

pub mod defaults;
pub mod embeddings_http;
pub mod image_http;
pub mod media_compat;
pub mod responses;
pub mod tts_http;

pub use defaults::{DEFAULT_API_BASE, DEFAULT_VISION_MODEL};
