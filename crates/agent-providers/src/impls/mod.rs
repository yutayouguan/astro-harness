//! 具体厂商实现 — 每个厂商一个模块。
//!
//! OpenAI 兼容厂商通过 [`openai_compat!`] 宏声明，一行定义一个 provider。
//! 原生厂商（Anthropic / Google / DeepSeek 等）有独立的消息转换 + SSE 解析。

/// 声明一个 OpenAI 兼容 provider：struct + ProviderExt + OpenAICompatible + Capabilities。
///
/// ```ignore
/// openai_compat!(Zhipu, "zhipu", "https://open.bigmodel.cn/api/paas/v4",
///                stream_usage: false, caps: chat_media);
/// ```
///
/// `auth` 默认 bearer；`caps` 可选 `chat_only`（仅 Chat）或 `chat_media`（含 Embedding/ImageGen/TTS）。
macro_rules! openai_compat {
    // bearer auth（默认）
    ($name:ident, $id:literal, $url:literal, stream_usage: $su:literal, caps: $caps:ident) => {
        openai_compat!(@full $name, $id, $url, $su, bearer, $caps);
    };
    // 显式指定 auth 模式
    ($name:ident, $id:literal, $url:literal, stream_usage: $su:literal, auth: $auth:ident, caps: $caps:ident) => {
        openai_compat!(@full $name, $id, $url, $su, $auth, $caps);
    };
    // ── 内部实现 ──
    (@full $name:ident, $id:literal, $url:literal, $su:literal, $auth:ident, $caps:ident) => {
        #[derive(Debug, Clone, Copy, Default)]
        pub struct $name;

        impl crate::traits::ProviderExt for $name {
            const NAME: &'static str = $id;
            const BASE_URL: &'static str = $url;
            fn auth_headers(&self, key: &str) -> ::reqwest::header::HeaderMap {
                openai_compat!(@auth $auth key)
            }
        }

        impl crate::compat::OpenAICompatible for $name {
            const STREAM_USAGE: bool = $su;
        }

        openai_compat!(@caps $name, $caps);
    };
    // ── auth 分支 ──
    (@auth bearer $key:ident) => { crate::impls::openai::bearer_headers($key) };
    (@auth none $key:ident) => { { let _ = $key; ::reqwest::header::HeaderMap::new() } };
    // ── capabilities 分支 ──
    (@caps $name:ident, chat_only) => {
        impl crate::traits::Capabilities for $name {
            type Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Self>>;
            type Embedding = crate::traits::Nothing;
            type ImageGen = crate::traits::Nothing;
            type VideoGen = crate::traits::Nothing;
            type TTS = crate::traits::Nothing;
            type MusicGen = crate::traits::Nothing;
        }
    };
    (@caps $name:ident, chat_media) => {
        impl crate::traits::Capabilities for $name {
            type Chat = crate::traits::Capable<crate::compat::OpenAICompletionModel<Self>>;
            type Embedding = crate::traits::Capable<crate::compat::media::CompatEmbeddingModel>;
            type ImageGen = crate::traits::Capable<crate::compat::media::CompatImageGenModel>;
            type VideoGen = crate::traits::Nothing;
            type TTS = crate::traits::Capable<crate::compat::media::CompatTTSModel>;
            type MusicGen = crate::traits::Nothing;
        }
    };
}

pub mod anthropic;
pub mod azure;
pub mod bailian;
pub mod deepseek;
pub mod gemini_native;
pub mod google;
pub mod hunyuan;
pub mod mimo;
pub mod minimax_chat;
pub mod moonshot;
pub mod nvidia;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod volcengine;
pub mod zhipu;

#[cfg(test)]
mod tests;
