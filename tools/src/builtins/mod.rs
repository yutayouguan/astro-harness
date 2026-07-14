//! 内置工具实现；经 [`crate::dispatch`] 按名称路由。

pub mod memory_tools;
pub mod scheduled;
pub mod image_gen;
pub mod file_ops;
pub mod terminal;
pub mod web_search;
pub mod code_exec;
pub mod vision;
pub mod tts;
pub mod skills_tool;
pub mod clarify;
pub mod confirm;
pub mod request_user_location;
pub mod present_ui;
pub mod present_metrics;
pub mod present_callout;
pub mod present_result;
pub mod delegate;
pub mod multi_agent;
pub mod orchestration;
pub mod task_plan;
pub mod browser;
pub mod create_agent;
