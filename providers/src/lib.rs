//! 多供应商 AI 能力统一封装。
//!
//! ## 新架构（trait-based，推荐）
//!
//! - [`types`]：统一消息模型（`Message` enum + `StreamChunk` + `Usage`）
//! - [`traits`]：能力 trait 系统（`CompletionModel` + `Capable<M>/Nothing` 编译期检查）
//! - [`compat`]：OpenAI 兼容层（`OpenAICompatible` trait — 一行接厂商）
//! - [`impls`]：14 个厂商实现（Anthropic / Google 原生 + 11 个 OpenAI 兼容）
//! - [`new_registry`]：基于 trait 的动态注册表
//! - [`new_dispatch`]：新管线聊天分发（bridge 兼容旧签名）
//! - [`bridge`]：新旧类型双向转换
//!
//! ## 旧架构（兼容层，逐步淘汰）
//!
//! - [`api`]：旧 trait（`AiProvider` / `ChatProvider`）+ 旧类型（`ChatMessage`）
//! - [`profile`]：静态配置表（`ProviderProfile` / `ApiMode`）
//! - [`protocol`]：旧 SSE 工具 + 探测路由
//! - [`anthropic`] / [`google`] / [`openai`]：旧协议实现（媒体/探测仍在用）
//! - [`vendors`]：`ProfileBackedProvider` 薄封装

pub mod anthropic;
pub mod api;
pub mod bridge;
pub mod compat;
pub mod impls;
pub mod new_dispatch;
pub mod new_registry;
pub mod google;
pub mod minimax;
pub mod openai;
pub mod profile;
pub mod protocol;
pub mod shared;
pub mod traits;
pub mod types;
pub mod vendors;

// 保持原有顶层路径，避免破坏下游 crate 的 `providers::trait_` 等引用。
pub use api::{client, registry, streaming, trait_};
pub use google::{files_http, interactions_chat, interactions_http, native_chat, robotics_http};
pub use openai::{embeddings_http, image_http, responses, tts_http};
pub use protocol::{extractor, http_stream, image_gen, media_http, tool_format, verify, vision};
pub use vendors::{
    azure, bailian, claude, deepseek, mimo, moonshot, nvidia, ollama, openrouter,
    profile_backed, volcengine, zhipu,
};

pub use client::ProviderClient;
pub use extractor::{parse_submit_payload, ExtractionError, Extractor, ExtractorBuilder};
pub use http_stream::merge_additional_params;
pub use profile::{ApiMode, ProviderProfile, PROFILES};
pub use streaming::{PauseControl, Usage};
pub use anthropic::tools::openai_tools_to_anthropic;
pub use tool_format::openai_tools_to_gemini_native;
pub use trait_::{
    AiProvider, AuthKind, ChatChunk, ChatMessage, ChatProvider, ChatStream, ChatToolCall,
    GeneratedAudio, GeneratedImage, GeneratedVideo, ImageGenProvider, ProviderConfig,
    ToolCallDeltaChunk, VerifyProvider, VerifyResult,
};
