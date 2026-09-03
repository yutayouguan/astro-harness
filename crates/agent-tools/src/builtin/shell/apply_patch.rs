//! apply_patch 工具：接收 diff-like 自由文本以新增、删除和更新文件。
//!
//! 协议格式对齐 apply_patch Lark grammar（见同目录 `apply_patch.lark`）。
//! 该工具为 *freeform* 类型——模型直接输出原始 patch 文本而非 JSON。

use std::path::{Path, PathBuf};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use types::tool_entry::FreeformToolFormat;

// ─── Lark grammar（嵌入二进制供 schema 注册）─────────────────────────────

const APPLY_PATCH_LARK_GRAMMAR: &str = include_str!("apply_patch.lark");

// ─── 解析数据结构 ───────────────────────────────────────────────────────

/// 一个 update file chunk 内的上下文/变更行块。
#[derive(Debug, Default, Clone, PartialEq)]
struct UpdateFileChunk {
    /// `@@ some_context` 行的内容（不含 `@@ ` 前缀）；`None` 表示无上下文标头。
    change_context: Option<String>,
    /// 待匹配/替换的旧行。
    old_lines: Vec<String>,
    /// 替换后的新行。
    new_lines: Vec<String>,
    /// `*** End of File` 标记——表明 `old_lines` 应匹配文件末尾。
    is_end_of_file: bool,
    additions: usize,
    deletions: usize,
}

impl UpdateFileChunk {
    /// 向 old_lines 和 new_lines 同时推入一条上下文行。
    fn push_context_line(&mut self, line: String) {
        self.old_lines.push(line.clone());
        self.new_lines.push(line);
    }
}

/// 解析后的单个文件操作。
#[derive(Debug, Clone, PartialEq)]
enum Hunk {
    Add {
        path: PathBuf,
        contents: String,
    },
    Delete {
        path: PathBuf,
    },
    Update {
        path: PathBuf,
        move_path: Option<PathBuf>,
        chunks: Vec<UpdateFileChunk>,
    },
}

/// 解析错误。
#[derive(Debug, Clone, PartialEq)]
enum ParseError {
    InvalidPatch(String),
    InvalidHunk { message: String, line_number: usize },
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::InvalidPatch(msg) => write!(f, "invalid patch: {msg}"),
            ParseError::InvalidHunk {
                message,
                line_number,
            } => write!(f, "invalid hunk at line {line_number}: {message}"),
        }
    }
}

impl std::error::Error for ParseError {}

// ─── 解析器常量 ─────────────────────────────────────────────────────────

const BEGIN_PATCH_MARKER: &str = "*** Begin Patch";
const END_PATCH_MARKER: &str = "*** End Patch";
const ADD_FILE_MARKER: &str = "*** Add File: ";
const DELETE_FILE_MARKER: &str = "*** Delete File: ";
const UPDATE_FILE_MARKER: &str = "*** Update File: ";
const MOVE_TO_MARKER: &str = "*** Move to: ";
const EOF_MARKER: &str = "*** End of File";
const CHANGE_CONTEXT_MARKER: &str = "@@ ";
const EMPTY_CHANGE_CONTEXT_MARKER: &str = "@@";
const MAX_REVERSIBLE_SNAPSHOT_BYTES: usize = 512 * 1024;

fn reversible_snapshots(
    before: Option<String>,
    after: Option<String>,
    otherwise_reversible: bool,
) -> (Option<String>, Option<String>, bool) {
    let size = before.as_ref().map_or(0, String::len) + after.as_ref().map_or(0, String::len);
    if otherwise_reversible && size <= MAX_REVERSIBLE_SNAPSHOT_BYTES {
        (before, after, true)
    } else {
        (None, None, false)
    }
}

// ─── 流式行解析器 ───────────────────────────────────────────────────────

#[derive(Debug, Default, Clone, Copy)]
enum ParserMode {
    #[default]
    NotStarted,
    StartedPatch,
    AddFile,
    DeleteFile,
    UpdateFile {
        hunk_line_number: usize,
    },
    EndedPatch,
}

#[derive(Debug, Default, Clone)]
struct PatchParser {
    mode: ParserMode,
    hunks: Vec<Hunk>,
    line_number: usize,
}

impl PatchParser {
    /// 解析完整 patch 文本，返回 hunk 列表。
    fn parse(patch: &str) -> Result<Vec<Hunk>, ParseError> {
        let mut parser = PatchParser::default();
        let lines: Vec<&str> = patch.trim().lines().collect();

        // 支持 heredoc 包裹（lenient mode）
        let lines = Self::strip_heredoc(&lines)?;

        for line in &lines {
            parser.line_number += 1;
            parser.process_line(line)?;
        }

        // 处理未以换行结尾的最后一行
        if !matches!(parser.mode, ParserMode::EndedPatch) {
            return Err(ParseError::InvalidPatch(
                "The last line of the patch must be '*** End Patch'".to_string(),
            ));
        }

        Ok(parser.hunks)
    }

