#![warn(clippy::unwrap_used)]
// 测试里的 unwrap 噪声不参与生产告警统计（生产代码仍然是 warn 级）。
#![cfg_attr(test, allow(clippy::unwrap_used))]

//! 多供应商 AI 能力统一封装。
//!
//! - [`types`]：Responses/Chat Completions 输入、流式分片与 Provider 配置
//! - [`traits`]：能力 trait 系统（`ResponsesModel` / `ChatCompletionModel` + `Capable<M>/Nothing`）
//! - [`compat`]：OpenAI 兼容层（`OpenAICompatible` trait — 一行接厂商）
//! - [`impls`]：19 个厂商实现（5 种协议管线）
//! - [`registry`]：协议管线注册表（trait-based `Registry`）
//! - [`dispatch`]：管线分发入口（外部唯一入口）
//! - [`shared`]：跨厂商共享基础设施（HTTP / SSE / 探测 / 抽取器 / 媒体 / 视觉）
//! - [`profile`]：静态配置表（`ProviderProfile` / `ApiMode`）

pub mod anthropic;
pub mod compat;
pub mod custom;
pub mod dispatch;
pub mod google;
pub mod impls;
pub mod minimax;
pub mod openai;
pub mod profile;
pub mod registry;
pub mod shared;
pub mod traits;
pub mod types;

// ── 顶层路径稳定性 ──
pub use google::{files_http, interactions_http, robotics_http};
pub use openai::{embeddings_http, image_http, responses, tts_http};
pub use profile::{env_api_key_names, read_env_api_key};
pub use shared::http as http_stream;
pub use shared::{extractor, media as media_http, verify, vision};
pub use types::image_gen;

// ── Dispatch (唯一公开入口) ──
pub use dispatch::{
    chat_stream, chat_stream_direct, default_model, embed, generate_image, generate_music,
    generate_video, supports_image_gen, text_to_speech, verify,
};
pub use profile::AuthKind;
pub use shared::verify::VerifyResult;

// ── Stream types ──
pub use types::stream::{CompletionStream, PauseControl, StreamChunk, Usage};

// ── Media types ──
pub use types::media::{GeneratedAudio, GeneratedImage, GeneratedVideo};

// ── Chat Completions compatibility types ──
pub use types::request_content::ChatCompletionMessage;

// ── Config ──
pub use types::ProviderConfig;

// ── Error types ──
pub use types::error::{ProviderError, ProviderResult};

pub use anthropic::tools::openai_tools_to_anthropic;
pub use compat::openai_compatible_base;
pub use extractor::{
    extractor as build_extractor, extractor_from_env, parse_submit_payload, ExtractionError,
    Extractor, ExtractorBuilder,
};
pub use google::tools::openai_tools_to_gemini_native;
pub use http_stream::merge_additional_params;
pub use profile::{ApiMode, ProviderProfile, PROFILES};
