//! 内置工具实现；经 inventory 自注册元数据与 handler，由 [`crate::dispatch`] 查表执行。
//!
//! 按领域分子目录（磁盘组织）；对外仍 re-export 各工具模块名，
//! 便于 `builtins::image_gen` 等路径引用。

pub mod agents;
pub mod hitl;
pub mod media;
pub mod memory;
pub mod present;
pub mod system;

pub use agents::{create_agent, delegate, multi_agent, orchestration, team};
pub use hitl::{ask_user, switch_mode};
pub use media::{
    audio_understand, image_gen, music_gen, robotics, tts, video_gen, video_understand, image_understand,
};
pub use memory::{context_tools, memory_tools, scheduled, skills_tool, todo};
pub use present::present_shared;
pub use system::{code_exec, file_ops, http_fetch, terminal, web_extract, web_search};