    /// 剥离可选 heredoc 包裹（`<<EOF` / `<<'EOF'` / `<<"EOF"`）。
    fn strip_heredoc<'a>(lines: &'a [&'a str]) -> Result<Vec<&'a str>, ParseError> {
        if lines.is_empty() {
            return Err(ParseError::InvalidPatch(
                "The first line of the patch must be '*** Begin Patch'".to_string(),
            ));
        }

        let first = lines[0].trim();
        if first == BEGIN_PATCH_MARKER {
            return Ok(lines.to_vec());
        }

        // 尝试 heredoc
        let is_heredoc = first == "<<EOF" || first == "<<'EOF'" || first == "<<\"EOF\"";
        if is_heredoc && lines.len() >= 4 {
            let last = lines[lines.len() - 1].trim();
            if last.ends_with("EOF") {
                let inner = &lines[1..lines.len() - 1];
                // inner 的首行应为 Begin Patch
                if !inner.is_empty() && inner[0].trim() == BEGIN_PATCH_MARKER {
                    return Ok(inner.to_vec());
                }
            }
        }

        Err(ParseError::InvalidPatch(
            "The first line of the patch must be '*** Begin Patch'".to_string(),
        ))
    }

    fn ensure_update_hunk_not_empty(&self, _line: &str) -> Result<(), ParseError> {
        if let Some(Hunk::Update { path, chunks, .. }) = self.hunks.last() {
            if chunks.is_empty() {
                if let ParserMode::UpdateFile { hunk_line_number } = self.mode {
                    return Err(ParseError::InvalidHunk {
                        message: format!("Update file hunk for path '{}' is empty", path.display()),
                        line_number: hunk_line_number,
                    });
                }
            }
        }
        Ok(())
    }

    /// 尝试处理 hunk 头和 End Patch 标记；成功消费返回 true。
    fn handle_header_or_end(&mut self, trimmed: &str) -> Result<bool, ParseError> {
        if trimmed == END_PATCH_MARKER {
            self.ensure_update_hunk_not_empty(trimmed)?;
            self.mode = ParserMode::EndedPatch;
            return Ok(true);
        }
        if let Some(path) = trimmed.strip_prefix(ADD_FILE_MARKER) {
            self.ensure_update_hunk_not_empty(trimmed)?;
            self.hunks.push(Hunk::Add {
                path: PathBuf::from(path),
                contents: String::new(),
            });
            self.mode = ParserMode::AddFile;
            return Ok(true);
        }
        if let Some(path) = trimmed.strip_prefix(DELETE_FILE_MARKER) {
            self.ensure_update_hunk_not_empty(trimmed)?;
            self.hunks.push(Hunk::Delete {
                path: PathBuf::from(path),
            });
            self.mode = ParserMode::DeleteFile;
            return Ok(true);
        }
        if let Some(path) = trimmed.strip_prefix(UPDATE_FILE_MARKER) {
            self.ensure_update_hunk_not_empty(trimmed)?;
            self.hunks.push(Hunk::Update {
                path: PathBuf::from(path),
                move_path: None,
                chunks: Vec::new(),
            });
            self.mode = ParserMode::UpdateFile {
                hunk_line_number: self.line_number,
            };
            return Ok(true);
        }
        Ok(false)
    }

    fn process_line(&mut self, line: &str) -> Result<(), ParseError> {
        let trimmed = line.trim();
        match self.mode {
            ParserMode::NotStarted => {
                if trimmed == BEGIN_PATCH_MARKER {
                    self.mode = ParserMode::StartedPatch;
                    return Ok(());
                }
                Err(ParseError::InvalidPatch(
                    "The first line of the patch must be '*** Begin Patch'".to_string(),
                ))
            }
            ParserMode::StartedPatch | ParserMode::DeleteFile => {
                if self.handle_header_or_end(trimmed)? {
                    return Ok(());
                }
                Err(ParseError::InvalidHunk {
                    message: format!(
                        "'{trimmed}' is not a valid hunk header. Valid hunk headers: \
                         '*** Add File: {{path}}', '*** Delete File: {{path}}', \
                         '*** Update File: {{path}}'"
                    ),
                    line_number: self.line_number,
                })
            }
            ParserMode::AddFile => {
                if self.handle_header_or_end(trimmed)? {
                    return Ok(());
                }
                if let Some(content_line) = line.strip_prefix('+') {
                    if let Some(Hunk::Add { contents, .. }) = self.hunks.last_mut() {
                        contents.push_str(content_line);
                        contents.push('\n');
                        return Ok(());
                    }
                }
                Err(ParseError::InvalidHunk {
                    message: format!("'{trimmed}' is not a valid add line (must start with '+')"),
                    line_number: self.line_number,
                })
            }
            ParserMode::UpdateFile { hunk_line_number } => {
                let update_line = line.trim_end();
                if self.handle_header_or_end(update_line)? {
                    return Ok(());
                }

                if let Some(Hunk::Update {
                    move_path, chunks, ..
                }) = self.hunks.last_mut()
                {
                    // End of File 后忽略空行，但要求下一个 @@ 开始新 chunk
                    if chunks.last().is_some_and(|c| c.is_end_of_file) && update_line.is_empty() {
                        return Ok(());
                    }

                    // Move to（仅在第一个 chunk 前允许）
                    if chunks.is_empty() && move_path.is_none() {
                        if let Some(dest) = update_line.strip_prefix(MOVE_TO_MARKER) {
                            *move_path = Some(PathBuf::from(dest));
                            self.mode = ParserMode::UpdateFile { hunk_line_number };
                            return Ok(());
                        }
                    }

                    // @@ 上下文标记
                    if update_line == EMPTY_CHANGE_CONTEXT_MARKER {
                        chunks.push(UpdateFileChunk::default());
                        return Ok(());
                    }
                    if let Some(ctx) = update_line.strip_prefix(CHANGE_CONTEXT_MARKER) {
                        chunks.push(UpdateFileChunk {
                            change_context: Some(ctx.to_string()),
                            ..UpdateFileChunk::default()
                        });
                        return Ok(());
                    }

                    // *** End of File
                    if update_line == EOF_MARKER {
                        if let Some(chunk) = chunks.last_mut() {
                            chunk.is_end_of_file = true;
                        }
                        return Ok(());
                    }

                    // 空行作为上下文行
                    if line.is_empty() {
                        if chunks.is_empty() {
                            chunks.push(UpdateFileChunk::default());
                        }
                        if let Some(chunk) = chunks.last_mut() {
                            chunk.push_context_line(String::new());
                        }
                        return Ok(());
                    }

                    // 空格前缀的上下文行
                    if let Some(ctx_line) = line.strip_prefix(' ') {
                        if chunks.is_empty() {
                            chunks.push(UpdateFileChunk::default());
                        }
                        if let Some(chunk) = chunks.last_mut() {
                            chunk.push_context_line(ctx_line.to_string());
                        }
                        return Ok(());
                    }

                    // + 添加行
                    if let Some(add_line) = line.strip_prefix('+') {
                        if chunks.is_empty() {
                            chunks.push(UpdateFileChunk::default());
                        }
                        if let Some(chunk) = chunks.last_mut() {
                            chunk.new_lines.push(add_line.to_string());
                            chunk.additions += 1;
                        }
                        return Ok(());
                    }

                    // - 删除行
                    if let Some(del_line) = line.strip_prefix('-') {
                        if chunks.is_empty() {
                            chunks.push(UpdateFileChunk::default());
                        }
                        if let Some(chunk) = chunks.last_mut() {
                            chunk.old_lines.push(del_line.to_string());
                            chunk.deletions += 1;
                        }
                        return Ok(());
                    }
                }

                Err(ParseError::InvalidHunk {
                    message: format!(
                        "Unexpected line in update hunk: '{line}'. Lines should start with \
                         ' ' (context), '+' (add), or '-' (delete)"
                    ),
                    line_number: self.line_number,
                })
            }
            ParserMode::EndedPatch => {
                if trimmed.is_empty() {
                    Ok(())
                } else {
                    Err(ParseError::InvalidPatch(
                        "The last line of the patch must be '*** End Patch'".to_string(),
                    ))
                }
            }
        }
    }
}

// ─── 序列查找（fuzzy match with decreasing strictness）─────────────────

/// 在 `lines[start..]` 中查找与 `pattern` 匹配的连续子序列，
/// 依次尝试：精确匹配 → 去尾空白 → 全去空白 → Unicode 标点归一化。
/// `eof` 为 true 时优先从文件末尾开始搜索。
fn seek_sequence(lines: &[String], pattern: &[String], start: usize, eof: bool) -> Option<usize> {
    if pattern.is_empty() {
        return Some(start);
    }
    if pattern.len() > lines.len() {
        return None;
    }

    let search_start = if eof && lines.len() >= pattern.len() {
        lines.len() - pattern.len()
    } else {
        start
    };

    // 精确匹配
    for i in search_start..=lines.len().saturating_sub(pattern.len()) {
        if lines[i..i + pattern.len()] == *pattern {
            return Some(i);
        }
    }

    // 去尾空白
    for i in search_start..=lines.len().saturating_sub(pattern.len()) {
        let ok = pattern
            .iter()
            .enumerate()
            .all(|(j, pat)| lines[i + j].trim_end() == pat.trim_end());
        if ok {
            return Some(i);
        }
    }

    // 全去空白
    for i in search_start..=lines.len().saturating_sub(pattern.len()) {
        let ok = pattern
            .iter()
            .enumerate()
            .all(|(j, pat)| lines[i + j].trim() == pat.trim());
        if ok {
            return Some(i);
        }
    }

    // Unicode 标点归一化
    fn normalise(s: &str) -> String {
        s.trim()
            .chars()
            .map(|c| match c {
                '\u{2010}' | '\u{2011}' | '\u{2012}' | '\u{2013}' | '\u{2014}' | '\u{2015}'
                | '\u{2212}' => '-',
                '\u{2018}' | '\u{2019}' | '\u{201A}' | '\u{201B}' => '\'',
                '\u{201C}' | '\u{201D}' | '\u{201E}' | '\u{201F}' => '"',
                '\u{00A0}' | '\u{2002}' | '\u{2003}' | '\u{2004}' | '\u{2005}' | '\u{2006}'
                | '\u{2007}' | '\u{2008}' | '\u{2009}' | '\u{200A}' | '\u{202F}' | '\u{205F}'
                | '\u{3000}' => ' ',
                other => other,
            })
            .collect()
    }

    for i in search_start..=lines.len().saturating_sub(pattern.len()) {
        let ok = pattern
            .iter()
            .enumerate()
            .all(|(j, pat)| normalise(&lines[i + j]) == normalise(pat));
        if ok {
            return Some(i);
        }
    }

    None
}

// ─── 应用补丁到文件系统 ────────────────────────────────────────────────

/// 跟踪受影响的文件路径。
struct AffectedPaths {
    added: Vec<PathBuf>,
    modified: Vec<PathBuf>,
    deleted: Vec<PathBuf>,
}

/// 解析并应用 patch 到 `workspace` 下（或绝对路径）。
fn apply_patches(workspace: &Path, hunks: &[Hunk]) -> anyhow::Result<types::ToolOutput> {
    if hunks.is_empty() {
        anyhow::bail!("No files were modified.");
    }

    let mut affected = AffectedPaths {
        added: Vec::new(),
        modified: Vec::new(),
        deleted: Vec::new(),
    };
    let mut changes = Vec::with_capacity(hunks.len());

    for hunk in hunks {
        match hunk {
            Hunk::Add { path, contents } => {
                let full = resolve_path(workspace, path);
                let existed = full.exists();
                let before_content = std::fs::read_to_string(&full).ok();
                let has_before = before_content.is_some();
                let (before_content, after_content, reversible) = reversible_snapshots(
                    before_content,
                    Some(contents.clone()),
                    !existed || has_before,
                );
                if let Some(parent) = full.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&full, contents)?;
                affected.added.push(path.clone());
                changes.push(types::ToolFileChange {
                    path: path.to_string_lossy().into_owned(),
                    move_path: None,
                    kind: if existed {
                        types::ToolFileChangeKind::Update
                    } else {
                        types::ToolFileChangeKind::Add
                    },
                    before_content,
                    after_content,
                    additions: contents.lines().count(),
                    deletions: 0,
                    reversible,
                });
            }
            Hunk::Delete { path } => {
                let full = resolve_path(workspace, path);
                let before_content = std::fs::read_to_string(&full).ok();
                std::fs::remove_file(&full)
                    .map_err(|e| anyhow::anyhow!("Failed to delete {}: {e}", full.display()))?;
                affected.deleted.push(path.clone());
                let deletions = before_content
                    .as_deref()
                    .map_or(0, |text| text.lines().count());
                let has_before = before_content.is_some();
                let (before_content, after_content, reversible) =
                    reversible_snapshots(before_content, None, has_before);
                changes.push(types::ToolFileChange {
                    path: path.to_string_lossy().into_owned(),
                    move_path: None,
                    kind: types::ToolFileChangeKind::Delete,
                    before_content: before_content.clone(),
                    after_content,
                    additions: 0,
                    deletions,
                    reversible,
                });
            }
            Hunk::Update {
                path,
                move_path,
                chunks,
            } => {
                let full = resolve_path(workspace, path);
                let original = std::fs::read_to_string(&full)
                    .map_err(|e| anyhow::anyhow!("Failed to read {}: {e}", full.display()))?;

                let new_contents = derive_new_contents(&original, &full, chunks)?;
                let additions = chunks.iter().map(|chunk| chunk.additions).sum();
                let deletions = chunks.iter().map(|chunk| chunk.deletions).sum();
                let destination_before = move_path
                    .as_ref()
                    .and_then(|dest| std::fs::read_to_string(resolve_path(workspace, dest)).ok());

                if let Some(dest) = move_path {
                    let dest_full = resolve_path(workspace, dest);
                    if let Some(parent) = dest_full.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&dest_full, &new_contents)?;
                    std::fs::remove_file(&full).map_err(|e| {
                        anyhow::anyhow!("Failed to remove original {}: {e}", full.display())
                    })?;
                } else {
                    std::fs::write(&full, &new_contents)?;
                }
                affected.modified.push(path.clone());
                let (before_content, after_content, reversible) = reversible_snapshots(
                    Some(original),
                    Some(new_contents),
                    move_path.is_none() || destination_before.is_none(),
                );
                changes.push(types::ToolFileChange {
                    path: path.to_string_lossy().into_owned(),
                    move_path: move_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned()),
                    kind: if move_path.is_some() {
                        types::ToolFileChangeKind::Move
                    } else {
                        types::ToolFileChangeKind::Update
                    },
                    before_content,
                    after_content,
                    additions,
                    deletions,
                    reversible,
                });
            }
        }
    }

    let snapshot_bytes = changes
        .iter()
        .map(|change| {
            change.before_content.as_ref().map_or(0, String::len)
                + change.after_content.as_ref().map_or(0, String::len)
        })
        .sum::<usize>();
    if snapshot_bytes > MAX_REVERSIBLE_SNAPSHOT_BYTES {
        for change in &mut changes {
            change.before_content = None;
            change.after_content = None;
            change.reversible = false;
        }
    }

    Ok(types::ToolOutput::FileChanges {
        text: format_summary(&affected),
        changes,
    })
}

