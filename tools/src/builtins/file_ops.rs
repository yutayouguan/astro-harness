//! 文件操作工具：在工作区内读写、列举、删除文件与目录。
//!
//! 所有路径均相对于 Agent 工作区，经 [`crate::path_safe::resolve_safe`] 校验，
//! 禁止访问 workspace 之外的文件系统。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `file_ops` 工具的参数结构。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FileOpsArgs {
    /// 相对于工作区的路径。
    pub path: String,
    /// 操作类型：`read` | `write` | `append` | `list` | `delete` | `mkdir`。
    pub operation: String,
    /// `write` / `append` 时写入的内容；其他操作可省略。
    #[serde(default)]
    pub content: Option<String>,
}

/// 向注册表注册 `file_ops` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "file_ops".to_string(),
        toolset: "file_ops".to_string(),
        description: "Read, write, append, list, or delete files under the agent workspace."
            .to_string(),
        schema: schema_for_args::<FileOpsArgs>(),
        check_fn: None,
        icon: "folder-kanban",
    });
}

/// 按 `operation` 执行文件系统操作。
///
/// 路径经 `resolve_safe` 解析；`write`/`append`/`mkdir` 会自动创建父目录。
pub fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: FileOpsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("file_ops 参数无效: {e}"))?;
    let op = parsed.operation.trim().to_lowercase();
    let full = crate::path_safe::resolve_safe(&ctx.workspace_dir, &parsed.path)?;

    match op.as_str() {
        "read" => {
            let text = std::fs::read_to_string(&full)?;
            Ok(text)
        }
        "write" => {
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let content = parsed.content.unwrap_or_default();
            std::fs::write(&full, content)?;
            Ok(format!("已写入 {}", full.display()))
        }
        "append" => {
            use std::io::Write;
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let content = parsed.content.unwrap_or_default();
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full)?;
            f.write_all(content.as_bytes())?;
            Ok(format!("已追加 {}", full.display()))
        }
        "list" => {
            let dir = if full.is_dir() {
                full
            } else {
                full.parent()
                    .unwrap_or(&ctx.workspace_dir)
                    .to_path_buf()
            };
            let mut names = Vec::new();
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                names.push(entry.file_name().to_string_lossy().to_string());
            }
            names.sort();
            Ok(names.join("\n"))
        }
        "delete" => {
            if full.is_dir() {
                std::fs::remove_dir_all(&full)?;
            } else {
                std::fs::remove_file(&full)?;
            }
            Ok(format!("已删除 {}", full.display()))
        }
        "mkdir" => {
            std::fs::create_dir_all(&full)?;
            Ok(format!("已创建目录 {}", full.display()))
        }
        other => anyhow::bail!("未知 operation: {other}"),
    }
}
