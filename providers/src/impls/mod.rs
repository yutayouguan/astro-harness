//! 具体厂商实现 — 每个厂商一个模块。
//!
//! OpenAI 兼容厂商只需 ~30 行（3 个 trait impl）。
//! 原生厂商（Anthropic / Google）有独立的消息转换 + SSE 解析。

pub mod azure;
pub mod deepseek;
pub mod moonshot;
pub mod nvidia;
pub mod ollama;
pub mod openai;
pub mod openrouter;
pub mod bailian;
pub mod volcengine;
pub mod zhipu;
pub mod hunyuan;

#[cfg(test)]
mod tests;