/// 相对路径基于 workspace 解析，绝对路径直接使用。
fn resolve_path(workspace: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        workspace.join(path)
    }
}

/// 根据 chunks 计算更新后的文件内容。
fn derive_new_contents(
    original: &str,
    path: &Path,
    chunks: &[UpdateFileChunk],
) -> anyhow::Result<String> {
    let mut original_lines: Vec<String> = original.split('\n').map(String::from).collect();
    // 去掉末尾空元素（trailing newline 产物）
    if original_lines.last().is_some_and(String::is_empty) {
        original_lines.pop();
    }

    let replacements = compute_replacements(&original_lines, path, chunks)?;
    let mut new_lines = apply_replacements(original_lines, &replacements);

    // 确保以换行结尾
    if !new_lines.last().is_some_and(String::is_empty) {
        new_lines.push(String::new());
    }
    Ok(new_lines.join("\n"))
}

/// (start_index, old_len, new_lines)
type Replacement = (usize, usize, Vec<String>);

/// 计算各 chunk 对应的 replacement。
fn compute_replacements(
    original_lines: &[String],
    path: &Path,
    chunks: &[UpdateFileChunk],
) -> anyhow::Result<Vec<Replacement>> {
    let mut replacements: Vec<Replacement> = Vec::new();
    let mut line_index: usize = 0;
    let path_str = path.display().to_string();

    for chunk in chunks {
        // 按上下文标记定位
        if let Some(ctx_line) = &chunk.change_context {
            if let Some(idx) = seek_sequence(
                original_lines,
                std::slice::from_ref(ctx_line),
                line_index,
                false,
            ) {
                line_index = idx + 1;
            } else {
                anyhow::bail!("Failed to find context '{ctx_line}' in {path_str}");
            }
        }

        if chunk.old_lines.is_empty() {
            // 纯插入——定位到文件末尾（或尾空行之前）
            let insertion_idx = if original_lines.last().is_some_and(String::is_empty) {
                original_lines.len() - 1
            } else {
                original_lines.len()
            };
            replacements.push((insertion_idx, 0, chunk.new_lines.clone()));
            continue;
        }

        // 查找 old_lines 在文件中的位置
        let mut pattern: &[String] = &chunk.old_lines;
        let mut found = seek_sequence(original_lines, pattern, line_index, chunk.is_end_of_file);

        let mut new_slice: &[String] = &chunk.new_lines;

        // 重试：去掉尾部空行（表示 EOF 的 trailing newline）
        if found.is_none() && pattern.last().is_some_and(String::is_empty) {
            pattern = &pattern[..pattern.len() - 1];
            if new_slice.last().is_some_and(String::is_empty) {
                new_slice = &new_slice[..new_slice.len() - 1];
            }
            found = seek_sequence(original_lines, pattern, line_index, chunk.is_end_of_file);
        }

        if let Some(start_idx) = found {
            replacements.push((start_idx, pattern.len(), new_slice.to_vec()));
            line_index = start_idx + pattern.len();
        } else {
            anyhow::bail!(
                "Failed to find expected lines in {}:\n{}",
                path_str,
                chunk.old_lines.join("\n"),
            );
        }
    }

    replacements.sort_by_key(|(idx, _, _)| *idx);
    Ok(replacements)
}

