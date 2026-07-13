//! 文件操作工具：在工作区内读写、列举、删除文件与目录。
//!
//! 所有路径均相对于 Agent 工作区，经 [`crate::path_safe::resolve_safe`] 校验，
//! 禁止访问 workspace 之外的文件系统。

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `read` 单次返回正文的默认/硬上限（字节）。
///
/// 64 KiB 介于「够读多数源码/配置」与「不撑爆 LLM 上下文」之间。
const MAX_READ_BYTES: usize = 64 * 1024;

/// `list` 最多返回的条目数。
const MAX_LIST_ENTRIES: usize = 500;

/// `list` 输出总字节软上限（与 read 对齐，避免目录名爆炸撑爆上下文）。
const MAX_LIST_BYTES: usize = 64 * 1024;

/// `file_ops` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FileOpsArgs {
    /// 相对于工作区的路径。
    pub path: String,
    /// 操作类型：`read` | `write` | `append` | `list` | `delete` | `mkdir`。
    pub operation: String,
    /// `write` / `append` 时写入的内容；`write` 必填，`append` 可省略则报错。
    #[serde(default)]
    pub content: Option<String>,
    /// `read` 时从该字节偏移开始读（默认 0）；用于大文件分段续读。
    #[serde(default)]
    pub offset: Option<u64>,
    /// `read` 时本次最多读取的字节数；超过 [`MAX_READ_BYTES`] 会被钳制。
    #[serde(default)]
    pub limit: Option<usize>,
    /// `delete` 目录时：为 `true` 才整树删除；默认仅删空目录。
    #[serde(default)]
    pub recursive: Option<bool>,
}

/// 向注册表注册 `file_ops` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "file_ops".to_string(),
        toolset: "file_ops".to_string(),
        description: "Read, write, append, list, mkdir, or delete files under the agent workspace. \
             read returns at most 64KiB UTF-8 (use offset/limit to continue). \
             list caps at 500 entries / 64KiB and marks dirs with '/'. \
             delete refuses workspace root; directories need recursive=true to remove trees."
            .to_string(),
        schema: schema_for_args::<FileOpsArgs>(),
        check_fn: None,
        icon: "folder-kanban",
    });
}

/// 按 `operation` 执行文件系统操作。
///
/// 路径经 `resolve_safe` 解析；`write`/`append`/`mkdir` 会自动创建父目录。
/// `read` / `list` 有字节或条目上限。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: FileOpsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("file_ops 参数无效: {e}"))?;
    let op = parsed.operation.trim().to_lowercase();
    let full = crate::path_safe::resolve_safe(&ctx.workspace_dir, &parsed.path)?;
    let rel = display_rel(&ctx.workspace_dir, &full);

    match op.as_str() {
        "read" => read_file_capped(&full, parsed.offset.unwrap_or(0), parsed.limit),
        "write" => {
            let content = parsed
                .content
                .ok_or_else(|| anyhow::anyhow!("write 需要 content 参数"))?;
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // resolve_safe 已拒绝越界 symlink；写前再确认最终路径仍在工作区
            reaffirm_within(&full, &ctx.workspace_dir)?;
            std::fs::write(&full, content.as_bytes())?;
            Ok(format!("已写入 {rel}"))
        }
        "append" => {
            use std::io::Write;
            let content = parsed
                .content
                .ok_or_else(|| anyhow::anyhow!("append 需要 content 参数"))?;
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            reaffirm_within(&full, &ctx.workspace_dir)?;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full)?;
            f.write_all(content.as_bytes())?;
            Ok(format!("已追加 {rel}"))
        }
        "list" => list_dir_capped(&full, &ctx.workspace_dir),
        "delete" => delete_path(&full, &ctx.workspace_dir, &rel, parsed.recursive.unwrap_or(false)),
        "mkdir" => {
            std::fs::create_dir_all(&full)?;
            Ok(format!("已创建目录 {rel}"))
        }
        other => anyhow::bail!("未知 operation: {other}"),
    }
}

fn display_rel(workspace: &Path, full: &Path) -> String {
    let ws = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    full.strip_prefix(&ws)
        .or_else(|_| full.strip_prefix(workspace))
        .map(|p| {
            let s = p.display().to_string();
            if s.is_empty() {
                ".".into()
            } else {
                s
            }
        })
        .unwrap_or_else(|_| full.display().to_string())
}

