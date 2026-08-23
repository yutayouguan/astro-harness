//! 跨域共享的辅助函数与类型——被多个 command 子模块引用。

/// 打开会话存储（session + chat 共用）。
pub(super) fn open_sessions() -> Result<session::SessionStore, String> {
    let root = home::default_memory_dir();
    memory::ensure_workspace(&root).map_err(|e| e.to_string())?;
    session::SessionStore::open_sessions_dir(&root.join("sessions")).map_err(|e| e.to_string())
}

/// 返回本机 Astro 记忆根目录路径。
pub(super) fn memory_root() -> std::path::PathBuf {
    home::default_memory_dir()
}

/// 引导默认工作区文件结构。
pub(super) fn bootstrap_workspace() -> Result<(), String> {
    memory::ensure_default_workspace()
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// 返回本机 Astro 记忆根目录（字符串）。
pub(super) fn memory_dir() -> String {
    home::default_memory_dir().to_string_lossy().into_owned()
}

/// 返回当前（或指定）Agent 工作区路径。
pub(super) fn workspace_dir() -> String {
    home::default_agent_workspace_dir()
        .to_string_lossy()
        .into_owned()
}

/// 暴露默认工作区路径给前端。
#[tauri::command]
pub async fn get_default_workspace_path() -> String {
    workspace_dir()
}

/// 将底层错误转为面向用户的中文提示。
pub(super) fn friendly_error(err: &str) -> String {
    if err.contains("transport") || err.contains("Connection refused") || err.contains("connect") {
        if crate::infra::grpc::embed_backend_enabled() {
            "无法连接后端服务。内嵌 backend 可能尚未就绪或启动失败，请查看日志后重试。".into()
        } else {
            "无法连接后端服务。请先运行：cargo run -p backend（或去掉 ASTRO_EMBED_BACKEND=0 使用内嵌）"
                .into()
        }
    } else {
        err.to_string()
    }
}