/// 倒序应用替换——避免前面的替换偏移后续索引。
fn apply_replacements(mut lines: Vec<String>, replacements: &[Replacement]) -> Vec<String> {
    for (start_idx, old_len, new_segment) in replacements.iter().rev() {
        let start_idx = *start_idx;
        let old_len = *old_len;

        for _ in 0..old_len {
            if start_idx < lines.len() {
                lines.remove(start_idx);
            }
        }

        for (offset, new_line) in new_segment.iter().enumerate() {
            lines.insert(start_idx + offset, new_line.clone());
        }
    }
    lines
}

/// 格式化 git 风格的变更摘要。
fn format_summary(affected: &AffectedPaths) -> String {
    let mut out = String::from("Success. Updated the following files:\n");
    for path in &affected.added {
        out.push_str(&format!("A {}\n", path.display()));
    }
    for path in &affected.modified {
        out.push_str(&format!("M {}\n", path.display()));
    }
    for path in &affected.deleted {
        out.push_str(&format!("D {}\n", path.display()));
    }
    out
}

// ─── 工具注册 ──────────────────────────────────────────────────────────

/// 向注册表注册 `apply_patch` freeform 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "apply_patch".to_string(),
        toolset: "apply_patch".to_string(),
        description: "Apply a freeform patch to add, delete, or update files. \
            This is a FREEFORM tool — the model outputs raw patch text following \
            the Lark grammar, not JSON."
            .to_string(),
        schema: serde_json::json!({ "type": "object", "properties": {} }),
        check_fn: None,
        icon: "file-diff",
        needs_confirmation: true,
        freeform_format: Some(FreeformToolFormat {
            r#type: "grammar".to_string(),
            syntax: "lark".to_string(),
            definition: APPLY_PATCH_LARK_GRAMMAR.to_string(),
        }),
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["apply_patch"],
    async_ctx: dispatch,
}

