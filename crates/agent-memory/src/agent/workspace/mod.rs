//! Agent 工作区：完整引导（会话库 + 技能播种）在本模块；路径脚手架见 `home`。

mod lifecycle;

pub use lifecycle::{ensure_default_workspace, ensure_workspace, EnsureWorkspaceReport};
