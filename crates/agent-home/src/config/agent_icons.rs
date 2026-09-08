//! Agent 表情符号与头像资源的存储与 `IDENTITY.md` 同步。
//!
//! 图标文件写入各 Agent 工作区 `assets/` 目录，`IDENTITY.md` 记录相对路径。
//! 创建 Agent 前可通过 `agents/pending-icons/` 暂存图标，创建后一次性应用。

use std::fs;
use std::path::{Path, PathBuf};

/// Agent 工作区内图标资源子目录名。
const ASSETS_DIR: &str = "assets";
/// Agent 图标类型：表情符号图片或头像图片。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentIconKind {
    /// 表情符号类图标，存为 `assets/emoji.{ext}`。
    Emoji,
    /// 头像类图标，存为 `assets/avatar.{ext}`。
    Avatar,
}

impl AgentIconKind {
    /// 返回小写文件名前缀（`"emoji"` / `"avatar"`）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Emoji => "emoji",
            Self::Avatar => "avatar",
        }
    }

    /// 返回 `IDENTITY.md` 中的字段标签（`"Emoji"` / `"Avatar"`）。
    pub fn identity_key(self) -> &'static str {
        match self {
            Self::Emoji => "Emoji",
            Self::Avatar => "Avatar",
        }
    }

    /// 从字符串解析图标类型；大小写不敏感，未知值返回 `None`。
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_lowercase().as_str() {
            "emoji" => Some(Self::Emoji),
            "avatar" => Some(Self::Avatar),
            _ => None,
        }
    }
}

/// 从文件名提取并白名单校验扩展名；不在白名单时回退 `"png"`。
fn sanitize_ext(file_name: &str) -> String {
    let ext = Path::new(file_name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("png")
        .to_lowercase();
    match ext.as_str() {
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "ico" => ext,
        _ => "png".to_string(),
    }
}

/// 返回数据根目录下待应用图标的缓存目录路径。
pub fn pending_icons_dir(base: &Path) -> PathBuf {
    crate::pending_agent_icons_dir(base)
}

/// 将 `IDENTITY.md` 中的图标字段解析为可读路径或 URL。
///
/// - 空值、`_(` 占位符、纯 emoji 字符 → `None`
/// - `http(s)://`、`data:image/` → 原样返回
/// - 相对/绝对本地路径 → 文件存在时返回绝对路径字符串，否则 `None`
pub fn resolve_icon_field(ws: &Path, value: Option<&str>) -> Option<String> {
    let raw = value?.trim();
    if raw.is_empty() || raw.starts_with("_(") {
        return None;
    }
    if raw.starts_with("http://") || raw.starts_with("https://") || raw.starts_with("data:image/") {
        return Some(raw.to_string());
    }
    let path = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        ws.join(raw)
    };
    if path.is_file() {
        Some(path.to_string_lossy().into_owned())
    } else {
        None
    }
}

/// 将图标字节写入工作区 `assets/{emoji|avatar}.{ext}`，并更新 `IDENTITY.md`。
///
/// 返回供 `IDENTITY.md` 使用的相对路径。空文件或超过 8MB 时报错；
/// 写入前会清理同 kind 的旧扩展名文件。
pub fn write_agent_icon(
    ws: &Path,
    kind: AgentIconKind,
    bytes: &[u8],
    file_name: &str,
) -> anyhow::Result<String> {
    if bytes.is_empty() {
        anyhow::bail!("图标文件为空");
    }
    if bytes.len() > 8 * 1024 * 1024 {
        anyhow::bail!("图标文件过大（上限 8MB）");
    }
    let ext = sanitize_ext(file_name);
    let assets = ws.join(ASSETS_DIR);
    fs::create_dir_all(&assets)?;

    // 清理同名旧扩展
    if let Ok(entries) = fs::read_dir(&assets) {
        let prefix = format!("{}.", kind.as_str());
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if name.starts_with(&prefix) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }

    let rel = format!("{ASSETS_DIR}/{}.{}", kind.as_str(), ext);
    let abs = ws.join(&rel);
    fs::write(&abs, bytes)?;
    upsert_identity_icon(ws, kind, &rel)?;
    Ok(rel)
}

/// 将图标暂存到 `agents/pending-icons/`，供新建 Agent 后批量应用。
///
/// 同 kind 的旧暂存文件会被替换。空文件或超过 8MB 时报错。
pub fn set_pending_agent_icon(
    base: &Path,
    kind: AgentIconKind,
    bytes: &[u8],
    file_name: &str,
) -> anyhow::Result<()> {
    if bytes.is_empty() {
        anyhow::bail!("图标文件为空");
    }
    if bytes.len() > 8 * 1024 * 1024 {
        anyhow::bail!("图标文件过大（上限 8MB）");
    }
    let dir = pending_icons_dir(base);
    fs::create_dir_all(&dir)?;
    let ext = sanitize_ext(file_name);
    let stem = kind.as_str();
    // 清掉同 kind 旧文件
    if let Ok(entries) = fs::read_dir(&dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with(&format!("{stem}.")) {
                let _ = fs::remove_file(entry.path());
            }
        }
    }
    fs::write(dir.join(format!("{stem}.{ext}")), bytes)?;
    Ok(())
}

