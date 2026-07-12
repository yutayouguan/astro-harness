//! 剪贴板文件读取与拖放附件辅助。

use std::path::PathBuf;

/// `write_paths`。
pub fn write_paths(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("没有可复制的文件".into());
    }
    #[cfg(target_os = "macos")]
    {
        return write_macos(paths);
    }
    #[cfg(target_os = "windows")]
    {
        return write_windows(paths);
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return write_linux(paths);
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        let _ = paths;
        Err("当前平台不支持文件剪贴板".into())
    }
}

/// `read_paths`。
pub fn read_paths() -> Result<Vec<PathBuf>, String> {
    #[cfg(target_os = "macos")]
    {
        return read_macos();
    }
    #[cfg(target_os = "windows")]
    {
        return read_windows();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        return read_linux();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", unix)))]
    {
        Err("当前平台不支持文件剪贴板".into())
    }
}

#[cfg(target_os = "macos")]
/// macOS：将文件路径写入剪贴板。
fn write_macos(paths: &[PathBuf]) -> Result<(), String> {
    let list = paths
        .iter()
        .map(|p| format!("POSIX file \"{}\"", p.to_string_lossy().replace('"', "\\\"")))
        .collect::<Vec<_>>()
        .join(", ");
    let script = format!("set the clipboard to {{{list}}}");
    let status = std::process::Command::new("osascript")
        .args(["-e", &script])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("复制到剪贴板失败".into());
    }
    Ok(())
}

#[cfg(target_os = "macos")]
/// macOS：从剪贴板读取文件路径。
fn read_macos() -> Result<Vec<PathBuf>, String> {
    let output = std::process::Command::new("osascript")
        .args([
            "-e",
            "try\nset theItems to the clipboard as «class furl»\non error\nreturn \"\"\nend try\nset out to \"\"\nrepeat with i in theItems\nset out to out & POSIX path of i & linefeed\nend repeat\nreturn out",
        ])
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("读取剪贴板失败".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    if paths.is_empty() {
        return Err("剪贴板中没有文件".into());
    }
    Ok(paths)
}

#[cfg(target_os = "windows")]
/// Windows：将文件路径写入剪贴板。
fn write_windows(paths: &[PathBuf]) -> Result<(), String> {
    // PowerShell: Set-Clipboard 支持 -Path（Win10+）
    let joined = paths
        .iter()
        .map(|p| format!("'{}'", p.to_string_lossy().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!("Set-Clipboard -Path @({joined})");
    let status = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", &script])
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err("复制到剪贴板失败".into());
    }
    Ok(())
}

#[cfg(target_os = "windows")]
/// Windows：从剪贴板读取文件路径。
fn read_windows() -> Result<Vec<PathBuf>, String> {
    let script = r#"
$files = Get-Clipboard -Format FileDropList -ErrorAction SilentlyContinue
if (-not $files) { exit 2 }
$files | ForEach-Object { $_.FullName }
"#;
    let output = std::process::Command::new("powershell")
        .args(["-NoProfile", "-Command", script])
        .output()
        .map_err(|e| e.to_string())?;
    if output.status.code() == Some(2) {
        return Err("剪贴板中没有文件".into());
    }
    if !output.status.success() {
        return Err("读取剪贴板失败".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let paths: Vec<PathBuf> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect();
    if paths.is_empty() {
        return Err("剪贴板中没有文件".into());
    }
    Ok(paths)
}

#[cfg(all(unix, not(target_os = "macos")))]
/// 本地路径 → `file://` URI。
fn path_to_file_uri(path: &std::path::Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::from("file://");
    for b in s.as_bytes() {
        match *b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'/' | b'.' | b'-' | b'_' | b'~' => {
                out.push(*b as char);
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(all(unix, not(target_os = "macos")))]
/// URI 百分号解码。
fn percent_decode(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let h = |c: u8| -> Option<u8> {
                match c {
                    b'0'..=b'9' => Some(c - b'0'),
                    b'a'..=b'f' => Some(c - b'a' + 10),
                    b'A'..=b'F' => Some(c - b'A' + 10),
                    _ => None,
                }
            };
            if let (Some(hi), Some(lo)) = (h(bytes[i + 1]), h(bytes[i + 2])) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(all(unix, not(target_os = "macos")))]
/// Linux：通过 URI 列表写入剪贴板。
fn write_linux(paths: &[PathBuf]) -> Result<(), String> {
    let uri_list = paths
        .iter()
        .map(|p| {
            let s = p.to_string_lossy();
            if s.starts_with("file://") {
                s.to_string()
            } else {
                path_to_file_uri(p)
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    // Prefer wl-copy, fall back to xclip
    let try_wl = std::process::Command::new("wl-copy")
        .arg("--type")
        .arg("text/uri-list")
        .arg(&uri_list)
        .status();
    if let Ok(st) = try_wl {
        if st.success() {
            return Ok(());
        }
    }
    let status = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-t", "text/uri-list"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(uri_list.as_bytes())?;
            }
            child.wait()
        })
        .map_err(|e| format!("需要 wl-copy 或 xclip: {e}"))?;
    if !status.success() {
        return Err("复制到剪贴板失败（需要 wl-copy 或 xclip）".into());
    }
    Ok(())
}

#[cfg(all(unix, not(target_os = "macos")))]
/// Linux：解析剪贴板中的文件 URI。
fn read_linux() -> Result<Vec<PathBuf>, String> {
    let output = std::process::Command::new("wl-paste")
        .args(["--type", "text/uri-list"])
        .output();
    let bytes = match output {
        Ok(o) if o.status.success() => o.stdout,
        _ => {
            let o = std::process::Command::new("xclip")
                .args(["-selection", "clipboard", "-t", "text/uri-list", "-o"])
                .output()
                .map_err(|e| format!("需要 wl-paste 或 xclip: {e}"))?;
            if !o.status.success() {
                return Err("读取剪贴板失败".into());
            }
            o.stdout
        }
    };
    let text = String::from_utf8_lossy(&bytes);
    let paths: Vec<PathBuf> = text
        .lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(|l| {
            let u = l.strip_prefix("file://").unwrap_or(l);
            PathBuf::from(percent_decode(u))
        })
        .collect();
    if paths.is_empty() {
        return Err("剪贴板中没有文件".into());
    }
    Ok(paths)
}
