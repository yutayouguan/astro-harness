//! 多供应商 AI 能力统一封装。
//!
//! 提供聊天流式、结构化抽取、图片生成、连通性探测等能力，
//! 通过 [`ProviderRegistry`] 按 provider id 路由到具体实现。
//!
//! 模块分层：
//! - [`api`]：对外契约（trait / client / registry / streaming）
//! - [`profile`]：Hermes 风格 ProviderProfile / ApiMode 表
//! - [`protocol`]：共享协议（Chat Completions / Anthropic / 探测 / 抽取）
//! - [`google`]：Gemini Interactions / Veo / Files / Robotics
//! - [`openai`]：Images / Responses / Whisper·视觉兼容
//! - [`vendors`]：其余供应商薄封装（ProfileBackedProvider）

pub mod api;
pub mod google;
pub mod openai;
pub mod profile;
pub mod protocol;
pub mod vendors;

// 保持原有顶层路径，避免破坏下游 crate 的 `providers::trait_` 等引用。
pub use api::{client, registry, streaming, trait_};
pub use google::{files_http, interactions_chat, interactions_http, native_chat, robotics_http};
pub use openai::{image_http, responses};
pub use protocol::{extractor, http_stream, image_gen, media_http, tool_format, verify, vision};
pub use vendors::{
    azure, bailian, claude, deepseek, mimo, minimax, moonshot, nvidia, ollama, openrouter,
    profile_backed, volcengine, zhipu,
};

pub use client::ProviderClient;
pub use extractor::{parse_submit_payload, ExtractionError, Extractor, ExtractorBuilder};
pub use http_stream::merge_additional_params;
pub use profile::{ApiMode, ProviderProfile, PROFILES};
pub use streaming::{PauseControl, Usage};
pub use tool_format::{openai_tools_to_anthropic, openai_tools_to_gemini_native};
pub use trait_::{
    AiProvider, AuthKind, ChatChunk, ChatMessage, ChatProvider, ChatStream, ChatToolCall,
    GeneratedImage, ImageGenProvider, ProviderConfig, ToolCallDeltaChunk, VerifyProvider,
    VerifyResult,
};
