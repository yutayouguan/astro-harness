//! 文件操作工具：在工作区内读写、列举、搜索、改写、移动、复制、删除文件与目录。
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

/// `search` 最多返回的命中条数。
const MAX_SEARCH_HITS: usize = 50;

/// `search` 输出总字节软上限。
const MAX_SEARCH_BYTES: usize = 64 * 1024;

/// `search` 扫描单文件内容的大小上限（更大则只匹配文件名）。
const MAX_SEARCH_FILE_BYTES: u64 = 1024 * 1024;

/// 递归搜索时最多访问的文件数（防超大目录拖垮）。
const MAX_SEARCH_FILES_SCANNED: usize = 2000;

/// `file_ops` tool args.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct FileOpsArgs {
    /// Workspace-relative path.
    pub path: String,
    /// Operation: `read` | `write` | `append` | `list` | `delete` | `mkdir` | `search` | `patch` | `move` | `copy`.
    pub operation: String,
    /// Content for `write` / `append` (`write` required).
    #[serde(default)]
    pub content: Option<String>,
    /// Search query (filename or content; case-insensitive). `content` also accepted.
    #[serde(default)]
    pub query: Option<String>,
    /// Patch: text to replace (must be unique unless `replace_all=true`).
    #[serde(default)]
    pub old_string: Option<String>,
    #[serde(default)]
    pub new_string: Option<String>,
    #[serde(default)]
    pub replace_all: Option<bool>,
    #[serde(default)]
    pub dest: Option<String>,
    /// Read: byte offset (default 0).
    #[serde(default)]
    pub offset: Option<u64>,
    /// Read: max bytes (capped at 64KiB). Search: max hits (default/cap 50).
    #[serde(default)]
    pub limit: Option<usize>,
    /// Read: start line (1-based, inclusive).
    #[serde(default)]
    pub start_line: Option<usize>,
    /// Read: end line (1-based, inclusive); omitted reads to EOF (still byte-capped).
    #[serde(default)]
    pub end_line: Option<usize>,
    /// Search: treat `query` as case-insensitive regex.
    #[serde(default)]
    pub regex: Option<bool>,
    /// Search/list: extension filter, e.g. `rs,toml` (no dots).
    #[serde(default)]
    pub ext: Option<String>,
    /// Delete dirs: true to remove trees. List: true for recursive listing.
    #[serde(default)]
    pub recursive: Option<bool>,
}

/// 向注册表注册 `file_ops` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "file_ops".to_string(),
        toolset: "file_ops".to_string(),
        description: "File ops under project_root or agent workspace: read, write, append, list, mkdir, delete, search, patch, move, copy. Paths are workspace-relative; read/list/search outputs are size-capped."
            .to_string(),
        schema: schema_for_args::<FileOpsArgs>(),
        check_fn: None,
        icon: "folder-kanban",
            ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["file_ops"],
    async_ctx: dispatch,
}

/// 把写入 memory 工作区的文件登记为本会话产物（关联 session_id），
/// 使其在文件空间归属当前会话，而非 reconcile 扫盘后的「未关联会话」。
///
/// project_root（委派 worktree / 代码仓）模式写入不属于记忆产物，跳过。
///
/// `rel` 为相对 workspace 的路径。这里刻意用非规范化的 `workspace_dir.join(rel)`
/// 拼绝对路径，与 reconcile 扫盘（`walkdir(memory_dir)`）及媒体登记的路径形态一致；
/// 若改用 `resolve_safe` 得到的 canonicalize 结果，遇到含符号链接的记忆根目录会与
/// reconcile 产生两条不同 path 的记录（一条已关联、一条未关联）。
async fn register_workspace_artifact(ctx: &ToolContext<'_>, rel: &str) {
    if ctx.project_root.is_some() || ctx.session_id.trim().is_empty() {
        return;
    }
    let abs = ctx.workspace_dir.join(rel);
    let Some(path) = abs.to_str() else {
        return;
    };
    if let Ok(db) = artifacts::open_default(&ctx.memory_dir).await {
        let agent_id = ctx.agent_id();
        let _ = db.register(
            path,
            artifacts::ArtifactSource::AgentWrite,
            Some(&ctx.session_id),
            None,
            Some(&agent_id),
        ).await;
    }
}

/// 写入 HTML 文件时构造 `ToolOutput::Media`，使前端活动卡渲染可预览的 HTML 卡片。
///
/// 前端 `commands.rs` 已把 `file` 类且扩展名为 html/htm 的 sidecar 映射为 html 预览。
fn maybe_html_sidecar(text: String, rel: &str) -> types::ToolOutput {
    let is_html = rel
        .rsplit('.')
        .next()
        .is_some_and(|e| matches!(e.to_ascii_lowercase().as_str(), "html" | "htm"));
    if is_html {
        let asset = types::MediaAsset::workspace(types::MediaKind::File, rel, "text/html");
        types::ToolOutput::Media {
            text,
            assets: vec![asset],
        }
    } else {
        text.into()
    }
}

