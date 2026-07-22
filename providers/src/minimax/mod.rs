//! MiniMax 多能力供应商（语言 + 视频 + 语音 + 图像 + 音乐）。
//!
//! - [`image_http`]：图像生成
//! - [`tts_http`]：语音合成（TTS）
//! - [`video_http`]：视频生成（异步任务）
//! - [`music_http`]：音乐生成
//! - [`voice_clone_http`]：声音克隆
//! - [`files_http`]：文件上传 / 查询

pub mod defaults;
pub mod files_http;
pub mod image_http;
pub mod music_http;
pub mod tts_http;
pub mod video_http;
pub mod voice_clone_http;

pub use defaults::{DEFAULT_API_BASE, DEFAULT_ANTHROPIC_BASE};

/// MiniMax 供应商别名。
pub type MiniMaxProvider = crate::vendors::profile_backed::ProfileBackedProvider;
