//! OpenAI 协议实现（Chat Completions 兼容媒体、Images、Responses 骨架）。
//!
//! 与 [`crate::google`] 隔离；多厂商 Chat Completions 分发仍在 [`crate::protocol::http_stream`]。

pub mod image_http;
pub mod media_compat;
pub mod responses;

/// OpenAI 供应商别名（[`ProfileBackedProvider`](crate::vendors::profile_backed::ProfileBackedProvider)）。
pub type OpenAiProvider = crate::vendors::profile_backed::ProfileBackedProvider;