/// 按 `operation` 执行文件系统操作。
///
/// 路径经 `resolve_safe` 解析；`write`/`append`/`mkdir`/`move`/`copy` 会自动创建父目录。
/// `read` / `list` / `search` 有字节或条目上限。
pub async fn dispatch(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<types::ToolOutput> {
    let parsed: FileOpsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("file_ops 参数无效: {e}"))?;
    let op = parsed.operation.trim().to_lowercase();
    let root = ctx.project_or_workspace();
    let full = crate::path_safe::resolve_safe(root, &parsed.path)?;
    let rel = display_rel(root, &full);
    let active_profile = if is_mutating_operation(&op) {
        let settings = memory::load_permission_settings(&ctx.memory_dir);
        let selected_profile = ctx
            .permission_profile
            .clone()
            .unwrap_or(settings.selection.profile_id);
        let profile_id =
            if ctx.workspace_write_grant && selected_profile == types::READ_ONLY_PROFILE {
                types::WORKSPACE_PROFILE.to_string()
            } else {
                selected_profile
            };
        if !matches!(op.as_str(), "copy" | "cp") {
            enforce_file_mutation_policy(&profile_id, root, &full)?;
        }
        Some(profile_id)
    } else {
        None
    };

    match op.as_str() {
        "read" => {
            if parsed.start_line.is_some() || parsed.end_line.is_some() {
                read_lines_range(&full, &rel, parsed.start_line, parsed.end_line).map(Into::into)
            } else {
                read_file_capped(&full, parsed.offset.unwrap_or(0), parsed.limit).map(Into::into)
            }
        }
        "write" => {
            let content = parsed
                .content
                .ok_or_else(|| anyhow::anyhow!("write 需要 content 参数"))?;
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            // resolve_safe 已拒绝越界 symlink；写前再确认最终路径仍在沙箱根
            reaffirm_within(&full, root)?;
            std::fs::write(&full, content.as_bytes())?;
            register_workspace_artifact(ctx, &rel).await;
            Ok(maybe_html_sidecar(format!("已写入 {rel}"), &rel))
        }
        "append" => {
            use std::io::Write;
            let content = parsed
                .content
                .ok_or_else(|| anyhow::anyhow!("append 需要 content 参数"))?;
            if let Some(parent) = full.parent() {
                std::fs::create_dir_all(parent)?;
            }
            reaffirm_within(&full, root)?;
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&full)?;
            f.write_all(content.as_bytes())?;
            register_workspace_artifact(ctx, &rel).await;
            Ok(maybe_html_sidecar(format!("已追加 {rel}"), &rel))
        }
        "list" => list_dir_capped(
            &full,
            root,
            parsed.recursive.unwrap_or(false),
            ext_filter(&parsed.ext),
        )
        .map(Into::into),
        "search" => {
            let query = parsed
                .query
                .as_deref()
                .or(parsed.content.as_deref())
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("search 需要 query（或 content）参数"))?;
            let max_hits = parsed
                .limit
                .unwrap_or(MAX_SEARCH_HITS)
                .clamp(1, MAX_SEARCH_HITS);
            let matcher = build_matcher(query, parsed.regex.unwrap_or(false))?;
            search_under(
                &full,
                root,
                query,
                &matcher,
                max_hits,
                ext_filter(&parsed.ext),
            )
            .map(Into::into)
        }
        "patch" => {
            let old = parsed
                .old_string
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("patch 需要 old_string"))?;
            let new = parsed
                .new_string
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("patch 需要 new_string"))?;
            reaffirm_within(&full, root)?;
            patch_file(&full, &rel, old, new, parsed.replace_all.unwrap_or(false))
                .map(|msg| maybe_html_sidecar(msg, &rel))
        }
        "move" | "rename" | "mv" => {
            let dest_rel = parsed
                .dest
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("move 需要 dest 参数"))?;
            let dest_full = crate::path_safe::resolve_safe(root, dest_rel)?;
            enforce_file_mutation_policy(
                active_profile
                    .as_deref()
                    .expect("move aliases are mutating operations"),
                root,
                &dest_full,
            )?;
            move_path(&full, &dest_full, root).map(Into::into)
        }
        "copy" | "cp" => {
            let dest_rel = parsed
                .dest
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("copy 需要 dest 参数"))?;
            let dest_full = crate::path_safe::resolve_safe(root, dest_rel)?;
            enforce_file_mutation_policy(
                active_profile
                    .as_deref()
                    .expect("copy aliases are mutating operations"),
                root,
                &dest_full,
            )?;
            copy_path(&full, &dest_full, root).map(Into::into)
        }
        "delete" => {
            delete_path(&full, root, &rel, parsed.recursive.unwrap_or(false)).map(Into::into)
        }
        "mkdir" => {
            std::fs::create_dir_all(&full)?;
            Ok(format!("已创建目录 {rel}").into())
        }
        other => anyhow::bail!("未知 operation: {other}"),
    }
}

