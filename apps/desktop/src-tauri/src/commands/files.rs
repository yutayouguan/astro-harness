//! 文件系统 Tauri 命令：列目录、读写文件、剪贴板、下载、删除、移动、复制。

use serde::Serialize;
use tauri::Manager;

use super::common::{bootstrap_workspace, memory_root, open_sessions, workspace_dir};

// ---------------------------------------------------------------------------
// DTOs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize)]
pub struct FileEntryDto {
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub size: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileBase64Dto {
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub base64: String,
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 在记忆沙箱内解析相对/绝对路径，拒绝越界。
fn resolve_memory_path(path: &str) -> Result<std::path::PathBuf, String> {
    let memory = memory_root();
    let p = std::path::PathBuf::from(path);
    // Path::starts_with is lexical — reject `..` so `{memory}/../outside` cannot pass.
    if !crate::ui::fs_ops::is_lexically_under(&memory, &p) {
        return Err("只能访问记忆目录内的路径".into());
    }
    // When the path exists, re-check after resolving symlinks.
    if p.exists() {
        if let (Ok(memory_canon), Ok(path_canon)) = (memory.canonicalize(), p.canonicalize()) {
            if !path_canon.starts_with(&memory_canon) {
                return Err("只能访问记忆目录内的路径".into());
            }
        }
    }
    Ok(p)
}

/// 读取项目 roots。项目文件 API 仅使用这些 roots，不复用记忆沙箱边界。
fn project_roots(project_id: &str) -> Result<Vec<std::path::PathBuf>, String> {
    let project_id = project_id.trim();
    if project_id.is_empty() {
        return Err("project_id 不能为空".into());
    }
    let project = open_sessions()?
        .get_project(project_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "项目不存在".to_string())?;
    if project.roots.is_empty() {
        return Err("项目没有可用根目录".into());
    }
    Ok(project.roots.into_iter().map(Into::into).collect())
}

/// 在多个项目根内解析路径。
///
/// 已存在路径必须在 canonicalize 后仍位于某个 root 内；不存在路径则验证
/// 最近存在祖先，阻止通过目录 symlink 把后续写入导向 root 外。
fn resolve_project_path(
    roots: &[std::path::PathBuf],
    path: &str,
) -> Result<std::path::PathBuf, String> {
    use std::path::Component;

    let raw = path.trim();
    if raw.is_empty() {
        return Err("路径不能为空".into());
    }
    let candidate = std::path::PathBuf::from(raw);
    if !candidate.is_absolute() {
        return Err("项目路径必须是绝对路径".into());
    }
    if candidate
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("项目路径不能包含 ..".into());
    }

    let canonical_roots = roots
        .iter()
        .map(|root| {
            let canonical = root
                .canonicalize()
                .map_err(|e| format!("项目根目录不可访问（{}）: {e}", root.display()))?;
            if !canonical.is_dir() {
                return Err(format!("项目根路径不是目录: {}", root.display()));
            }
            Ok((root, canonical))
        })
        .collect::<Result<Vec<_>, String>>()?;

    // symlink_metadata 能识别 dangling symlink；这种路径必须进入 canonicalize
    // 并失败，而不能被当作安全的“不存在路径”继续写入。
    if std::fs::symlink_metadata(&candidate).is_ok() {
        let canonical = candidate
            .canonicalize()
            .map_err(|e| format!("无法解析项目路径（可能是失效符号链接）: {e}"))?;
        if canonical_roots
            .iter()
            .any(|(_, root)| canonical.as_path() == root.as_path() || canonical.starts_with(root))
        {
            return Ok(candidate);
        }
        return Err("路径不属于该项目".into());
    }

    let matching_roots = canonical_roots
        .iter()
        .filter(|(raw_root, canonical_root)| {
            candidate.starts_with(raw_root) || candidate.starts_with(canonical_root)
        })
        .collect::<Vec<_>>();
    if matching_roots.is_empty() {
        return Err("路径不属于该项目".into());
    }

    let mut ancestor = candidate.as_path();
    while std::fs::symlink_metadata(ancestor).is_err() {
        ancestor = ancestor
            .parent()
            .ok_or_else(|| "找不到已存在的父目录".to_string())?;
    }
    let canonical_ancestor = ancestor
        .canonicalize()
        .map_err(|e| format!("无法解析父目录（可能是失效符号链接）: {e}"))?;
    if matching_roots.iter().any(|(_, root)| {
        canonical_ancestor.as_path() == root.as_path()
            || canonical_ancestor.starts_with(root.as_path())
    }) {
        Ok(candidate)
    } else {
        Err("路径不属于该项目".into())
    }
}

fn ensure_not_project_root(
    roots: &[std::path::PathBuf],
    path: &std::path::Path,
    operation: &str,
) -> Result<(), String> {
    let canonical = path
        .canonicalize()
        .map_err(|e| format!("无法解析路径: {e}"))?;
    for root in roots {
        let root = root
            .canonicalize()
            .map_err(|e| format!("项目根目录不可访问（{}）: {e}", root.display()))?;
        if canonical == root {
            return Err(format!("不能{operation}项目根目录"));
        }
    }
    Ok(())
}

/// 净化文件/目录名，去掉危险字符。
fn sanitize_entry_name(name: &str) -> Result<String, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("名称不能为空".into());
    }
    if name.contains('/') || name.contains('\\') {
        return Err("名称不能包含路径分隔符".into());
    }
    if name == "." || name == ".." {
        return Err("无效的名称".into());
    }
    Ok(name.to_string())
}

