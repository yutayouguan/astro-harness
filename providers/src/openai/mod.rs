//! OpenAI 协议实现（Chat Completions 兼容媒体、Images、Responses 骨架）。
//!
//! 本模块是 **协议实现包**，与 [`crate::google`] 隔离；多厂商 Chat Completions
//! 分发仍在 [`crate::protocol::http_stream`]。请避免 `use providers::openai::*`
//! 的 glob 导入。
//!
//! - [`image_http`]：Images API
//! - [`media_compat`]：Whisper / 视觉 / 音频描述
//! - [`responses`]：Responses API 骨架
//! - [`defaults`]：默认模型 / 基址常量

pub mod defaults;
pub mod embeddings_http;
pub mod image_http;
pub mod media_compat;
pub mod responses;
pub mod tts_http;

pub use defaults::{DEFAULT_API_BASE, DEFAULT_VISION_MODEL};

/// OpenAI 供应商别名（[`ProfileBackedProvider`](crate::vendors::profile_backed::ProfileBackedProvider)）。
pub type OpenAiProvider = crate::vendors::profile_backed::ProfileBackedProvider;
