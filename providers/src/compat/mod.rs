//! OpenAI Chat Completions 兼容层 — 一行接入 OpenAI 兼容厂商。

pub mod completion;
pub mod messages;
pub mod sse;

pub use completion::{OpenAICompatible, OpenAICompletionModel};