/// 比较两条路径是否指向同一位置（规范化后）。
fn paths_equal(a: &std::path::Path, b: &std::path::Path) -> bool {
    if a == b {
        return true;
    }
    std::fs::canonicalize(a)
        .ok()
        .zip(std::fs::canonicalize(b).ok())
        .map(|(ca, cb)| ca == cb)
        .unwrap_or(false)
}

/// 将目录项转为前端 FileEntry DTO。
fn file_entry_dto(path: &std::path::Path, name: &str, is_dir: bool) -> FileEntryDto {
    let size = if is_dir {
        0
    } else {
        std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(0)
    };
    FileEntryDto {
        path: path.to_string_lossy().to_string(),
        name: name.to_string(),
        is_dir,
        size,
    }
}

/// 根据扩展名猜测 MIME 类型。
fn guess_mime(name: &str) -> String {
    let ext = std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "svg" => "image/svg+xml",
        "bmp" => "image/bmp",
        "md" | "markdown" => "text/markdown",
        "txt" => "text/plain",
        "json" => "application/json",
        "csv" => "text/csv",
        "html" | "htm" => "text/html",
        "css" => "text/css",
        "js" => "text/javascript",
        "ts" | "tsx" => "text/typescript",
        "rs" => "text/x-rust",
        "py" => "text/x-python",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "m4a" => "audio/mp4",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "zip" => "application/zip",
        _ => "application/octet-stream",
    }
    .into()
}

