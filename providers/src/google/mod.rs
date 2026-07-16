//! Google Gemini 协议与供应商实现（与 OpenAI 兼容层隔离）。
//!
//! - [`interactions_chat`] / [`interactions_http`]：Interactions API
//! - [`veo_http`]：Veo 原生视频与 API 根路径
//! - [`files_http`]：Files API
//! - [`robotics_http`]：Robotics-ER `generateContent`（例外保留）

pub mod files_http;
pub mod interactions_chat;
pub mod interactions_http;
pub mod robotics_http;
pub mod veo_http;

/// Google 供应商别名（[`ProfileBackedProvider`](crate::vendors::profile_backed::ProfileBackedProvider)）。
pub type GoogleProvider = crate::vendors::profile_backed::ProfileBackedProvider;