/// 分发入口：从 args 提取原始 patch 文本，解析并应用。
pub async fn dispatch(
    ctx: &ToolContext<'_>,
    args: &serde_json::Value,
) -> anyhow::Result<types::ToolOutput> {
    // freeform 工具的输入可能以 JSON string 或 object.input 传入
    let patch_text = extract_patch_text(args)?;

    let hunks = PatchParser::parse(&patch_text).map_err(|e| anyhow::anyhow!("{e}"))?;

    // 以 project_root（如有）或 workspace_dir 为工作目录
    let workspace = ctx.project_root.as_deref().unwrap_or(&ctx.workspace_dir);

    apply_patches(workspace, &hunks)
}

/// 从 `serde_json::Value` 中提取原始 patch 文本。
///
/// 支持三种形态：
/// 1. 直接 string：`"*** Begin Patch\n..."`
/// 2. object with `input` key：`{ "input": "*** Begin Patch\n..." }`
/// 3. object with `patch` key：`{ "patch": "*** Begin Patch\n..." }`
fn extract_patch_text(args: &serde_json::Value) -> anyhow::Result<String> {
    if let Some(s) = args.as_str() {
        return Ok(s.to_string());
    }
    if let Some(obj) = args.as_object() {
        for key in &["input", "patch"] {
            if let Some(s) = obj.get(*key).and_then(|v| v.as_str()) {
                return Ok(s.to_string());
            }
        }
    }
    anyhow::bail!(
        "apply_patch expects freeform patch text (string), got: {}",
        serde_json::to_string_pretty(args).unwrap_or_default()
    )
}

