//! 多供应商 AI 能力统一封装。
//!
//! - [`types`]：统一消息模型（`Message` enum + `StreamChunk` + `Usage` + `ProviderConfig`）
//! - [`traits`]：能力 trait 系统（`CompletionModel` + `Capable<M>/Nothing` 编译期检查）
//! - [`compat`]：OpenAI 兼容层（`OpenAICompatible` trait — 一行接厂商）
//! - [`impls`]：14 个厂商实现（Anthropic / Google 原生 + 11 个 OpenAI 兼容）
//! - [`new_registry`]：基于 trait 的动态注册表
//! - [`new_dispatch`]：新管线聊天分发
//! - [`shared`]：跨厂商共享基础设施（HTTP / SSE / 探测 / 抽取器 / 媒体 / 视觉）
//! - [`api`]：旧 trait（`AiProvider` / `ChatProvider`）+ 旧兼容类型（`ChatMessage` 等）
//! - [`profile`]：静态配置表（`ProviderProfile` / `ApiMode`）

pub mod anthropic;
pub mod api;
pub mod compat;
pub mod impls;
pub mod new_dispatch;
pub mod new_registry;
pub mod google;
pub mod minimax;
pub mod openai;
pub mod profile;
pub mod shared;
pub mod traits;
pub mod types;

// ── 顶层路径稳定性 ──
pub use api::{registry, streaming, trait_};
pub use google::{files_http, interactions_http, robotics_http};
pub use profile::{read_env_api_key, env_api_key_names};
pub use openai::{embeddings_http, image_http, responses, tts_http};
pub use shared::{extractor, media as media_http, verify, vision};
pub use shared::http as http_stream;
pub use types::image_gen;

// ── 核心类型 re-exports ──
pub use types::ProviderConfig;
pub use streaming::{PauseControl, Usage};

pub use compat::openai_compatible_base;
pub use extractor::{
    extractor as build_extractor, extractor_from_env, parse_submit_payload, ExtractionError,
    Extractor, ExtractorBuilder,
};
pub use http_stream::merge_additional_params;
pub use profile::{ApiMode, ProviderProfile, PROFILES};
pub use anthropic::tools::openai_tools_to_anthropic;
pub use google::tools::openai_tools_to_gemini_native;

pub use trait_::{
    AiProvider, AuthKind, ChatChunk, ChatMessage, ChatProvider, ChatStream, ChatToolCall,
    GeneratedAudio, GeneratedImage, GeneratedVideo,
    ToolCallDeltaChunk, VerifyProvider, VerifyResult,
};