/// 按字节偏移读取文件，最多返回 `limit.min(MAX_READ_BYTES)` 字节的 UTF-8 文本。
fn read_file_capped(path: &Path, offset: u64, limit: Option<usize>) -> anyhow::Result<String> {
    let meta = std::fs::metadata(path)?;
    let total = meta.len();
    if offset > total {
        anyhow::bail!("offset {offset} 超出文件大小 {total} 字节");
    }

    let cap = limit.unwrap_or(MAX_READ_BYTES).min(MAX_READ_BYTES);
    let remaining = (total - offset) as usize;
    let to_read = remaining.min(cap);

    let mut file = std::fs::File::open(path)?;
    if offset > 0 {
        file.seek(SeekFrom::Start(offset))?;
    }
    let mut buf = vec![0u8; to_read];
    let n = file.read(&mut buf)?;
    buf.truncate(n);

    let text = decode_utf8_prefix(&buf)?;
    // 用解码后长度作续读偏移，便于末尾不完整 UTF-8 序列在下次读入时拼完整
    let next_offset = offset + text.len() as u64;
    let hit_cap = next_offset < total;

    if hit_cap {
        Ok(format!(
            "{text}\n\n[truncated] returned bytes {offset}..{next_offset} of {total} total. \
             Continue with offset={next_offset} (optional limit, max {MAX_READ_BYTES})."
        ))
    } else {
        Ok(text)
    }
}

/// 将字节前缀解码为 UTF-8；允许末尾不完整多字节序列被裁掉。
fn decode_utf8_prefix(buf: &[u8]) -> anyhow::Result<String> {
    match std::str::from_utf8(buf) {
        Ok(s) => Ok(s.to_string()),
        Err(e) => {
            let valid_up_to = e.valid_up_to();
            if valid_up_to > 0 && e.error_len().is_none() {
                Ok(std::str::from_utf8(&buf[..valid_up_to])
                    .expect("valid_up_to is char boundary")
                    .to_string())
            } else {
                anyhow::bail!(
                    "文件不是有效 UTF-8 文本（已读 {} 字节处非法）。二进制请勿用 file_ops read 整段灌入上下文。",
                    valid_up_to
                )
            }
        }
    }
}

fn list_dir_capped(full: &Path, workspace: &Path) -> anyhow::Result<String> {
    let dir = if full.is_dir() {
        full.to_path_buf()
    } else {
        full.parent()
            .unwrap_or(workspace)
            .to_path_buf()
    };

    let mut names: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().to_string();
        let is_dir = entry
            .file_type()
            .map(|t| t.is_dir())
            .unwrap_or(false);
        if is_dir {
            names.push(format!("{name}/"));
        } else {
            names.push(name);
        }
    }
    names.sort();

    let total = names.len();
    let mut out = String::new();
    let mut shown = 0usize;
    for name in &names {
        let line = if out.is_empty() {
            name.clone()
        } else {
            format!("\n{name}")
        };
        if shown >= MAX_LIST_ENTRIES || out.len() + line.len() > MAX_LIST_BYTES {
            out.push_str(&format!(
                "\n\n[truncated] listed {shown}/{total} entries \
                 (caps: {MAX_LIST_ENTRIES} entries / {MAX_LIST_BYTES} bytes)."
            ));
            return Ok(out);
        }
        out.push_str(&line);
        shown += 1;
    }
    Ok(out)
}

fn delete_path(
    full: &Path,
    workspace: &Path,
    rel: &str,
    recursive: bool,
) -> anyhow::Result<String> {
    let ws = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    // 根目录：用 symlink_metadata 判断自身，避免跟随
    if rel == "." || rel.is_empty() {
        anyhow::bail!("禁止删除 workspace 根目录");
    }
    if let Ok(canon) = full.canonicalize() {
        if canon == ws {
            anyhow::bail!("禁止删除 workspace 根目录");
        }
    }
    if full == workspace || full == ws {
        anyhow::bail!("禁止删除 workspace 根目录");
    }

    let meta = std::fs::symlink_metadata(full)?;
    if meta.file_type().is_symlink() {
        // 只删链接本身，不跟随
        std::fs::remove_file(full)?;
    } else if meta.is_dir() {
        if recursive {
            std::fs::remove_dir_all(full)?;
        } else {
            match std::fs::remove_dir(full) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::DirectoryNotEmpty => {
                    anyhow::bail!("目录非空：删除目录树需设置 recursive: true");
                }
                Err(e) => return Err(e.into()),
            }
        }
    } else {
        std::fs::remove_file(full)?;
    }
    Ok(format!("已删除 {rel}"))
}