/// 用系统默认应用打开路径。
fn open_path_with_system(path: &std::path::Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        // `start` 需要空标题参数，路径单独传入
        std::process::Command::new("cmd")
            .args(["/C", "start", ""])
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        std::process::Command::new("xdg-open")
            .arg(path)
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = path;
        Err("当前平台不支持用系统应用打开文件".into())
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

/// 列出项目目录（包含隐藏项），目录优先并按名称排序。
#[tauri::command]
pub async fn project_list_files(
    project_id: String,
    path: String,
) -> Result<Vec<FileEntryDto>, String> {
    let roots = project_roots(&project_id)?;
    let directory = resolve_project_path(&roots, &path)?;
    if !directory.is_dir() {
        return Err("路径不是目录".into());
    }

    let mut entries = Vec::new();
    for entry in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let entry_path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        let file_type = entry.file_type().map_err(|e| e.to_string())?;

        // 不跟随越界目录 symlink；指向项目内的 symlink 仍可正常浏览。
        let metadata = if file_type.is_symlink() {
            if resolve_project_path(&roots, &entry_path.to_string_lossy()).is_ok() {
                std::fs::metadata(&entry_path).ok()
            } else {
                std::fs::symlink_metadata(&entry_path).ok()
            }
        } else {
            entry.metadata().ok()
        };
        let is_dir = metadata.as_ref().is_some_and(|metadata| metadata.is_dir())
            && (!file_type.is_symlink()
                || resolve_project_path(&roots, &entry_path.to_string_lossy()).is_ok());
        let size = if is_dir {
            0
        } else {
            metadata.map(|metadata| metadata.len() as i64).unwrap_or(0)
        };
        entries.push(FileEntryDto {
            path: entry_path.to_string_lossy().into_owned(),
            name,
            is_dir,
            size,
        });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    Ok(entries)
}

/// 读取项目内 UTF-8 文本文件。
#[tauri::command]
pub async fn project_read_file(project_id: String, path: String) -> Result<String, String> {
    let roots = project_roots(&project_id)?;
    let path = resolve_project_path(&roots, &path)?;
    if path.is_dir() {
        return Err("路径是目录".into());
    }
    std::fs::read_to_string(path).map_err(|e| e.to_string())
}

/// 写入项目内 UTF-8 文本文件；缺失的父目录会一并创建。
#[tauri::command]
pub async fn project_write_file(
    project_id: String,
    path: String,
    content: String,
) -> Result<(), String> {
    let roots = project_roots(&project_id)?;
    let path = resolve_project_path(&roots, &path)?;
    if path.is_dir() {
        return Err("不能写入目录".into());
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, content).map_err(|e| e.to_string())
}

/// 在项目目录下新建空文件。
#[tauri::command]
pub async fn project_create_file(
    project_id: String,
    parent: String,
    name: String,
) -> Result<FileEntryDto, String> {
    let name = sanitize_entry_name(&name)?;
    let roots = project_roots(&project_id)?;
    let parent = resolve_project_path(&roots, &parent)?;
    if !parent.is_dir() {
        return Err("父路径不是目录".into());
    }
    let path = resolve_project_path(&roots, &parent.join(&name).to_string_lossy())?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    options.open(&path).map_err(|e| e.to_string())?;
    Ok(file_entry_dto(&path, &name, false))
}

/// 在项目目录下新建目录。
#[tauri::command]
pub async fn project_create_directory(
    project_id: String,
    parent: String,
    name: String,
) -> Result<FileEntryDto, String> {
    let name = sanitize_entry_name(&name)?;
    let roots = project_roots(&project_id)?;
    let parent = resolve_project_path(&roots, &parent)?;
    if !parent.is_dir() {
        return Err("父路径不是目录".into());
    }
    let path = resolve_project_path(&roots, &parent.join(&name).to_string_lossy())?;
    std::fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(file_entry_dto(&path, &name, true))
}

/// 重命名项目内路径；项目 root 本身不可重命名。
#[tauri::command]
pub async fn project_rename_path(
    project_id: String,
    path: String,
    new_name: String,
) -> Result<FileEntryDto, String> {
    let new_name = sanitize_entry_name(&new_name)?;
    let roots = project_roots(&project_id)?;
    let source = resolve_project_path(&roots, &path)?;
    if !source.exists() {
        return Err("路径不存在".into());
    }
    ensure_not_project_root(&roots, &source, "重命名")?;
    if source.file_name().and_then(|name| name.to_str()) == Some(new_name.as_str()) {
        return Ok(file_entry_dto(&source, &new_name, source.is_dir()));
    }
    let parent = source
        .parent()
        .ok_or_else(|| "路径没有父目录".to_string())?;
    let destination = resolve_project_path(&roots, &parent.join(&new_name).to_string_lossy())?;
    if std::fs::symlink_metadata(&destination).is_ok() {
        return Err("文件或目录已存在".into());
    }
    std::fs::rename(&source, &destination).map_err(|e| e.to_string())?;
    Ok(file_entry_dto(
        &destination,
        &new_name,
        destination.is_dir(),
    ))
}

/// 将项目内路径移入系统废纸篓；项目 roots 本身不可删除。
#[tauri::command]
pub async fn project_trash_paths(project_id: String, paths: Vec<String>) -> Result<u32, String> {
    let roots = project_roots(&project_id)?;
    let mut resolved = Vec::with_capacity(paths.len());
    for path in paths {
        let path = resolve_project_path(&roots, &path)?;
        if !path.exists() {
            return Err(format!("路径不存在: {}", path.display()));
        }
        ensure_not_project_root(&roots, &path, "删除")?;
        resolved.push(path);
    }
    for path in &resolved {
        trash::delete(path).map_err(|e| e.to_string())?;
    }
    Ok(resolved.len() as u32)
}

/// 在沙箱内列举工作区 / 文件空间路径。
#[tauri::command]
pub async fn list_files(path: Option<String>) -> Result<Vec<FileEntryDto>, String> {
    bootstrap_workspace()?;
    // 默认进入当前激活 Agent 的工作空间（而非整个 ~/.astro）
    let root = path.unwrap_or_else(workspace_dir);
    let root_path = std::path::PathBuf::from(&root);
    if !root_path.exists() {
        return Ok(vec![]);
    }

    let mut entries = Vec::new();
    let read = std::fs::read_dir(&root_path).map_err(|e| e.to_string())?;
    for entry in read.flatten() {
        let meta = entry.metadata().ok();
        let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
        let size = meta.map(|m| m.len() as i64).unwrap_or(0);
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with('.') {
            continue;
        }
        entries.push(FileEntryDto {
            path: entry.path().to_string_lossy().to_string(),
            name,
            is_dir,
            size,
        });
    }
    entries.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.cmp(&b.name),
    });
    Ok(entries)
}

