//! 工作区路径解析、模板脚手架与 Agent 生命周期。

pub mod agent_config;
pub mod generated;
pub mod lifecycle;
pub mod paths;
pub mod templates;

pub use agent_config::*;
pub use generated::*;
pub use lifecycle::*;
pub use paths::*;