/// 清除暂存图标；`kind` 为 `None` 时删除整个 pending 目录。
pub fn clear_pending_agent_icon(base: &Path, kind: Option<AgentIconKind>) -> anyhow::Result<()> {
    let dir = pending_icons_dir(base);
    if !dir.is_dir() {
        return Ok(());
    }
    if let Some(kind) = kind {
        let stem = kind.as_str();
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(&format!("{stem}.")) {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
    } else if dir.is_dir() {
        let _ = fs::remove_dir_all(&dir);
    }
    Ok(())
}

/// 将 pending 目录中的图标写入新建 Agent 工作区并更新 `IDENTITY.md`。
///
/// 应用成功后清空 pending 目录。无 pending 文件时返回 `Ok(false)`。
pub fn apply_pending_agent_icons(base: &Path, ws: &Path) -> anyhow::Result<bool> {
    let dir = pending_icons_dir(base);
    if !dir.is_dir() {
        return Ok(false);
    }
    let mut applied = false;
    for kind in [AgentIconKind::Emoji, AgentIconKind::Avatar] {
        let stem = kind.as_str();
        let found = fs::read_dir(&dir)?.flatten().find(|e| {
            e.file_name()
                .to_string_lossy()
                .starts_with(&format!("{stem}."))
        });
        let Some(entry) = found else {
            continue;
        };
        let bytes = fs::read(entry.path())?;
        let name = entry.file_name().to_string_lossy().to_string();
        write_agent_icon(ws, kind, &bytes, &name)?;
        applied = true;
    }
    let _ = clear_pending_agent_icon(base, None);
    Ok(applied)
}

/// 为已有 Agent 更新 emoji 和/或 avatar 图标。
///
/// 工作区不存在时报错；`emoji` / `avatar` 为 `None` 时跳过对应类型。
pub fn update_agent_icons(
    base: &Path,
    agent_id: &str,
    emoji: Option<(&[u8], &str)>,
    avatar: Option<(&[u8], &str)>,
) -> anyhow::Result<()> {
    let id = crate::normalize_agent_id(agent_id);
    let ws = crate::agent_workspace_dir(base, &id);
    if !ws.is_dir() {
        anyhow::bail!("Agent 工作区不存在: {}", ws.display());
    }
    if let Some((bytes, name)) = emoji {
        write_agent_icon(&ws, AgentIconKind::Emoji, bytes, name)?;
    }
    if let Some((bytes, name)) = avatar {
        write_agent_icon(&ws, AgentIconKind::Avatar, bytes, name)?;
    }
    Ok(())
}

/// 在 `IDENTITY.md` 中插入或更新指定 kind 的图标相对路径行。
///
/// 匹配 `- **Emoji:**` / `- emoji:` 等变体；新行优先插在 `Name` 字段之后。
fn upsert_identity_icon(ws: &Path, kind: AgentIconKind, rel_path: &str) -> anyhow::Result<()> {
    let path = ws.join("IDENTITY.md");
    let text = if path.is_file() {
        fs::read_to_string(&path)?
    } else {
        "# IDENTITY.md\n\n- **Name:**\n".to_string()
    };
    let key_lower = kind.as_str(); // emoji / avatar
    let label = kind.identity_key();
    let new_line = format!("- **{label}:** {rel_path}");
    let mut found = false;
    let mut out: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let is_field = trimmed.starts_with('-')
            && trimmed
                .trim_start_matches('-')
                .trim()
                .to_lowercase()
                .starts_with(&format!("{key_lower}:"));
        // also match **Emoji:**
        let is_field = is_field
            || (trimmed.starts_with('-')
                && trimmed
                    .to_lowercase()
                    .contains(&format!("**{key_lower}:**")));
        if is_field {
            if !found {
                out.push(new_line.clone());
                found = true;
            }
            continue;
        }
        out.push(line.to_string());
    }
    if !found {
        // 插到 Name 行后，否则文件末尾
        let mut inserted = false;
        let mut with_insert = Vec::new();
        for line in &out {
            with_insert.push(line.clone());
            let lower = line.to_lowercase();
            if !inserted
                && (lower.contains("**name:**") || lower.trim_start().starts_with("- name:"))
            {
                with_insert.push(new_line.clone());
                inserted = true;
            }
        }
        if !inserted {
            with_insert.push(new_line);
        }
        out = with_insert;
    }
    let mut body = out.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    fs::write(path, body)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn write_and_resolve_local_icon() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("workspace-demo");
        fs::create_dir_all(&ws).unwrap();
        fs::write(
            ws.join("IDENTITY.md"),
            "# IDENTITY.md\n\n- **Name:** Demo\n",
        )
        .unwrap();

        let rel = write_agent_icon(&ws, AgentIconKind::Avatar, b"fake-png", "a.PNG").unwrap();
        assert_eq!(rel, "assets/avatar.png");
        assert!(ws.join(&rel).is_file());

        let identity = fs::read_to_string(ws.join("IDENTITY.md")).unwrap();
        assert!(identity.contains("assets/avatar.png"));

        let resolved = resolve_icon_field(&ws, Some("assets/avatar.png")).unwrap();
        assert!(resolved.ends_with("assets/avatar.png"));
        assert!(resolve_icon_field(&ws, Some("🚀")).is_none());
    }

    #[test]
    fn pending_applies_then_clears() {
        let base = TempDir::new().unwrap();
        let ws = base.path().join("workspace-x");
        fs::create_dir_all(&ws).unwrap();
        fs::write(ws.join("IDENTITY.md"), "# ID\n- **Name:** X\n").unwrap();

        set_pending_agent_icon(base.path(), AgentIconKind::Emoji, b"e", "e.webp").unwrap();
        set_pending_agent_icon(base.path(), AgentIconKind::Avatar, b"a", "a.jpg").unwrap();
        assert!(apply_pending_agent_icons(base.path(), &ws).unwrap());
        assert!(ws.join("assets/emoji.webp").is_file());
        assert!(ws.join("assets/avatar.jpg").is_file());
        assert!(
            !pending_icons_dir(base.path()).exists() || {
                fs::read_dir(pending_icons_dir(base.path()))
                    .map(|d| d.count() == 0)
                    .unwrap_or(true)
            }
        );
    }
}
