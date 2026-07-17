//! 各供应商薄封装；协议细节在 [`crate::protocol`] / [`crate::google`] / [`crate::openai`]，
//! 配置在 [`crate::profile`]。
//!
//! Google / OpenAI 见顶层 [`crate::google`]、[`crate::openai`]。

pub mod profile_backed;

pub mod azure;
pub mod bailian;
pub mod claude;
pub mod deepseek;
pub mod mimo;
pub mod minimax;
pub mod moonshot;
pub mod nvidia;
pub mod ollama;
pub mod openrouter;
pub mod volcengine;
pub mod zhipu;