/// 安全读取文本文件内容。
#[tauri::command]
pub async fn read_file(path: String) -> Result<String, String> {
    let p = resolve_memory_path(&path)?;
    if p.is_dir() {
        return Err("路径是目录".into());
    }
    std::fs::read_to_string(&p).map_err(|e| e.to_string())
}

/// 用系统默认应用打开记忆目录内的文件/文件夹（不走 shell.open 的 URL 校验）。
#[tauri::command]
pub async fn open_path_externally(path: String) -> Result<(), String> {
    let p = resolve_memory_path(&path)?;
    if !p.exists() {
        return Err("文件不存在".into());
    }
    open_path_with_system(&p)
}

/// 在系统文件管理器中显示路径。
#[tauri::command]
pub async fn reveal_in_folder(path: String) -> Result<(), String> {
    let p = resolve_memory_path(&path)?;
    if !p.exists() {
        return Err("路径不存在".into());
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .args(["-R"])
            .arg(&p)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", p.to_string_lossy()))
            .spawn()
            .map_err(|e| e.to_string())?;
        return Ok(());
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let parent = p.parent().unwrap_or(&p);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// 将路径移入废纸篓。
#[tauri::command]
pub async fn trash_paths(paths: Vec<String>) -> Result<u32, String> {
    let mut ok = 0u32;
    for path in paths {
        let p = resolve_memory_path(&path)?;
        let memory = memory_root();
        if paths_equal(&p, &memory) {
            return Err("不能删除数据根目录".into());
        }
        for agent in home::list_agents(&memory) {
            if paths_equal(&p, std::path::Path::new(&agent.path)) {
                return Err("不能删除 Agent 工作区根目录".into());
            }
        }
        if !p.exists() {
            continue;
        }
        trash::delete(&p).map_err(|e| e.to_string())?;
        ok += 1;
    }
    Ok(ok)
}

/// 以 Base64 读取文件（附件 / 图标）。
#[tauri::command]
pub async fn read_file_base64(path: String) -> Result<FileBase64Dto, String> {
    use base64::Engine;
    let p = resolve_memory_path(&path)?;
    if !p.is_file() {
        return Err("不是文件".into());
    }
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("文件过大（>32MB）".into());
    }
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    Ok(FileBase64Dto {
        mime: guess_mime(&name),
        size: bytes.len() as u64,
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        name,
    })
}

/// 读取用户主动提供的本地文件（拖放 / 系统剪贴板路径），不限记忆沙箱；上限 32MB。
#[tauri::command]
pub async fn read_user_file_base64(path: String) -> Result<FileBase64Dto, String> {
    use base64::Engine;
    let raw = path.trim();
    if raw.is_empty() {
        return Err("路径为空".into());
    }
    let p = std::path::PathBuf::from(raw);
    if !p.is_absolute() {
        return Err("需要绝对路径".into());
    }
    if !p.is_file() {
        return Err("不是文件".into());
    }
    let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("文件过大（>32MB）".into());
    }
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("file")
        .to_string();
    Ok(FileBase64Dto {
        mime: guess_mime(&name),
        size: bytes.len() as u64,
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        name,
    })
}

/// 将记忆沙箱内文件复制到系统下载目录；返回跨平台展示路径（`~/Downloads/...`）。
#[tauri::command]
pub async fn download_file_to_downloads(path: String) -> Result<String, String> {
    let src = resolve_memory_path(&path)?;
    if !src.is_file() {
        return Err("不是文件".into());
    }
    let name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "无效文件名".to_string())?;
    let dest_dir = home::user_downloads_dir();
    let dest = crate::ui::fs_ops::unique_dest_name(&dest_dir, name);
    std::fs::copy(&src, &dest).map_err(|e| e.to_string())?;
    Ok(home::display_user_path(&dest))
}

