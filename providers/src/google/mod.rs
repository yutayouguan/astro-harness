//! Google Gemini 协议与供应商实现（与 OpenAI 兼容层隔离）。
//!
//! 本模块是 **协议实现包**（Interactions / Veo / Files / Robotics），
//! 请避免 `use providers::google::*` 的 glob 导入，以免一次引入大量 HTTP 符号。
//!
//! - [`interactions_http`]：Interactions API（媒体）
//! - [`veo_http`]：Veo 原生视频与 API 根路径
//! - [`files_http`]：Files API
//! - [`robotics_http`]：Robotics-ER `generateContent`（例外保留）
//! - [`defaults`]：默认模型 / host 常量

pub mod defaults;
pub mod files_http;
pub mod interactions_http;
pub mod robotics_http;
pub mod tools;
pub mod veo_http;

pub use defaults::{DEFAULT_API_HOST, DEFAULT_MODEL, DEFAULT_VISION_MODEL};
