//! 跨平台剪贴板文件读写（通过 tauri-plugin-clipboard）。

use std::path::PathBuf;
use tauri_plugin_clipboard::Clipboard;

/// 将本地文件路径写入系统剪贴板。
pub fn write_paths(clipboard: &Clipboard, paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("没有可复制的文件".into());
    }
    for p in paths {
        if !p.exists() {
            return Err(format!("路径不存在: {}", p.display()));
        }
    }

    let uris: Vec<String> = paths.iter().map(|p| path_to_file_uri(p)).collect();
    clipboard.write_files_uris(uris)
}

/// 从系统剪贴板读取文件路径。
pub fn read_paths(clipboard: &Clipboard) -> Result<Vec<PathBuf>, String> {
    let has = clipboard.has_files().unwrap_or(false);
    if has {
        let files = clipboard.read_files()?;
        let paths: Vec<PathBuf> = files
            .into_iter()
            .map(PathBuf::from)
            .filter(|p| p.exists())
            .collect();
        if !paths.is_empty() {
            return Ok(paths);
        }
    }

    // 回退：尝试从纯文本解析路径
    if let Ok(text) = clipboard.read_text() {
        let paths = parse_path_lines(&text);
        if !paths.is_empty() {
            return Ok(paths);
        }
    }

    Err("剪贴板中没有文件".into())
}

fn path_to_file_uri(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    #[cfg(target_os = "windows")]
    {
        s.replace('\\', "/");
        format!("file:///{}", s.replace('\\', "/"))
    }
    #[cfg(not(target_os = "windows"))]
    {
        format!("file://{s}")
    }
}

fn parse_path_lines(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let raw = l.strip_prefix("file://").unwrap_or(l);
            let path = if raw.len() >= 3
                && raw.as_bytes().first() == Some(&b'/')
                && raw
                    .as_bytes()
                    .get(1)
                    .is_some_and(|b| b.is_ascii_alphabetic())
                && raw.as_bytes().get(2) == Some(&b':')
            {
                PathBuf::from(&raw[1..])
            } else {
                PathBuf::from(raw)
            };
            if path.is_absolute() && path.exists() {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}
