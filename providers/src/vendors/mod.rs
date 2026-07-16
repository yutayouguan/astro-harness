//! 各供应商薄封装；协议细节在 [`crate::protocol`] / [`crate::google`] / [`crate::openai`]，
//! 配置在 [`crate::profile`]。
//!
//! Google / OpenAI 见顶层 [`crate::google`]、[`crate::openai`]。

pub mod profile_backed;

pub mod claude;
pub mod deepseek;
pub mod minimax;
pub mod openrouter;
pub mod bailian;
pub mod nvidia;
pub mod moonshot;
pub mod volcengine;
pub mod zhipu;
pub mod azure;
pub mod mimo;
pub mod ollama;