// ─── 测试 ──────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn wrap_patch(body: &str) -> String {
        format!("*** Begin Patch\n{body}\n*** End Patch")
    }

    // ── 解析测试 ──

    #[test]
    fn parse_add_file() {
        let patch = wrap_patch("*** Add File: foo.txt\n+hello\n+world");
        let hunks = PatchParser::parse(&patch).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0],
            Hunk::Add {
                path: PathBuf::from("foo.txt"),
                contents: "hello\nworld\n".to_string()
            }
        );
    }

    #[test]
    fn parse_delete_file() {
        let patch = wrap_patch("*** Delete File: gone.txt");
        let hunks = PatchParser::parse(&patch).unwrap();
        assert_eq!(hunks.len(), 1);
        assert_eq!(
            hunks[0],
            Hunk::Delete {
                path: PathBuf::from("gone.txt"),
            }
        );
    }

    #[test]
    fn parse_update_file() {
        let patch = wrap_patch(
            "*** Update File: main.rs\n\
             @@ fn main()\n\
             -    old_line\n\
             +    new_line",
        );
        let hunks = PatchParser::parse(&patch).unwrap();
        assert_eq!(hunks.len(), 1);
        match &hunks[0] {
            Hunk::Update { path, chunks, .. } => {
                assert_eq!(path, &PathBuf::from("main.rs"));
                assert_eq!(chunks.len(), 1);
                assert_eq!(chunks[0].change_context, Some("fn main()".to_string()));
                assert_eq!(chunks[0].old_lines, vec!["    old_line"]);
                assert_eq!(chunks[0].new_lines, vec!["    new_line"]);
            }
            _ => panic!("expected UpdateFile hunk"),
        }
    }

    #[test]
    fn parse_update_with_move() {
        let patch = wrap_patch(
            "*** Update File: old.rs\n\
             *** Move to: new.rs\n\
             @@\n\
             -old\n\
             +new",
        );
        let hunks = PatchParser::parse(&patch).unwrap();
        match &hunks[0] {
            Hunk::Update {
                path, move_path, ..
            } => {
                assert_eq!(path, &PathBuf::from("old.rs"));
                assert_eq!(move_path.as_deref(), Some(Path::new("new.rs")));
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    #[test]
    fn parse_mixed_hunks() {
        let patch = wrap_patch(
            "*** Add File: new.py\n\
             +content\n\
             *** Delete File: old.py\n\
             *** Update File: edit.py\n\
             @@\n\
             -x\n\
             +y",
        );
        let hunks = PatchParser::parse(&patch).unwrap();
        assert_eq!(hunks.len(), 3);
    }

    #[test]
    fn parse_context_lines() {
        let patch = wrap_patch(
            "*** Update File: f.txt\n\
             @@\n \
             context\n\
             -old\n\
             +new",
        );
        let hunks = PatchParser::parse(&patch).unwrap();
        match &hunks[0] {
            Hunk::Update { chunks, .. } => {
                assert_eq!(chunks[0].old_lines, vec!["context", "old"]);
                assert_eq!(chunks[0].new_lines, vec!["context", "new"]);
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    #[test]
    fn parse_end_of_file_marker() {
        let patch = wrap_patch(
            "*** Update File: f.txt\n\
             @@\n\
             +appended\n\
             *** End of File",
        );
        let hunks = PatchParser::parse(&patch).unwrap();
        match &hunks[0] {
            Hunk::Update { chunks, .. } => {
                assert!(chunks[0].is_end_of_file);
            }
            _ => panic!("expected UpdateFile"),
        }
    }

    #[test]
    fn parse_heredoc_wrapped() {
        let patch = "<<'EOF'\n*** Begin Patch\n*** Add File: a.txt\n+hi\n*** End Patch\nEOF";
        let hunks = PatchParser::parse(patch).unwrap();
        assert_eq!(hunks.len(), 1);
    }

    #[test]
    fn parse_rejects_bad_start() {
        let result = PatchParser::parse("garbage");
        assert!(result.is_err());
    }

    #[test]
    fn parse_rejects_missing_end() {
        let result = PatchParser::parse("*** Begin Patch\n*** Add File: a.txt\n+hi");
        assert!(result.is_err());
    }

    #[test]
    fn parse_empty_update_is_error() {
        let patch = "*** Begin Patch\n*** Update File: f.txt\n*** End Patch";
        let result = PatchParser::parse(patch);
        assert!(result.is_err());
    }

    // ── 序列查找测试 ──

    #[test]
    fn seek_exact_match() {
        let lines: Vec<String> = vec!["foo", "bar", "baz"]
            .into_iter()
            .map(String::from)
            .collect();
        let pattern: Vec<String> = vec!["bar", "baz"].into_iter().map(String::from).collect();
        assert_eq!(seek_sequence(&lines, &pattern, 0, false), Some(1));
    }

    #[test]
    fn seek_trim_match() {
        let lines: Vec<String> = vec!["  foo  ", "  bar\t"]
            .into_iter()
            .map(String::from)
            .collect();
        let pattern: Vec<String> = vec!["foo", "bar"].into_iter().map(String::from).collect();
        assert_eq!(seek_sequence(&lines, &pattern, 0, false), Some(0));
    }

    #[test]
    fn seek_pattern_longer_than_input() {
        let lines: Vec<String> = vec!["one"].into_iter().map(String::from).collect();
        let pattern: Vec<String> = vec!["a", "b", "c"].into_iter().map(String::from).collect();
        assert_eq!(seek_sequence(&lines, &pattern, 0, false), None);
    }

    // ── 应用测试 ──

    #[test]
    fn apply_add_file() {
        let dir = tempdir().unwrap();
        let patch = wrap_patch(&format!(
            "*** Add File: {}\n+hello\n+world",
            dir.path().join("new.txt").display()
        ));
        let hunks = PatchParser::parse(&patch).unwrap();
        let result = apply_patches(dir.path(), &hunks).unwrap();
        assert!(result.text().contains("A "));
        assert_eq!(result.file_changes().len(), 1);
        assert_eq!(
            result.file_changes()[0].kind,
            types::ToolFileChangeKind::Add
        );
        assert_eq!(result.file_changes()[0].before_content, None);
        assert_eq!(
            result.file_changes()[0].after_content.as_deref(),
            Some("hello\nworld\n")
        );
        assert!(result.file_changes()[0].reversible);
        let content = std::fs::read_to_string(dir.path().join("new.txt")).unwrap();
        assert_eq!(content, "hello\nworld\n");
    }

    #[test]
    fn apply_delete_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("del.txt");
        std::fs::write(&path, "x").unwrap();
        let patch = wrap_patch(&format!("*** Delete File: {}", path.display()));
        let hunks = PatchParser::parse(&patch).unwrap();
        let result = apply_patches(dir.path(), &hunks).unwrap();
        assert!(result.text().contains("D "));
        assert!(!path.exists());
    }

    #[test]
    fn apply_update_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("update.txt");
        std::fs::write(&path, "foo\nbar\n").unwrap();
        let patch = wrap_patch(&format!(
            "*** Update File: {}\n@@\n foo\n-bar\n+baz",
            path.display()
        ));
        let hunks = PatchParser::parse(&patch).unwrap();
        let result = apply_patches(dir.path(), &hunks).unwrap();
        assert!(result.text().contains("M "));
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "foo\nbaz\n");
    }

    #[test]
    fn apply_update_with_move() {
        let dir = tempdir().unwrap();
        let src = dir.path().join("src.txt");
        let dst = dir.path().join("dst.txt");
        std::fs::write(&src, "line\n").unwrap();
        let patch = wrap_patch(&format!(
            "*** Update File: {}\n*** Move to: {}\n@@\n-line\n+line2",
            src.display(),
            dst.display()
        ));
        let hunks = PatchParser::parse(&patch).unwrap();
        apply_patches(dir.path(), &hunks).unwrap();
        assert!(!src.exists());
        let content = std::fs::read_to_string(&dst).unwrap();
        assert_eq!(content, "line2\n");
    }

    #[test]
    fn apply_multiple_chunks() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("multi.txt");
        std::fs::write(&path, "foo\nbar\nbaz\nqux\n").unwrap();
        let patch = wrap_patch(&format!(
            "*** Update File: {}\n@@\n foo\n-bar\n+BAR\n@@\n baz\n-qux\n+QUX",
            path.display()
        ));
        let hunks = PatchParser::parse(&patch).unwrap();
        apply_patches(dir.path(), &hunks).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "foo\nBAR\nbaz\nQUX\n");
    }

    #[test]
    fn apply_relative_path() {
        let dir = tempdir().unwrap();
        let patch = wrap_patch("*** Add File: sub/dir/file.txt\n+hello");
        let hunks = PatchParser::parse(&patch).unwrap();
        apply_patches(dir.path(), &hunks).unwrap();
        let content = std::fs::read_to_string(dir.path().join("sub/dir/file.txt")).unwrap();
        assert_eq!(content, "hello\n");
    }

    #[test]
    fn extract_patch_text_from_string() {
        let args = serde_json::json!("raw patch text");
        assert_eq!(extract_patch_text(&args).unwrap(), "raw patch text");
    }

    #[test]
    fn extract_patch_text_from_input_key() {
        let args = serde_json::json!({ "input": "patch body" });
        assert_eq!(extract_patch_text(&args).unwrap(), "patch body");
    }

    #[test]
    fn extract_patch_text_from_patch_key() {
        let args = serde_json::json!({ "patch": "patch body" });
        assert_eq!(extract_patch_text(&args).unwrap(), "patch body");
    }
}
