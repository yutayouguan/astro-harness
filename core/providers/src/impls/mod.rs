//! 具体厂商实现 — 每个厂商一个模块。
//!
//! OpenAI 兼容厂商只需 ~30 行（3 个 trait impl）。
//! 原生厂商（Anthropic / Google）有独立的消息转换 + SSE 解析。

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
pub mod openai_responses;
pub mod openrouter;
pub mod volcengine;
pub mod zhipu;

#[cfg(test)]
mod tests;