fn is_mutating_operation(operation: &str) -> bool {
    matches!(
        operation,
        "write"
            | "append"
            | "delete"
            | "mkdir"
            | "patch"
            | "move"
            | "rename"
            | "mv"
            | "copy"
            | "cp"
    )
}

fn enforce_file_mutation_policy(
    profile_id: &str,
    root: &Path,
    target: &Path,
) -> anyhow::Result<()> {
    match profile_id {
        types::READ_ONLY_PROFILE => {
            anyhow::bail!("permission denied: read-only profile does not allow file mutations")
        }
        types::WORKSPACE_PROFILE => {
            let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
            for protected in [".git", ".agents", ".astro"] {
                if target.starts_with(root.join(protected))
                    || target.starts_with(canonical_root.join(protected))
                {
                    anyhow::bail!(
                        "permission denied: workspace metadata path {protected} is read-only"
                    );
                }
            }
            Ok(())
        }
        types::DANGER_FULL_ACCESS_PROFILE => Ok(()),
        custom => anyhow::bail!(
            "custom permission profile {custom:?} is not executable until its filesystem rules are fully resolved"
        ),
    }
}

/// 把逗号分隔的扩展名列表解析为小写去点集合；空则返回 `None`（不过滤）。
fn ext_filter(raw: &Option<String>) -> Option<Vec<String>> {
    let raw = raw.as_deref()?.trim();
    if raw.is_empty() {
        return None;
    }
    let exts: Vec<String> = raw
        .split(',')
        .map(|s| s.trim().trim_start_matches('.').to_lowercase())
        .filter(|s| !s.is_empty())
        .collect();
    if exts.is_empty() {
        None
    } else {
        Some(exts)
    }
}

/// 判断路径扩展名是否落在过滤集合内；无过滤时恒为 `true`。
fn ext_matches(path: &Path, filter: &Option<Vec<String>>) -> bool {
    match filter {
        None => true,
        Some(exts) => path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| exts.iter().any(|x| x == &e.to_lowercase()))
            .unwrap_or(false),
    }
}

/// 内容匹配器：正则或大小写不敏感子串。
enum Matcher {
    Substr(String),
    Regex(regex::Regex),
}

impl Matcher {
    fn is_match(&self, haystack: &str) -> bool {
        match self {
            Matcher::Substr(needle) => haystack.to_lowercase().contains(needle),
            Matcher::Regex(re) => re.is_match(haystack),
        }
    }
}

/// 构造匹配器；`use_regex=true` 时编译大小写不敏感正则。
fn build_matcher(query: &str, use_regex: bool) -> anyhow::Result<Matcher> {
    if use_regex {
        let re = regex::RegexBuilder::new(query)
            .case_insensitive(true)
            .build()
            .map_err(|e| anyhow::anyhow!("search 正则无效: {e}"))?;
        Ok(Matcher::Regex(re))
    } else {
        Ok(Matcher::Substr(query.to_lowercase()))
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
                // 末尾多字节序列不完整：裁掉，下次续读补齐
                Ok(std::str::from_utf8(&buf[..valid_up_to])
                    .expect("valid_up_to is char boundary")
                    .to_string())
            } else if valid_up_to == 0 && e.error_len().is_none() {
                // 整个窗口只装下一个被截断的多字节字符：这不是二进制，而是窗口/偏移问题
                let first = buf.first().copied().unwrap_or(0);
                if first & 0b1100_0000 == 0b1000_0000 {
                    anyhow::bail!(
                        "read 的 offset 落在多字节字符中间（首字节为 UTF-8 续接字节）。请把 offset 对齐到字符边界，通常直接用上次返回的续读 offset。"
                    )
                } else {
                    anyhow::bail!(
                        "limit 太小，无法容纳当前位置的单个多字节字符，请增大 limit（至少 4 字节）后重试。"
                    )
                }
            } else {
                anyhow::bail!(
                    "文件不是有效 UTF-8 文本（已读 {} 字节处非法）。二进制请勿用 file_ops read 整段灌入上下文。",
                    valid_up_to
                )
            }
        }
    }
}

