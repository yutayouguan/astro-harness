//! 各供应商薄封装；协议细节在 [`crate::protocol`]，配置在 [`crate::profile`]。

pub mod profile_backed;

pub mod google;
pub mod openai;
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