/// 将 Base64 内容写入系统下载目录；返回跨平台展示路径（`~/Downloads/...`）。
#[tauri::command]
pub async fn download_bytes_to_downloads(
    filename: String,
    base64_data: String,
) -> Result<String, String> {
    use base64::Engine;
    let name = sanitize_entry_name(&filename)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(base64_data.trim())
        .map_err(|e| format!("Base64 解码失败: {e}"))?;
    if bytes.len() > 32 * 1024 * 1024 {
        return Err("文件过大（>32MB）".into());
    }
    let dest_dir = home::user_downloads_dir();
    let dest = crate::ui::fs_ops::unique_dest_name(&dest_dir, &name);
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    Ok(home::display_user_path(&dest))
}

/// 复制选中路径到剪贴板。
#[tauri::command]
pub async fn copy_paths_to_clipboard(
    app: tauri::AppHandle,
    paths: Vec<String>,
) -> Result<(), String> {
    if paths.is_empty() {
        return Err("没有可复制的文件".into());
    }
    let mut resolved = Vec::new();
    for path in &paths {
        let p = resolve_memory_path(path)?;
        if !p.exists() {
            return Err(format!("路径不存在: {}", p.display()));
        }
        resolved.push(p);
    }
    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    crate::infra::clipboard_files::write_paths(&clipboard, &resolved)
}

/// 列出系统剪贴板中的文件路径（仅文件，不含目录）；供聊天输入粘贴附件。
#[tauri::command]
pub async fn list_clipboard_file_paths(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    let paths = crate::infra::clipboard_files::read_paths(&clipboard)?;
    let files: Vec<String> = paths
        .into_iter()
        .filter(|p| p.is_file())
        .map(|p| p.to_string_lossy().to_string())
        .collect();
    if files.is_empty() {
        return Err("剪贴板中没有文件".into());
    }
    Ok(files)
}

/// 从剪贴板粘贴路径列表。
#[tauri::command]
pub async fn paste_paths_from_clipboard(
    app: tauri::AppHandle,
    dest_dir: String,
    mode: Option<String>,
) -> Result<Vec<FileEntryDto>, String> {
    let dest_dir = resolve_memory_path(&dest_dir)?;
    if !dest_dir.is_dir() {
        return Err("目标不是目录".into());
    }
    let clipboard = app.state::<tauri_plugin_clipboard::Clipboard>();
    let sources = crate::infra::clipboard_files::read_paths(&clipboard)?;
    let cut = mode.as_deref() == Some("cut");
    let mut out = Vec::new();
    for src in sources {
        if !src.exists() {
            continue;
        }
        // 外部源可不在 memory 内；目标必须在 memory 内（dest_dir 已 resolve）
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "无效文件名".to_string())?;
        let candidate = dest_dir.join(name);
        // cut + 同目录：与 move_paths 一致，no-op
        if cut && paths_equal(&src, &candidate) {
            out.push(file_entry_dto(&src, name, src.is_dir()));
            continue;
        }
        let dest = crate::ui::fs_ops::unique_dest_name(&dest_dir, name);
        if src.is_dir() && crate::ui::fs_ops::is_same_or_subdir(&src, &dest_dir) {
            return Err("不能粘贴到自身或其子目录".into());
        }
        if cut {
            // 仅当源也在 memory 沙箱内才允许 move；否则强制 copy
            let can_move = resolve_memory_path(&src.to_string_lossy()).is_ok();
            if can_move {
                crate::ui::fs_ops::move_path(&src, &dest)?;
            } else {
                crate::ui::fs_ops::copy_path_recursive(&src, &dest)?;
            }
        } else {
            crate::ui::fs_ops::copy_path_recursive(&src, &dest)?;
        }
        let final_name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name)
            .to_string();
        out.push(file_entry_dto(&dest, &final_name, dest.is_dir()));
    }
    if out.is_empty() {
        return Err("剪贴板中没有可粘贴的文件".into());
    }
    Ok(out)
}

