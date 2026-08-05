//! 多供应商 AI 能力统一封装。
//!
//! - [`types`]：统一消息模型（`Message` enum + `StreamChunk` + `Usage` + `ProviderConfig`）
//! - [`traits`]：能力 trait 系统（`CompletionModel` + `Capable<M>/Nothing` 编译期检查）
//! - [`compat`]：OpenAI 兼容层（`OpenAICompatible` trait — 一行接厂商）
//! - [`impls`]：19 个厂商实现（5 种协议管线）
//! - [`registry`]：协议管线注册表（trait-based `Registry`）
//! - [`dispatch`]：管线分发入口（外部唯一入口）
//! - [`shared`]：跨厂商共享基础设施（HTTP / SSE / 探测 / 抽取器 / 媒体 / 视觉）
//! - [`profile`]：静态配置表（`ProviderProfile` / `ApiMode`）

pub mod anthropic;
pub mod compat;
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
pub use profile::{read_env_api_key, env_api_key_names};
pub use openai::{embeddings_http, image_http, responses, tts_http};
pub use shared::{extractor, media as media_http, verify, vision};
pub use shared::http as http_stream;
pub use types::image_gen;

// ── Dispatch (唯一公开入口) ──
pub use dispatch::{chat_stream, chat_stream_direct, generate_image, text_to_speech, generate_video, generate_music, embed, verify, default_model, supports_image_gen};
pub use profile::AuthKind;
pub use shared::verify::VerifyResult;

// ── Stream types ──
pub use types::stream::{PauseControl, Usage, CompletionStream, StreamChunk};

// ── Media types ──
pub use types::media::{GeneratedAudio, GeneratedImage, GeneratedVideo};

// ── Message types ──
pub use types::message::Message;

// ── Config ──
pub use types::ProviderConfig;

pub use compat::openai_compatible_base;
pub use extractor::{
    extractor as build_extractor, extractor_from_env, parse_submit_payload, ExtractionError,
    Extractor, ExtractorBuilder,
};
pub use http_stream::merge_additional_params;
pub use profile::{ApiMode, ProviderProfile, PROFILES};
pub use anthropic::tools::openai_tools_to_anthropic;
pub use google::tools::openai_tools_to_gemini_native;
