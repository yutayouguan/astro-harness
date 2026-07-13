//! Agent 工作区与 Astro 数据根目录布局。
//!
//! 职责：
//! - 解析 `~/.astro`（或 `ASTRO_MEMORY_DIR`）下的多 Agent 目录约定
//! - 工作区（`workspace` / `workspace-{id}`）的创建、列举、激活与核心模板
//! - 日记忆（`mermaid/YYYY-MM-DD.md`）路径与初始化
//! - 首次启动时创建目录树、状态 JSON、会话库与公共技能
//!
//! 不变量：
//! - 默认 Agent id 恒为 `workspace`；其他 id 对应 `workspace-{id}` 目录
//! - 工作区 Markdown 在 `workspace-*`，模型/工具/MCP 配置在 `agents/{id}/config.json`
//! - `ensure_*` / `create_*` 不覆盖已存在的用户文件内容

mod paths;
mod lifecycle;
mod templates;

pub use paths::*;
pub use lifecycle::*;
