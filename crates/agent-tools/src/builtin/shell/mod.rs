//! 系统能力：文件、终端、代码执行、HTTP 抓取、检索、工具搜索、上下文管理。

pub mod apply_patch;
pub mod code_exec;
pub mod context_remaining;
pub mod jobs;
pub mod new_context_window;
pub mod request_permissions;
pub mod request_plugin_install;
pub mod exec_command;
pub mod tool_search;
pub mod wait_for_environment;
pub mod web_fetch;
pub mod web_search;
pub mod write_stdin;