/// 按行范围读取（1 起，`start_line`/`end_line` 含端点）。
///
/// 仅支持 UTF-8 文本；输出总量仍受 [`MAX_READ_BYTES`] 约束，超出时截断并提示。
fn read_lines_range(
    path: &Path,
    rel: &str,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> anyhow::Result<String> {
    let start = start_line.unwrap_or(1).max(1);
    if let (Some(s), Some(e)) = (start_line, end_line) {
        if e < s {
            anyhow::bail!("read 的 end_line({e}) 不能小于 start_line({s})");
        }
    }
    let end = end_line.unwrap_or(usize::MAX).max(start);

    let meta = std::fs::metadata(path)?;
    // 行模式需整读文件；给一个宽松上限，避免对超大文件误用
    const MAX_LINE_MODE_FILE: u64 = 8 * 1024 * 1024;
    if meta.len() > MAX_LINE_MODE_FILE {
        anyhow::bail!(
            "文件过大（{} 字节 > 8MiB），行范围读取不可用。请改用 offset/limit 字节分段读取: {rel}",
            meta.len()
        );
    }
    let raw = std::fs::read(path)?;
    let text = std::str::from_utf8(&raw)
        .map_err(|_| anyhow::anyhow!("read 行范围仅支持 UTF-8 文本文件: {rel}"))?;

    let total_lines = text.lines().count();
    if start > total_lines {
        anyhow::bail!("start_line({start}) 超出文件总行数({total_lines})");
    }

    let mut out = String::new();
    let mut emitted = 0usize;
    let mut capped = false;
    let mut partial_line = false;
    for (idx, line) in text.lines().enumerate() {
        let ln = idx + 1;
        if ln < start {
            continue;
        }
        if ln > end {
            break;
        }
        let piece = if out.is_empty() {
            line.to_string()
        } else {
            format!("\n{line}")
        };
        if out.len() + piece.len() > MAX_READ_BYTES {
            capped = true;
            if out.is_empty() {
                // 单行本身就超上限：截断到字符边界后返回前缀，避免静默丢弃整行
                let mut cut = MAX_READ_BYTES.min(line.len());
                while cut > 0 && !line.is_char_boundary(cut) {
                    cut -= 1;
                }
                out.push_str(&line[..cut]);
                emitted = 1;
                partial_line = true;
            }
            break;
        }
        out.push_str(&piece);
        emitted += 1;
    }

    let last = start + emitted.saturating_sub(1);
    let header = format!("[lines {start}..{last} of {total_lines}]\n");
    if partial_line {
        Ok(format!(
            "{header}{out}\n\n[truncated] line {start} exceeds {MAX_READ_BYTES} bytes; only its prefix is shown. \
             Use byte-mode read (offset/limit) to page through this long line.",
        ))
    } else if capped {
        Ok(format!(
            "{header}{out}\n\n[truncated] line range exceeded {MAX_READ_BYTES} bytes; \
             continue with start_line={}.",
            last + 1
        ))
    } else {
        Ok(format!("{header}{out}"))
    }
}

/// 递归遍历时跳过的噪音目录（与 search 保持一致）。
const NOISE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    ".astro",
    "dist",
    "build",
    ".venv",
];

fn list_dir_capped(
    full: &Path,
    workspace: &Path,
    recursive: bool,
    ext: Option<Vec<String>>,
) -> anyhow::Result<String> {
    let dir = if full.is_dir() {
        full.to_path_buf()
    } else {
        full.parent().unwrap_or(workspace).to_path_buf()
    };

    let mut names: Vec<String> = Vec::new();
    if recursive {
        collect_tree(&dir, &dir, &ext, &mut names);
    } else {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
            let name = entry.file_name().to_string_lossy().to_string();
            if is_dir {
                names.push(format!("{name}/"));
            } else if ext_matches(&entry.path(), &ext) {
                names.push(name);
            }
        }
    }
    names.sort();

    let total = names.len();
    let mut out = String::new();
    for (shown, name) in names.iter().enumerate() {
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
    }
    Ok(out)
}

/// 递归收集目录树内的相对路径（目录带尾 `/`），跳过噪音目录与 symlink。
fn collect_tree(dir: &Path, base: &Path, ext: &Option<Vec<String>>, out: &mut Vec<String>) {
    if out.len() >= MAX_LIST_ENTRIES {
        return;
    }
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        if out.len() >= MAX_LIST_ENTRIES {
            return;
        }
        let path = entry.path();
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_symlink() {
            continue;
        }
        let rel = path
            .strip_prefix(base)
            .unwrap_or(&path)
            .to_string_lossy()
            .to_string();
        if ft.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if NOISE_DIRS.contains(&name.as_str()) {
                continue;
            }
            out.push(format!("{rel}/"));
            collect_tree(&path, base, ext, out);
        } else if ft.is_file() && ext_matches(&path, ext) {
            out.push(rel);
        }
    }
}

