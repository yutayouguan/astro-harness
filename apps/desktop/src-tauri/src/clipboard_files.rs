//! 跨平台剪贴板文件读写（macOS / Windows / Linux）。
//!
//! - macOS：NSPasteboard 文件 URL（读写均不依赖 AppleScript）
//! - Windows：PowerShell FileDropList（Win10+）
//! - Linux：`text/uri-list`（wl-clipboard / xclip / xsel），失败时回退绝对路径纯文本

use std::path::{Path, PathBuf};

#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
use std::io::Write;
#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
use std::process::{Command, Stdio};

/// 将本地文件路径写入系统剪贴板。
pub fn write_paths(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("没有可复制的文件".into());
    }
    for p in paths {
        if !p.exists() {
            return Err(format!("路径不存在: {}", p.display()));
        }
    }

    #[cfg(target_os = "macos")]
    {
        write_macos(paths)
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

/// 从系统剪贴板读取文件路径。
pub fn read_paths() -> Result<Vec<PathBuf>, String> {
    #[cfg(target_os = "macos")]
    {
        read_macos()
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

/// 解析剪贴板文本中的本地路径（`file://` / 绝对路径，多行）。
fn parse_path_lines(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(|l| l.trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let raw = l.strip_prefix("file://").unwrap_or(l);
            let decoded = percent_decode(raw);
            // Windows file URI: file:///C:/Users/... → /C:/Users/... → C:/Users/...
            let path = if decoded.len() >= 3
                && decoded.as_bytes()[0] == b'/'
                && decoded.as_bytes()[1].is_ascii_alphabetic()
                && decoded.as_bytes()[2] == b':'
            {
                PathBuf::from(&decoded[1..])
            } else {
                PathBuf::from(&decoded)
            };
            if path.is_absolute() && path.exists() {
                Some(path)
            } else {
                None
            }
        })
        .collect()
}

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

#[cfg_attr(not(test), allow(dead_code))]
fn path_to_file_uri(path: &Path) -> String {
    let s = path.to_string_lossy();
    // Windows: C:\a → file:///C:/a
    #[cfg(target_os = "windows")]
    {
        let normalized = s.replace('\\', "/");
        let mut out = String::from("file:///");
        for b in normalized.as_bytes() {
            match *b {
                b'A'..=b'Z'
                | b'a'..=b'z'
                | b'0'..=b'9'
                | b'/'
                | b'.'
                | b'-'
                | b'_'
                | b'~'
                | b':' => {
                    out.push(*b as char);
                }
                _ => out.push_str(&format!("%{b:02X}")),
            }
        }
        return out;
    }
    #[cfg(not(target_os = "windows"))]
    {
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
}

#[cfg(any(target_os = "windows", all(unix, not(target_os = "macos"))))]
fn paths_as_plain_text(paths: &[PathBuf]) -> String {
    paths
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("\n")
}

fn require_existing_files(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>, String> {
    let files: Vec<PathBuf> = paths
        .into_iter()
        .filter(|p| p.is_file() || p.is_dir())
        .collect();
    if files.is_empty() {
        Err("剪贴板中没有文件".into())
    } else {
        Ok(files)
    }
}

// ─── macOS ───────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn write_macos(paths: &[PathBuf]) -> Result<(), String> {
    use objc::runtime::{Class, Object};
    use objc::{msg_send, sel, sel_impl};
    use std::ffi::CString;

    unsafe {
        let ns_pasteboard_cls =
            Class::get("NSPasteboard").ok_or_else(|| "NSPasteboard 不可用".to_string())?;
        let ns_string_cls = Class::get("NSString").ok_or_else(|| "NSString 不可用".to_string())?;
        let ns_url_cls = Class::get("NSURL").ok_or_else(|| "NSURL 不可用".to_string())?;
        let ns_array_cls =
            Class::get("NSMutableArray").ok_or_else(|| "NSMutableArray 不可用".to_string())?;

        let pb: *mut Object = msg_send![ns_pasteboard_cls, generalPasteboard];
        if pb.is_null() {
            return Err("无法获取系统剪贴板".into());
        }
        let _: u64 = msg_send![pb, clearContents];

        let arr: *mut Object = msg_send![ns_array_cls, array];
        if arr.is_null() {
            return Err("无法创建剪贴板对象数组".into());
        }

        for path in paths {
            let raw = path.to_string_lossy();
            let c_path = CString::new(raw.as_ref()).map_err(|e| e.to_string())?;
            let ns_path: *mut Object =
                msg_send![ns_string_cls, stringWithUTF8String: c_path.as_ptr()];
            if ns_path.is_null() {
                return Err("路径编码失败".into());
            }
            let ns_url: *mut Object = msg_send![ns_url_cls, fileURLWithPath: ns_path];
            if ns_url.is_null() {
                return Err(format!("无效文件路径: {}", path.display()));
            }
            let _: () = msg_send![arr, addObject: ns_url];
        }

        let ok: bool = msg_send![pb, writeObjects: arr];
        if !ok {
            return Err("复制到剪贴板失败".into());
        }
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn read_macos() -> Result<Vec<PathBuf>, String> {
    use objc::runtime::{Class, Object};
    use objc::{msg_send, sel, sel_impl};
    use std::ffi::CStr;

    unsafe {
        let ns_pasteboard_cls =
            Class::get("NSPasteboard").ok_or_else(|| "NSPasteboard 不可用".to_string())?;
        let ns_array_cls = Class::get("NSArray").ok_or_else(|| "NSArray 不可用".to_string())?;
        let ns_url_cls = Class::get("NSURL").ok_or_else(|| "NSURL 不可用".to_string())?;
        let ns_dict_cls =
            Class::get("NSDictionary").ok_or_else(|| "NSDictionary 不可用".to_string())?;
        let ns_number_cls = Class::get("NSNumber").ok_or_else(|| "NSNumber 不可用".to_string())?;
        let ns_string_cls = Class::get("NSString").ok_or_else(|| "NSString 不可用".to_string())?;

        let pb: *mut Object = msg_send![ns_pasteboard_cls, generalPasteboard];
        if pb.is_null() {
            return Err("无法获取系统剪贴板".into());
        }

        // @[NSURL class]
        let classes: *mut Object = msg_send![ns_array_cls, arrayWithObject: ns_url_cls];
        // @{ NSPasteboardURLReadingFileURLsOnlyKey: @YES }
        let yes: *mut Object = msg_send![ns_number_cls, numberWithBool: true];
        let key_c = std::ffi::CString::new("NSPasteboardURLReadingFileURLsOnlyKey").unwrap();
        // Use the real constant string value — on Apple platforms the key is
        // NSPasteboardURLReadingFileURLsOnlyKey == "NSPasteboardURLReadingFileURLsOnlyKey"
        let key: *mut Object = msg_send![ns_string_cls, stringWithUTF8String: key_c.as_ptr()];
        let opts: *mut Object = msg_send![ns_dict_cls, dictionaryWithObject: yes forKey: key];

        let urls: *mut Object = msg_send![pb, readObjectsForClasses: classes options: opts];
        let mut paths = Vec::new();
        if !urls.is_null() {
            let count: usize = msg_send![urls, count];
            for i in 0..count {
                let url: *mut Object = msg_send![urls, objectAtIndex: i];
                if url.is_null() {
                    continue;
                }
                let ns_path: *mut Object = msg_send![url, path];
                if ns_path.is_null() {
                    continue;
                }
                let utf8: *const i8 = msg_send![ns_path, UTF8String];
                if utf8.is_null() {
                    continue;
                }
                let s = CStr::from_ptr(utf8).to_string_lossy().into_owned();
                let p = PathBuf::from(s);
                if p.exists() {
                    paths.push(p);
                }
            }
        }

        if paths.is_empty() {
            // 回退：纯文本绝对路径
            let type_c = std::ffi::CString::new("public.utf8-plain-text").unwrap();
            let type_s: *mut Object =
                msg_send![ns_string_cls, stringWithUTF8String: type_c.as_ptr()];
            let ns_text: *mut Object = msg_send![pb, stringForType: type_s];
            if !ns_text.is_null() {
                let utf8: *const i8 = msg_send![ns_text, UTF8String];
                if !utf8.is_null() {
                    let text = CStr::from_ptr(utf8).to_string_lossy();
                    paths = parse_path_lines(&text);
                }
            }
        }

        require_existing_files(paths)
    }
}

// ─── Windows ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "windows")]
fn write_windows(paths: &[PathBuf]) -> Result<(), String> {
    // Set-Clipboard -Path：写入 FileDropList，资源管理器 / 应用内粘贴均可识别
    let joined = paths
        .iter()
        .map(|p| format!("'{}'", p.to_string_lossy().replace('\'', "''")))
        .collect::<Vec<_>>()
        .join(",");
    let script = format!("$ErrorActionPreference='Stop'; Set-Clipboard -Path @({joined})");
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .status()
        .map_err(|e| format!("需要 PowerShell: {e}"))?;
    if !status.success() {
        // 回退：写入绝对路径纯文本，供应用内粘贴解析
        let text = paths_as_plain_text(paths).replace('\'', "''");
        let fallback = format!("Set-Clipboard -Value @'\n{text}\n'@");
        let st2 = Command::new("powershell")
            .args(["-NoProfile", "-NonInteractive", "-Command", &fallback])
            .status()
            .map_err(|e| e.to_string())?;
        if !st2.success() {
            return Err("复制到剪贴板失败".into());
        }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn read_windows() -> Result<Vec<PathBuf>, String> {
    let script = r#"
$ErrorActionPreference='Stop'
$files = Get-Clipboard -Format FileDropList -ErrorAction SilentlyContinue
if ($files) {
  $files | ForEach-Object { $_.FullName }
  exit 0
}
$raw = Get-Clipboard -Raw -ErrorAction SilentlyContinue
if ($raw) { Write-Output $raw; exit 0 }
exit 2
"#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .output()
        .map_err(|e| format!("需要 PowerShell: {e}"))?;
    if output.status.code() == Some(2) {
        return Err("剪贴板中没有文件".into());
    }
    if !output.status.success() {
        return Err("读取剪贴板失败".into());
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let paths = parse_path_lines(&text);
    require_existing_files(paths)
}

// ─── Linux ───────────────────────────────────────────────────────────────────

#[cfg(all(unix, not(target_os = "macos")))]
fn pipe_to_command(program: &str, args: &[&str], data: &[u8]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{program}: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(data)
            .map_err(|e| format!("写入 {program} stdin 失败: {e}"))?;
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{program} 退出码 {:?}", status.code()))
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn write_linux(paths: &[PathBuf]) -> Result<(), String> {
    let uri_list = paths
        .iter()
        .map(|p| path_to_file_uri(p))
        .collect::<Vec<_>>()
        .join("\n");
    let uri_bytes = uri_list.as_bytes();
    let plain = paths_as_plain_text(paths);
    let plain_bytes = plain.as_bytes();

    // 1) text/uri-list（文件管理器 / 浏览器友好）
    let uri_ok = pipe_to_command("wl-copy", &["--type", "text/uri-list", "--"], uri_bytes)
        .or_else(|_| {
            pipe_to_command(
                "xclip",
                &["-selection", "clipboard", "-t", "text/uri-list"],
                uri_bytes,
            )
        })
        .or_else(|_| {
            pipe_to_command(
                "xsel",
                &["--clipboard", "--input", "--mime-type", "text/uri-list"],
                uri_bytes,
            )
        });

    if uri_ok.is_ok() {
        return Ok(());
    }

    // 2) 纯文本绝对路径（应用内粘贴可解析）
    let plain_ok = pipe_to_command("wl-copy", &["--type", "text/plain", "--"], plain_bytes)
        .or_else(|_| {
            pipe_to_command(
                "xclip",
                &["-selection", "clipboard", "-t", "text/plain"],
                plain_bytes,
            )
        })
        .or_else(|_| pipe_to_command("xsel", &["--clipboard", "--input"], plain_bytes));

    plain_ok.map_err(|e| format!("复制到剪贴板失败（需要 wl-clipboard、xclip 或 xsel）: {e}"))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn read_command_stdout(program: &str, args: &[&str]) -> Option<Vec<u8>> {
    let output = Command::new(program)
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if output.status.success() && !output.stdout.is_empty() {
        Some(output.stdout)
    } else {
        None
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn read_linux() -> Result<Vec<PathBuf>, String> {
    // 优先 uri-list，再纯文本
    let bytes = read_command_stdout("wl-paste", &["--type", "text/uri-list"])
        .or_else(|| {
            read_command_stdout(
                "xclip",
                &["-selection", "clipboard", "-t", "text/uri-list", "-o"],
            )
        })
        .or_else(|| read_command_stdout("wl-paste", &["--type", "text/plain"]))
        .or_else(|| {
            read_command_stdout(
                "xclip",
                &["-selection", "clipboard", "-t", "text/plain", "-o"],
            )
        })
        .or_else(|| read_command_stdout("xsel", &["--clipboard", "--output"]))
        .ok_or_else(|| "读取剪贴板失败（需要 wl-clipboard、xclip 或 xsel）".to_string())?;

    let text = String::from_utf8_lossy(&bytes);
    let paths = parse_path_lines(&text);
    require_existing_files(paths)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_posix_paths() {
        let dir = std::env::temp_dir();
        let file = dir.join(format!(
            "astro-clip-parse-{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::write(&file, b"x").unwrap();
        let text = format!("{}\n# comment\n", file.display());
        let got = parse_path_lines(&text);
        assert_eq!(got, vec![file.clone()]);
        let _ = std::fs::remove_file(&file);
    }

    #[test]
    fn parse_file_uri() {
        let dir = std::env::temp_dir();
        let file = dir.join(format!(
            "astro-clip-uri-{}.txt",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::write(&file, b"x").unwrap();
        let uri = path_to_file_uri(&file);
        let got = parse_path_lines(&uri);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].canonicalize().unwrap(), file.canonicalize().unwrap());
        let _ = std::fs::remove_file(&file);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_write_read_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "astro-clip-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("sample.png");
        std::fs::write(&file, b"PNG").unwrap();

        write_paths(std::slice::from_ref(&file)).expect("write clipboard");
        let got = read_paths().expect("read clipboard");
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].canonicalize().unwrap(), file.canonicalize().unwrap());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