/// 安全写入文本文件。
#[tauri::command]
pub async fn write_file(
    path: String,
    content: String,
    session_id: Option<String>,
    as_artifact: Option<bool>,
) -> Result<(), String> {
    let p = resolve_memory_path(&path)?;
    if p.is_dir() {
        return Err("路径是目录".into());
    }
    if let Some(parent) = p.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(&p, content).map_err(|e| e.to_string())?;

    if as_artifact.unwrap_or(false) {
        let mem = home::default_memory_dir();
        if let Ok(db) = artifacts::open_default(&mem) {
            let _ = db.register(
                &path,
                artifacts::ArtifactSource::AgentWrite,
                session_id.as_deref(),
                None,
                None,
            );
        }
    }
    Ok(())
}

/// Tauri 命令：create_file。
#[tauri::command]
pub async fn create_file(parent: String, name: String) -> Result<FileEntryDto, String> {
    let name = sanitize_entry_name(&name)?;
    let parent = resolve_memory_path(&parent)?;
    if !parent.is_dir() {
        return Err("父路径不是目录".into());
    }
    let path = parent.join(&name);
    if path.exists() {
        return Err("文件或目录已存在".into());
    }
    std::fs::write(&path, "").map_err(|e| e.to_string())?;
    Ok(file_entry_dto(&path, &name, false))
}

/// Tauri 命令：create_directory。
#[tauri::command]
pub async fn create_directory(parent: String, name: String) -> Result<FileEntryDto, String> {
    let name = sanitize_entry_name(&name)?;
    let parent = resolve_memory_path(&parent)?;
    if !parent.is_dir() {
        return Err("父路径不是目录".into());
    }
    let path = parent.join(&name);
    if path.exists() {
        return Err("文件或目录已存在".into());
    }
    std::fs::create_dir(&path).map_err(|e| e.to_string())?;
    Ok(file_entry_dto(&path, &name, true))
}

/// 重命名或移动沙箱内路径。
#[tauri::command]
pub async fn rename_path(path: String, new_name: String) -> Result<FileEntryDto, String> {
    let new_name = sanitize_entry_name(&new_name)?;
    let p = resolve_memory_path(&path)?;
    let memory = memory_root();
    if paths_equal(&p, &memory) {
        return Err("不能重命名数据根目录".into());
    }
    for agent in home::list_agents(&memory) {
        if paths_equal(&p, std::path::Path::new(&agent.path)) {
            return Err("不能重命名 Agent 工作区根目录".into());
        }
    }
    if !p.exists() {
        return Err("路径不存在".into());
    }
    let current_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if current_name == new_name {
        return Ok(file_entry_dto(&p, &new_name, p.is_dir()));
    }
    let parent = p.parent().ok_or_else(|| "无父目录".to_string())?;
    let dest = parent.join(&new_name);
    if dest.exists() {
        return Err("文件或目录已存在".into());
    }
    std::fs::rename(&p, &dest).map_err(|e| e.to_string())?;
    Ok(file_entry_dto(&dest, &new_name, dest.is_dir()))
}

/// 复制选中路径到目标目录。
#[tauri::command]
pub async fn copy_paths(
    sources: Vec<String>,
    dest_dir: String,
) -> Result<Vec<FileEntryDto>, String> {
    let dest_dir = resolve_memory_path(&dest_dir)?;
    if !dest_dir.is_dir() {
        return Err("目标不是目录".into());
    }
    let mut out = Vec::new();
    for src in sources {
        let src = resolve_memory_path(&src)?;
        if !src.exists() {
            return Err(format!("源不存在: {}", src.display()));
        }
        if src.is_dir() && crate::ui::fs_ops::is_same_or_subdir(&src, &dest_dir) {
            return Err("不能复制到自身或其子目录".into());
        }
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "无效文件名".to_string())?;
        let dest = crate::ui::fs_ops::unique_dest_name(&dest_dir, name);
        if crate::ui::fs_ops::is_same_or_subdir(&src, &dest) {
            return Err("不能复制到自身或其子目录".into());
        }
        crate::ui::fs_ops::copy_path_recursive(&src, &dest)?;
        let final_name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name)
            .to_string();
        out.push(file_entry_dto(&dest, &final_name, dest.is_dir()));
    }
    Ok(out)
}