/// 在 `start`（文件或目录）下搜索文件名 / 文本内容。
fn search_under(
    start: &Path,
    workspace: &Path,
    query: &str,
    matcher: &Matcher,
    max_hits: usize,
    ext: Option<Vec<String>>,
) -> anyhow::Result<String> {
    let root = if start.is_dir() {
        start.to_path_buf()
    } else if start.is_file() {
        // 单文件：只扫这一份
        return search_one_file(start, workspace, matcher, &ext)
            .map(|hit| hit.unwrap_or_else(|| format!("未找到匹配「{query}」")));
    } else {
        anyhow::bail!("路径不存在: {}", display_rel(workspace, start));
    };

    let mut hits: Vec<String> = Vec::new();
    let mut scanned = 0usize;
    let mut truncated = false;
    let mut stack = vec![root];

    while let Some(dir) = stack.pop() {
        if hits.len() >= max_hits || scanned >= MAX_SEARCH_FILES_SCANNED {
            truncated = true;
            break;
        }
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(_) => continue,
        };
        let mut entries: Vec<_> = entries.filter_map(|e| e.ok()).collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries {
            if hits.len() >= max_hits || scanned >= MAX_SEARCH_FILES_SCANNED {
                truncated = true;
                break;
            }
            let path = entry.path();
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                // 跳过常见噪音目录
                let name = entry.file_name().to_string_lossy().to_string();
                if NOISE_DIRS.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
                continue;
            }
            if !ft.is_file() {
                continue;
            }
            scanned += 1;
            if let Some(hit) = search_one_file(&path, workspace, matcher, &ext)? {
                hits.push(hit);
            }
        }
    }

    if hits.is_empty() {
        return Ok(format!("未找到匹配「{query}」（已扫描 {scanned} 个文件）"));
    }

    let mut out = format!("找到 {} 处匹配「{query}」:\n", hits.len());
    for hit in &hits {
        let line = format!("\n{hit}");
        if out.len() + line.len() > MAX_SEARCH_BYTES {
            truncated = true;
            break;
        }
        out.push_str(&line);
    }
    if truncated {
        out.push_str(&format!(
            "\n\n[truncated] scanned={scanned}, hits={}, caps: {max_hits} hits / {MAX_SEARCH_FILES_SCANNED} files / {MAX_SEARCH_BYTES} bytes. Narrow path or query."
            , hits.len().min(max_hits)
        ));
    }
    Ok(out)
}

/// 替换文件文本：默认要求 `old` 唯一出现；`replace_all=true` 时替换全部匹配。
fn patch_file(
    path: &Path,
    rel: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> anyhow::Result<String> {
    if old.is_empty() {
        anyhow::bail!("patch 的 old_string 不能为空");
    }
    let raw = std::fs::read(path)?;
    let text = std::str::from_utf8(&raw)
        .map_err(|_| anyhow::anyhow!("patch 仅支持 UTF-8 文本文件: {rel}"))?;
    let matches = text.matches(old).count();
    if matches == 0 {
        anyhow::bail!("patch 未找到 old_string（0 处匹配）: {rel}");
    }
    if replace_all {
        let updated = text.replace(old, new);
        std::fs::write(path, updated.as_bytes())?;
        return Ok(format!("已 patch {rel}（{matches} 处替换）"));
    }
    match matches {
        1 => {
            let updated = text.replacen(old, new, 1);
            std::fs::write(path, updated.as_bytes())?;
            Ok(format!("已 patch {rel}（1 处替换）"))
        }
        n => anyhow::bail!(
            "patch 的 old_string 不唯一（{n} 处匹配），请提供更长/更独特的上下文，或设置 replace_all=true: {rel}"
        ),
    }
}

fn search_one_file(
    path: &Path,
    workspace: &Path,
    matcher: &Matcher,
    ext: &Option<Vec<String>>,
) -> anyhow::Result<Option<String>> {
    if !ext_matches(path, ext) {
        return Ok(None);
    }
    let rel = display_rel(workspace, path);
    let name_hit = path
        .file_name()
        .and_then(|n| n.to_str())
        .map(|n| matcher.is_match(n))
        .unwrap_or(false)
        || matcher.is_match(&rel);

    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(_) => return Ok(None),
    };
    if meta.len() > MAX_SEARCH_FILE_BYTES {
        return Ok(if name_hit {
            Some(format!(
                "{rel}  (filename match; file >1MiB, content skipped)"
            ))
        } else {
            None
        });
    }

    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(_) => {
            return Ok(if name_hit {
                Some(format!("{rel}  (filename match)"))
            } else {
                None
            })
        }
    };
    // 粗略跳过明显二进制
    if bytes.iter().take(512).any(|&b| b == 0) {
        return Ok(if name_hit {
            Some(format!("{rel}  (filename match; binary skipped)"))
        } else {
            None
        });
    }
    let Ok(text) = std::str::from_utf8(&bytes) else {
        return Ok(if name_hit {
            Some(format!("{rel}  (filename match; non-utf8 skipped)"))
        } else {
            None
        });
    };

    let mut line_hits: Vec<String> = Vec::new();
    for (idx, line) in text.lines().enumerate() {
        if matcher.is_match(line) {
            let trimmed = line.trim();
            let snippet = if trimmed.chars().count() > 160 {
                let s: String = trimmed.chars().take(160).collect();
                format!("{s}…")
            } else {
                trimmed.to_string()
            };
            line_hits.push(format!("L{}: {snippet}", idx + 1));
            if line_hits.len() >= 5 {
                break;
            }
        }
    }

    if line_hits.is_empty() {
        return Ok(if name_hit {
            Some(format!("{rel}  (filename match)"))
        } else {
            None
        });
    }

    Ok(Some(format!("{rel}\n  {}", line_hits.join("\n  "))))
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

