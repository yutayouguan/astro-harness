//! 工具结果落盘与可恢复引用（Cursor 式「大结果外部化」）。
//!
//! 不变量：
//! - `ResponseItem` / DB `item_json` 始终保留全文（审计、FTS、UI）
//! - `compressed_content` 可改为 spill / prune 视图，供 Provider 读取
//! - 落盘路径：`{memory_dir}/sessions/tool_spills/{session_id}/{message_id}.txt`

use std::fs;
use std::path::{Path, PathBuf};

pub const TOOL_SPILL_MARK: &str = "[astro:tool-spill]";
pub const TOOL_PRUNE_MARK: &str = "[astro:tool-pruned]";
/// Agno 式逐条 LLM 摘要后的 Provider 视图标记。
pub const TOOL_LLM_COMPRESS_MARK: &str = "[astro:llm-compressed-tool-result]";

/// 超过该字节数时在记录阶段落盘并改写 provider 视图。
pub const DEFAULT_SPILL_THRESHOLD_BYTES: usize = 16 * 1024;

/// Hermes/Claude 风格 prune：尾部保护区外、超过该字符数的 tool 结果可廉价清除。
pub const PRUNE_MIN_CHARS: usize = 200;

const PREVIEW_CHARS: usize = 800;

pub fn spill_file_path(memory_dir: &Path, session_id: &str, message_id: i64) -> PathBuf {
    memory_dir
        .join("data")
        .join("tool_spills")
        .join(session_id)
        .join(format!("{message_id}.txt"))
}

/// 将全文写入 spill 文件；返回绝对路径。
pub fn write_tool_spill(
    memory_dir: &Path,
    session_id: &str,
    message_id: i64,
    content: &str,
) -> anyhow::Result<PathBuf> {
    let path = spill_file_path(memory_dir, session_id, message_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&path, content)?;
    Ok(path)
}

/// spill 文件相对 `memory_dir` 的路径，写入 provider 视图。
pub fn spill_path_for_prompt(memory_dir: &Path, absolute: &Path) -> String {
    absolute
        .strip_prefix(memory_dir)
        .unwrap_or(absolute)
        .to_string_lossy()
        .replace('\\', "/")
}

pub fn make_spill_view(
    tool_name: Option<&str>,
    spill_rel: &str,
    total_bytes: usize,
    content: &str,
) -> String {
    let name = tool_name.unwrap_or("unknown");
    let preview: String = content.chars().take(PREVIEW_CHARS).collect();
    let truncated = content.chars().count() > PREVIEW_CHARS;
    format!(
        "{TOOL_SPILL_MARK}\n\
         Tool: {name}\n\
         Bytes: {total_bytes}\n\
         Spill: {spill_rel}\n\
         Recovery: use `exec_command` to read the spill path with offset/limit, or `search` (scope=session).\n\
         \n\
         Preview{truncated_suffix}:\n{preview}",
        truncated_suffix = if truncated { " (truncated)" } else { "" },
    )
}

pub fn make_prune_view(tool_name: Option<&str>, spill_rel: Option<&str>) -> String {
    let name = tool_name.unwrap_or("unknown");
    let spill_line = spill_rel
        .map(|p| format!("\nSpill: {p}"))
        .unwrap_or_default();
    format!(
        "{TOOL_PRUNE_MARK}\n\
         Tool: {name}\n\
         Reason: context window maintenance (recent tail protected).{spill_line}\n\
         Full output remains in session DB. Recover via `search` (scope=session) or `exec_command` to read the spill path."
    )
}

pub fn is_externalized_view(s: &str) -> bool {
    let t = s.trim_start();
    t.starts_with(TOOL_SPILL_MARK)
        || t.starts_with(TOOL_PRUNE_MARK)
        || t.starts_with(TOOL_LLM_COMPRESS_MARK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spill_roundtrip_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = write_tool_spill(dir.path(), "sess-1", 42, "hello world").unwrap();
        assert!(path.exists());
        let rel = spill_path_for_prompt(dir.path(), &path);
        assert!(rel.contains("tool_spills/sess-1/42.txt"));
        let view = make_spill_view(Some("exec_command"), &rel, 11, "hello world");
        assert!(view.contains(TOOL_SPILL_MARK));
        assert!(view.contains("exec_command"));
    }

    #[test]
    fn prune_view_marks_externalized() {
        let v = make_prune_view(Some("grep"), None);
        assert!(is_externalized_view(&v));
    }
}
