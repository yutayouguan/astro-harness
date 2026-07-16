//! 内置工具实现；经 [`crate::dispatch`] 按名称路由。
//!
//! 按领域分子目录（磁盘组织）；对外仍 re-export 各工具模块名，
//! 使 `crate::image_gen` 等路径与历史 `register` / `dispatch` 兼容。

pub mod media;
pub mod system;
pub mod memory;
pub mod hitl;
pub mod present;
pub mod agents;

pub use media::{image_gen, music, music_gen, tts, video_gen, vision};
pub use system::{browser, code_exec, file_ops, terminal, web_search};
pub use memory::{memory_tools, scheduled, skills_tool, task_plan};
pub use hitl::{clarify, confirm, request_user_location};
pub use present::{
    present_callout, present_metrics, present_result, present_shared, present_ui,
};
pub use agents::{create_agent, delegate, multi_agent, orchestration};