/// 移动 / 重命名：源须存在，目标不得已存在；自动创建目标父目录。
fn move_path(src: &Path, dest: &Path, workspace: &Path) -> anyhow::Result<String> {
    let src_rel = display_rel(workspace, src);
    let dest_rel = display_rel(workspace, dest);
    if !src.exists() {
        anyhow::bail!("move 源路径不存在: {src_rel}");
    }
    if dest.exists() {
        anyhow::bail!("move 目标已存在: {dest_rel}（请先删除或换目标）");
    }
    reaffirm_within(dest, workspace)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    match std::fs::rename(src, dest) {
        Ok(()) => {}
        Err(_) => {
            // 跨设备等 rename 失败：退回 复制 + 删除
            copy_tree(src, dest)?;
            if src.is_dir() {
                std::fs::remove_dir_all(src)?;
            } else {
                std::fs::remove_file(src)?;
            }
        }
    }
    Ok(format!("已移动 {src_rel} → {dest_rel}"))
}

/// 复制文件或目录树：目标不得已存在；自动创建目标父目录。
fn copy_path(src: &Path, dest: &Path, workspace: &Path) -> anyhow::Result<String> {
    let src_rel = display_rel(workspace, src);
    let dest_rel = display_rel(workspace, dest);
    if !src.exists() {
        anyhow::bail!("copy 源路径不存在: {src_rel}");
    }
    if dest.exists() {
        anyhow::bail!("copy 目标已存在: {dest_rel}（请先删除或换目标）");
    }
    reaffirm_within(dest, workspace)?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    copy_tree(src, dest)?;
    Ok(format!("已复制 {src_rel} → {dest_rel}"))
}

/// 递归复制 `src` 到 `dest`（文件或目录），跳过 symlink（仅复制其目标已解析路径不适用，直接跳过链接）。
fn copy_tree(src: &Path, dest: &Path) -> anyhow::Result<()> {
    let meta = std::fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        anyhow::bail!("暂不支持复制符号链接: {}", src.display());
    }
    if meta.is_dir() {
        std::fs::create_dir_all(dest)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let child_src = entry.path();
            let child_dest = dest.join(entry.file_name());
            let ft = entry.file_type()?;
            if ft.is_symlink() {
                continue;
            }
            copy_tree(&child_src, &child_dest)?;
        }
    } else {
        std::fs::copy(src, dest)?;
    }
    Ok(())
}

