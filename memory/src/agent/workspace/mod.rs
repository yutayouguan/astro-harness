//! Agent 工作区与 Astro 数据根目录布局。
//!
//! 路径与脚手架主体在 `home`；完整 `ensure_workspace`（含会话库与技能）在本模块编排。

mod paths;
mod lifecycle;

pub use paths::*;
pub use lifecycle::*;
