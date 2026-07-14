//! 记忆相关 Tauri 命令。
//!
//! ## 架构说明（Frozen Snapshot / `refresh_memory`）
//!
//! - **Tauri 进程不持有长驻 [`agent::loop_::AgentLoop`]。** 聊天经 gRPC 交给 `backend`，
//!   由后端按 `session_id` 缓存 `AgentLoop`；同会话 system prompt 使用冻结 snapshot。
//! - Tauri 侧记忆读写多为请求作用域的 [`memory::MemoryManager`]（`new` / `for_agent`）。
//! - 因此本命令：对**当前活跃 Agent** 打开 `MemoryManager`，调用
//!   [`MemoryManager::refresh_memory_snapshot`] 从磁盘重载 `MEMORY.md` / `USER.md` 并
//!   对齐该 Manager 的 snapshot（验证盘上内容可读、返回最新渲染文本）。
//! - **已在 backend 内存中的聊天会话**仍保留其构造时的冻结 snapshot；新开会话 / 重新
//!   `get_session` 构建的 Loop 会从磁盘加载最新内容。若需刷新「正在进行的对话」prompt，
//!   需后续在 backend 暴露对活会话 `AgentLoop::refresh_memory` 的 RPC（P1 命令面先可用）。

use serde::Serialize;

/// `refresh_memory` 返回：活跃 Agent 重载后的 MEMORY / USER snapshot 渲染。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshMemoryDto {
    pub agent_id: String,
    pub memory_content: String,
    pub user_content: String,
}

/// 从磁盘重载当前活跃 Agent 的 MEMORY / USER 到 MemoryManager snapshot。
///
/// 见模块文档：不触碰 backend 侧已冻结的 AgentLoop；新会话会读到最新盘文件。
#[tauri::command]
pub async fn refresh_memory(agent_id: Option<String>) -> Result<RefreshMemoryDto, String> {
    let root = memory::default_memory_dir();
    memory::ensure_workspace(&root).map_err(|e| e.to_string())?;
    let id = agent_id
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| memory::active_agent_id(&root));
    let mut mgr = memory::MemoryManager::for_agent(root, &id).map_err(|e| e.to_string())?;
    mgr.refresh_memory_snapshot().map_err(|e| e.to_string())?;
    let (memory_content, user_content) = mgr.prompt_content();
    Ok(RefreshMemoryDto {
        agent_id: mgr.agent_id,
        memory_content,
        user_content,
    })
}