/// 写操作前再次确认：若最终路径是 symlink，目标必须仍在 workspace 内（防 TOCTOU）。
fn reaffirm_within(path: &Path, workspace: &Path) -> anyhow::Result<()> {
    let base = workspace
        .canonicalize()
        .unwrap_or_else(|_| workspace.to_path_buf());
    if let Ok(meta) = std::fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            let target = std::fs::canonicalize(path)
                .map_err(|e| anyhow::anyhow!("无法解析符号链接 {}: {e}", path.display()))?;
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
    use crate::context::{ImageGenTargets, ToolContext};
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn test_ctx<'a>(
        dir: &'a TempDir,
        memory: &'a std::sync::RwLock<memory::MemoryManager>,
        sessions: &'a session::SessionStore,
        targets: &'a ImageGenTargets,
        creds: &'a crate::context::ModelCredentials,
    ) -> ToolContext<'a> {
        ToolContext {
            memory,
            sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: dir.path().join("workspace"),
            project_root: None,
            image_gen_targets: targets,
            session_id: "test".into(),
            turn_id: None,
            credentials: creds,
            chat_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            network_grant: crate::InProcessNetworkGrant::default(),
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
        }
    }

    fn write_ws_file(dir: &TempDir, name: &str, bytes: &[u8]) -> PathBuf {
        let p = dir.path().join(name);
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&p, bytes).unwrap();
        p
    }

    #[test]
    fn read_only_and_workspace_metadata_block_mutations() {
        let dir = TempDir::new().unwrap();
        let normal = dir.path().join("src/lib.rs");
        let git_config = dir.path().join(".git/config");
        assert!(
            enforce_file_mutation_policy(types::READ_ONLY_PROFILE, dir.path(), &normal)
                .unwrap_err()
                .to_string()
                .contains("read-only")
        );
        assert!(
            enforce_file_mutation_policy(types::WORKSPACE_PROFILE, dir.path(), &normal).is_ok()
        );
        assert!(
            enforce_file_mutation_policy(types::WORKSPACE_PROFILE, dir.path(), &git_config)
                .unwrap_err()
                .to_string()
                .contains("metadata")
        );
        assert!(enforce_file_mutation_policy(
            types::DANGER_FULL_ACCESS_PROFILE,
            dir.path(),
            &git_config
        )
        .is_ok());
    }

    #[test]
    fn dispatch_blocks_mutating_aliases_and_protected_destinations() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path().join("workspace");
        fs::create_dir_all(workspace.join(".git")).unwrap();
        fs::write(workspace.join("source.txt"), "source").unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let memory = std::sync::RwLock::new(memory);
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();

        memory::set_permission_preset(dir.path(), types::PermissionPreset::ReadOnly).unwrap();
        {
            let mut ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
            let error = dispatch(
                &ctx,
                &serde_json::json!({
                    "operation": "cp",
                    "path": "source.txt",
                    "dest": "copy.txt"
                }),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("read-only"), "{error}");
            assert!(!workspace.join("copy.txt").exists());

            ctx.workspace_write_grant = true;
            dispatch(
                &ctx,
                &serde_json::json!({
                    "operation": "cp",
                    "path": "source.txt",
                    "dest": "copy.txt"
                }),
            )
            .unwrap();
            assert_eq!(
                fs::read_to_string(workspace.join("copy.txt")).unwrap(),
                "source"
            );
        }
        {
            let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
            let error = dispatch(
                &ctx,
                &serde_json::json!({
                    "operation": "cp",
                    "path": "source.txt",
                    "dest": "second-copy.txt"
                }),
            )
            .unwrap_err()
            .to_string();
            assert!(error.contains("read-only"), "{error}");
            assert!(!workspace.join("second-copy.txt").exists());
        }

        memory::set_permission_preset(dir.path(), types::PermissionPreset::AskForApproval).unwrap();
        fs::write(workspace.join(".git/config"), "[core]").unwrap();
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        dispatch(
            &ctx,
            &serde_json::json!({
                "operation": "copy",
                "path": ".git/config",
                "dest": "saved-git-config"
            }),
        )
        .unwrap();
        assert_eq!(
            fs::read_to_string(workspace.join("saved-git-config")).unwrap(),
            "[core]"
        );
        let error = dispatch(
            &ctx,
            &serde_json::json!({
                "operation": "rename",
                "path": "source.txt",
                "dest": ".git/source.txt"
            }),
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("metadata"), "{error}");
        assert!(workspace.join("source.txt").exists());
        assert!(!workspace.join(".git/source.txt").exists());
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
        let small = list_dir_capped(dir.path(), dir.path(), false, None).unwrap();
        assert!(small.contains("adir/"));
        assert!(small.contains("a.txt"));
        assert!(!small.contains("[truncated]"));

        for i in 0..(MAX_LIST_ENTRIES + 10) {
            fs::write(dir.path().join(format!("f{i:04}.txt")), "x").unwrap();
        }
        let out = list_dir_capped(dir.path(), dir.path(), false, None).unwrap();
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

    #[test]
    fn search_matches_filename_and_content() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("src")).unwrap();
        fs::write(
            dir.path().join("src/hello.rs"),
            "fn main() { todo_marker(); }\n",
        )
        .unwrap();
        fs::write(dir.path().join("readme.md"), "no hit here\n").unwrap();
        fs::write(dir.path().join("todo_notes.txt"), "filename only\n").unwrap();

        let sub = build_matcher("todo_marker", false).unwrap();
        let out = search_under(dir.path(), dir.path(), "todo_marker", &sub, 20, None).unwrap();
        assert!(out.contains("src/hello.rs") || out.contains("src\\hello.rs"));
        assert!(out.contains("L1:"));

        let name_m = build_matcher("todo_notes", false).unwrap();
        let by_name =
            search_under(dir.path(), dir.path(), "todo_notes", &name_m, 20, None).unwrap();
        assert!(by_name.contains("todo_notes.txt"));
        assert!(by_name.contains("filename match"));
    }

    #[test]
    fn search_regex_and_ext_filter() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("a.rs"), "fn add(a: i32) {}\n").unwrap();
        fs::write(dir.path().join("b.txt"), "fn add(a: i32) {}\n").unwrap();

        // 正则匹配函数定义
        let re = build_matcher(r"fn\s+add", true).unwrap();
        let out = search_under(dir.path(), dir.path(), "fn add", &re, 20, None).unwrap();
        assert!(out.contains("a.rs"));
        assert!(out.contains("b.txt"));

        // 只搜 rs 扩展
        let re2 = build_matcher(r"fn\s+add", true).unwrap();
        let only_rs = search_under(
            dir.path(),
            dir.path(),
            "fn add",
            &re2,
            20,
            Some(vec!["rs".into()]),
        )
        .unwrap();
        assert!(only_rs.contains("a.rs"));
        assert!(!only_rs.contains("b.txt"));
    }

    #[test]
    fn search_empty_query_path_missing_handled_by_dispatch_contract() {
        let dir = TempDir::new().unwrap();
        let missing = dir.path().join("nope");
        let m = build_matcher("x", false).unwrap();
        let err = search_under(&missing, dir.path(), "x", &m, 10, None).unwrap_err();
        assert!(err.to_string().contains("不存在"));
    }

    #[test]
    fn patch_unique_replace_succeeds() {
        let dir = TempDir::new().unwrap();
        let p = write_ws_file(&dir, "a.rs", b"fn foo() {}\nfn bar() {}\n");
        let out = patch_file(&p, "a.rs", "fn foo() {}", "fn foo() { 1 }", false).unwrap();
        assert!(out.contains("1 处替换"));
        let body = fs::read_to_string(&p).unwrap();
        assert_eq!(body, "fn foo() { 1 }\nfn bar() {}\n");
    }

    #[test]
    fn patch_zero_matches_fails() {
        let dir = TempDir::new().unwrap();
        let p = write_ws_file(&dir, "a.txt", b"hello\n");
        let err = patch_file(&p, "a.txt", "missing", "x", false).unwrap_err();
        assert!(err.to_string().contains("0 处匹配"));
    }

    #[test]
    fn patch_multiple_matches_fails() {
        let dir = TempDir::new().unwrap();
        let p = write_ws_file(&dir, "a.txt", b"aa aa aa\n");
        let err = patch_file(&p, "a.txt", "aa", "bb", false).unwrap_err();
        assert!(err.to_string().contains("不唯一"));
        assert_eq!(fs::read_to_string(&p).unwrap(), "aa aa aa\n");
    }

    #[test]
    fn patch_replace_all_replaces_every_match() {
        let dir = TempDir::new().unwrap();
        let p = write_ws_file(&dir, "a.txt", b"aa aa aa\n");
        let out = patch_file(&p, "a.txt", "aa", "bb", true).unwrap();
        assert!(out.contains("3 处替换"));
        assert_eq!(fs::read_to_string(&p).unwrap(), "bb bb bb\n");
    }

    #[test]
    fn read_line_range_returns_slice_with_header() {
        let dir = TempDir::new().unwrap();
        let p = write_ws_file(&dir, "code.rs", b"l1\nl2\nl3\nl4\nl5\n");
        let out = read_lines_range(&p, "code.rs", Some(2), Some(4)).unwrap();
        assert!(out.contains("[lines 2..4 of 5]"));
        assert!(out.contains("l2\nl3\nl4"));
        assert!(!out.contains("l1"));
        assert!(!out.contains("l5"));
    }

    #[test]
    fn read_line_range_huge_single_line_returns_prefix() {
        let dir = TempDir::new().unwrap();
        let long = "x".repeat(MAX_READ_BYTES + 500);
        let body = format!("{long}\nsecond\n");
        let p = write_ws_file(&dir, "long.txt", body.as_bytes());
        let out = read_lines_range(&p, "long.txt", Some(1), None).unwrap();
        assert!(out.contains("[lines 1..1 of 2]"));
        assert!(out.contains("line 1 exceeds"));
        // 返回的是前缀（不含整行），且未静默丢弃
        let shown = out.split("]\n").nth(1).unwrap_or("");
        assert!(shown.len() >= MAX_READ_BYTES - 4 && shown.starts_with("xxxx"));
    }

    #[test]
    fn read_small_limit_reports_limit_not_binary() {
        let dir = TempDir::new().unwrap();
        // 首字符为 3 字节中文，limit=2 无法容纳
        let p = write_ws_file(&dir, "zh.txt", "你好".as_bytes());
        let err = read_file_capped(&p, 0, Some(2)).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("limit"), "should mention limit: {msg}");
        assert!(
            !msg.contains("不是有效 UTF-8"),
            "must not falsely claim binary: {msg}"
        );
    }

    #[test]
    fn move_and_copy_roundtrip() {
        let dir = TempDir::new().unwrap();
        let src = write_ws_file(&dir, "src.txt", b"hello");
        let moved = dir.path().join("sub/moved.txt");
        move_path(&src, &moved, dir.path()).unwrap();
        assert!(!src.exists());
        assert_eq!(fs::read_to_string(&moved).unwrap(), "hello");

        let copied = dir.path().join("copy.txt");
        copy_path(&moved, &copied, dir.path()).unwrap();
        assert!(moved.exists());
        assert_eq!(fs::read_to_string(&copied).unwrap(), "hello");

        // 目标已存在应报错
        let err = copy_path(&moved, &copied, dir.path()).unwrap_err();
        assert!(err.to_string().contains("已存在"));
    }

    #[test]
    fn list_recursive_walks_tree() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join("a/b")).unwrap();
        fs::write(dir.path().join("a/b/deep.rs"), "x").unwrap();
        fs::write(dir.path().join("top.txt"), "x").unwrap();
        let out = list_dir_capped(dir.path(), dir.path(), true, None).unwrap();
        assert!(out.contains("a/") || out.contains("a\\"));
        assert!(out.contains("deep.rs"));
        assert!(out.contains("top.txt"));
    }
}
