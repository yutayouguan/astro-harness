//! 桌面端浏览器侧栏与 Agent `browser_*` 工具共享同一任务浏览器会话。

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPanelRequest {
    pub session_id: String,
    pub action: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPreviewFileRequest {
    pub session_id: String,
    pub project_id: String,
    pub path: String,
    #[serde(default)]
    pub content: Option<String>,
}

#[tauri::command]
pub async fn browser_panel_control(request: BrowserPanelRequest) -> Result<Value, String> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return Err("需要先开始一个对话，才能创建共享浏览器会话".into());
    }
    let raw = tools::builtin::shell::browser::desktop_control(
        session_id,
        &home::default_memory_dir(),
        request.action.trim(),
        request.args,
    )
    .await
    .map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browser_preview_project_file(
    request: BrowserPreviewFileRequest,
) -> Result<Value, String> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return Err("需要先开始一个对话，才能预览网页".into());
    }
    let roots = super::files::project_roots(&request.project_id).await?;
    let path = super::files::resolve_project_path(&roots, &request.path)?;
    if request
        .content
        .as_ref()
        .is_some_and(|content| content.len() > 16 * 1024 * 1024)
    {
        return Err("HTML 预览内容不能超过 16 MB".into());
    }
    let raw = tools::builtin::shell::browser::desktop_preview_file(
        session_id,
        &home::default_memory_dir(),
        &path,
        request.content,
    )
    .await
    .map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}