/// 写操作前再次确认：若最终路径是 symlink，目标必须仍在 workspace 内（防 TOCTOU）。
fn reaffirm_within(path: &Path, workspace: &Path) -> anyhow::Result<()> {
    let base = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            let target = std::fs::canonicalize(path).map_err(|e| {
                anyhow::anyhow!("无法解析符号链接 {}: {e}", path.display())
            })?;
            if !target.starts_with(&base) {
                anyhow::bail!("路径越界：不允许访问 workspace 之外的文件");
            }
        }
    }
    if let Some(parent) = path.parent() {
        if parent.exists() {
            let canon = parent.canonicalize()?;
            if !canon.starts_with(&base) {
                anyhow::bail!("路径越界：不允许访问 workspace 之外的文件");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn write_ws_file(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn small_file_not_truncated() {
        let dir = TempDir::new().unwrap();
        let p = write_ws_file(&dir, "small.txt", b"hello tools");
        let out = read_file_capped(&p, 0, None).unwrap();
        assert_eq!(out, "hello tools");
        assert!(!out.contains("[truncated]"));
    }

    #[test]
    fn over_limit_truncated_with_hint() {
        let dir = TempDir::new().unwrap();
        let body = "a".repeat(MAX_READ_BYTES + 100);
        let p = write_ws_file(&dir, "big.txt", body.as_bytes());
        let out = read_file_capped(&p, 0, None).unwrap();
        assert!(out.contains("[truncated]"));
        assert!(out.contains(&format!("of {} total", body.len())));
        assert!(out.contains(&format!("offset={MAX_READ_BYTES}")));
        let content = out.split("\n\n[truncated]").next().unwrap();
        assert_eq!(content.len(), MAX_READ_BYTES);
    }

    #[test]
    fn offset_continues_after_truncate() {
        let dir = TempDir::new().unwrap();
        let body = format!("{}{}", "a".repeat(MAX_READ_BYTES), "TAIL");
        let p = write_ws_file(&dir, "cont.txt", body.as_bytes());
        let first = read_file_capped(&p, 0, None).unwrap();
        assert!(first.contains("[truncated]"));
        let second = read_file_capped(&p, MAX_READ_BYTES as u64, None).unwrap();
        assert_eq!(second, "TAIL");
        assert!(!second.contains("[truncated]"));
    }

    #[test]
    fn custom_limit_clamped_and_hinted() {
        let dir = TempDir::new().unwrap();
        let p = write_ws_file(&dir, "mid.txt", b"0123456789abcdef");
        let out = read_file_capped(&p, 0, Some(8)).unwrap();
        assert!(out.starts_with("01234567"));
        assert!(out.contains("[truncated]"));
        assert!(out.contains("offset=8"));
    }

    #[test]
    fn rejects_non_utf8() {
        let dir = TempDir::new().unwrap();
        let p = write_ws_file(&dir, "bin.dat", &[0xff, 0xfe, 0x00, 0x01]);
        let err = read_file_capped(&p, 0, None).unwrap_err();
        assert!(err.to_string().contains("UTF-8"));
    }

    #[test]
    fn incomplete_utf8_at_cap_boundary_trimmed() {
        let dir = TempDir::new().unwrap();
        let text = "你好世界";
        let p = write_ws_file(&dir, "zh.txt", text.as_bytes());
        let out = read_file_capped(&p, 0, Some(4)).unwrap();
        let content = out.split("\n\n[truncated]").next().unwrap();
        assert_eq!(content, "你");
        assert!(out.contains("[truncated]"));
    }

    #[test]
    fn list_marks_dirs_and_truncates() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("adir")).unwrap();
        fs::write(dir.path().join("a.txt"), "x").unwrap();
        let small = list_dir_capped(dir.path(), dir.path()).unwrap();
        assert!(small.contains("adir/"));
        assert!(small.contains("a.txt"));
        assert!(!small.contains("[truncated]"));

        for i in 0..(MAX_LIST_ENTRIES + 10) {
            fs::write(dir.path().join(format!("f{i:04}.txt")), "x").unwrap();
        }
        let out = list_dir_capped(dir.path(), dir.path()).unwrap();
        assert!(out.contains("[truncated]"));
        assert!(out.contains("entries"));
    }

    #[test]
    fn delete_refuses_workspace_root() {
        let dir = TempDir::new().unwrap();
        let err = delete_path(dir.path(), dir.path(), ".", false).unwrap_err();
        assert!(err.to_string().contains("根目录"));
    }

    #[test]
    fn delete_dir_requires_recursive() {
        let dir = TempDir::new().unwrap();
        let nested = dir.path().join("nest");
        fs::create_dir_all(nested.join("child")).unwrap();
        let err = delete_path(&nested, dir.path(), "nest", false).unwrap_err();
        assert!(err.to_string().contains("recursive"));
        delete_path(&nested, dir.path(), "nest", true).unwrap();
        assert!(!nested.exists());
    }
}