/// 移动选中路径到目标目录。
#[tauri::command]
pub async fn move_paths(
    sources: Vec<String>,
    dest_dir: String,
) -> Result<Vec<FileEntryDto>, String> {
    let dest_dir = resolve_memory_path(&dest_dir)?;
    if !dest_dir.is_dir() {
        return Err("目标不是目录".into());
    }
    let memory = memory_root();
    let mut out = Vec::new();
    for src in sources {
        let src = resolve_memory_path(&src)?;
        if paths_equal(&src, &memory) {
            return Err("不能移动数据根目录".into());
        }
        for agent in home::list_agents(&memory) {
            if paths_equal(&src, std::path::Path::new(&agent.path)) {
                return Err("不能移动 Agent 工作区根目录".into());
            }
        }
        if !src.exists() {
            return Err(format!("源不存在: {}", src.display()));
        }
        let name = src
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "无效文件名".to_string())?;
        let candidate = dest_dir.join(name);
        let same = paths_equal(&src, &candidate);
        let dest = if same {
            candidate
        } else {
            crate::ui::fs_ops::unique_dest_name(&dest_dir, name)
        };
        if paths_equal(&src, &dest) {
            out.push(file_entry_dto(&src, name, src.is_dir()));
            continue;
        }
        crate::ui::fs_ops::move_path(&src, &dest)?;
        let final_name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(name)
            .to_string();
        out.push(file_entry_dto(&dest, &final_name, dest.is_dir()));
    }
    Ok(out)
}

/// Tauri 命令：delete_path。
#[tauri::command]
pub async fn delete_path(path: String) -> Result<(), String> {
    let p = resolve_memory_path(&path)?;
    let memory = memory_root();
    if paths_equal(&p, &memory) {
        return Err("不能删除数据根目录".into());
    }
    // 不能删除任一 Agent 工作区根目录
    for agent in home::list_agents(&memory) {
        if paths_equal(&p, std::path::Path::new(&agent.path)) {
            return Err("不能删除 Agent 工作区根目录".into());
        }
    }
    if !p.exists() {
        return Err("路径不存在".into());
    }
    if p.is_dir() {
        std::fs::remove_dir_all(&p).map_err(|e| e.to_string())?;
    } else {
        std::fs::remove_file(&p).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod project_path_tests {
    use super::{ensure_not_project_root, resolve_project_path};
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[test]
    fn accepts_existing_child_in_any_project_root() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        let file = second.path().join("src").join("main.rs");
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(&file, "fn main() {}").unwrap();

        let resolved = resolve_project_path(
            &[first.path().to_path_buf(), second.path().to_path_buf()],
            &file.to_string_lossy(),
        )
        .unwrap();
        assert_eq!(resolved, file);
    }

    #[test]
    fn rejects_parent_dir_even_when_lexically_below_root() {
        let root = TempDir::new().unwrap();
        let escaped = root.path().join("nested").join("..").join("outside.txt");
        let error = resolve_project_path(&[root.path().to_path_buf()], &escaped.to_string_lossy())
            .unwrap_err();
        assert!(error.contains(".."));
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        use std::os::unix::fs::symlink;

        let root = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let link = root.path().join("outside-link");
        symlink(outside.path(), &link).unwrap();

        let existing = link.join(".");
        assert!(
            resolve_project_path(&[root.path().to_path_buf()], &existing.to_string_lossy())
                .is_err()
        );

        let missing = link.join("new").join("file.txt");
        assert!(
            resolve_project_path(&[root.path().to_path_buf()], &missing.to_string_lossy()).is_err()
        );
    }

    #[test]
    fn accepts_missing_write_path_when_nearest_parent_is_inside_root() {
        let root = TempDir::new().unwrap();
        let existing_parent = root.path().join("existing");
        std::fs::create_dir(&existing_parent).unwrap();
        let missing = existing_parent.join("new").join("deep").join("file.txt");

        let resolved =
            resolve_project_path(&[root.path().to_path_buf()], &missing.to_string_lossy()).unwrap();
        assert_eq!(resolved, missing);
    }

    #[test]
    fn protects_each_project_root_from_mutation() {
        let first = TempDir::new().unwrap();
        let second = TempDir::new().unwrap();
        let roots: Vec<PathBuf> = vec![first.path().to_path_buf(), second.path().to_path_buf()];

        let error = ensure_not_project_root(&roots, second.path(), "删除").unwrap_err();
        assert!(error.contains("项目根目录"));

        let child = second.path().join("child");
        std::fs::create_dir(&child).unwrap();
        ensure_not_project_root(&roots, &child, "删除").unwrap();
    }
}
