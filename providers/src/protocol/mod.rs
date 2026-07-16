//! 协议与传输实现：共享分发、探测、工具格式与结构化抽取。
//!
//! Google / OpenAI 专属实现见 [`crate::google`]、[`crate::openai`]。

pub mod extractor;
pub mod http_stream;
pub mod image_gen;
pub mod media_http;
pub mod tool_format;
pub mod verify;
