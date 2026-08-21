//! 内置工具实现；经 inventory 自注册元数据与 handler，由 [`crate::dispatch`] 查表执行。
//!
//! 内置工具按领域分子目录。
//! 便于 `builtin::image_gen` 等路径引用。

pub mod agents;
pub mod hitl;
pub mod media;
pub mod memory;
pub mod present;
pub mod shell;

pub use agents::{persona_create, subagent};
pub use hitl::{ask_user, switch_mode};
pub use media::{
    audio_understand, image_gen, image_understand, music_gen, robotics, tts, video_gen,
    video_understand,
};
pub use memory::{context_tools, memory_tools, scheduled, skills_tool, todo};
pub use present::present_shared;
pub use shell::{
    code_exec, context_remaining, file_ops, new_context_window, request_plugin_install, terminal,
    tool_search, wait_for_environment, web_fetch, web_search,
};
