//! Agent 工作区路径解析与规范化（跨平台：macOS / Windows / Linux）。

use std::fs;
use std::path::{Path, PathBuf};

/// 默认 Agent 的 id（存储在 DB 和协议层）。
pub const DEFAULT_AGENT_ID: &str = "default";

/// 默认 Agent 的**工作区内容目录名**（`~/.astro/workspace/`），
/// 与 Agent id `"default"` 无关，仅用于文件系统路径拼接。
pub const DEFAULT_AGENT_WORKSPACE_DIR: &str = "workspace";

/// 新建 Agent id 的可读 slug 与随机后缀分隔符：`{slug}--{hex}`
pub const AGENT_ID_SUFFIX_SEP: &str = "--";

/// slug 为空（如纯中文名）时的兜底可读段：`agent--{hex}`
pub const AGENT_ID_FALLBACK_SLUG: &str = "agent";

/// 随机后缀的十六进制位数（48 bit，本地少量 Agent 足够且目录名不至过长）
const AGENT_ID_HEX_LEN: usize = 12;

/// slug 段最大长度（避免目录名过长）
const AGENT_ID_SLUG_MAX: usize = 24;

/// 用户主目录（跨平台统一入口）。
///
/// - Windows：优先 `USERPROFILE`，再 `HOME`（Git Bash 等）
/// - macOS / Linux：优先 `HOME`，再 `USERPROFILE`
pub fn user_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
    }
}

/// 用户「下载」目录（跨平台行为一致）。
///
/// 优先级：
/// 1. `XDG_DOWNLOAD_DIR`（Linux / 部分桌面）
/// 2. `{home}/Downloads` 或 `{home}/下载`（已存在则用之）
/// 3. 创建 `{home}/Downloads`
pub fn user_downloads_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DOWNLOAD_DIR") {
        let p = PathBuf::from(xdg.trim());
        if !p.as_os_str().is_empty() {
            let _ = fs::create_dir_all(&p);
            return p;
        }
    }
    let home = user_home_dir().unwrap_or_else(|| PathBuf::from("."));
    for name in ["Downloads", "下载"] {
        let p = home.join(name);
        if p.is_dir() {
            return p;
        }
    }
    let p = home.join("Downloads");
    let _ = fs::create_dir_all(&p);
    p
}

/// 将路径格式化为跨平台一致的展示文案（主目录用 `~`，分隔符统一 `/`）。
///
/// 仅用于 Toast / UI 文案；真实 IO 仍使用绝对路径。
pub fn display_user_path(path: &Path) -> String {
    let fwd = |p: &Path| p.to_string_lossy().replace('\\', "/");
    let Some(home) = user_home_dir() else {
        return fwd(path);
    };
    let path_forms = [path.canonicalize().ok(), Some(path.to_path_buf())];
    let home_forms = [home.canonicalize().ok(), Some(home)];
    for p in path_forms.iter().flatten() {
        for h in home_forms.iter().flatten() {
            if let Ok(rest) = p.strip_prefix(h) {
                let rest = fwd(rest);
                return if rest.is_empty() {
                    "~".to_string()
                } else {
                    format!("~/{rest}")
                };
            }
        }
    }
    fwd(path)
}

/// 默认记忆/工作空间根目录：`$ASTRO_MEMORY_DIR` 或 `~/.astro`
pub fn default_memory_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ASTRO_MEMORY_DIR") {
        return PathBuf::from(dir);
    }
    user_home_dir()
        .map(|home| home.join(".astro"))
        .unwrap_or_else(|| PathBuf::from(".astro"))
}

/// 数据库目录：`{base}/data/`（state.db、usage.db、subagents 等）
pub fn data_dir(base: &Path) -> PathBuf {
    base.join("data")
}

/// 会话 rollout 目录：`{base}/sessions/rollouts/`
pub fn rollouts_dir(base: &Path) -> PathBuf {
    base.join("sessions").join("rollouts")
}

/// 记忆子系统目录：`{base}/memory/`（dreaming、audit、pending、learning）
pub fn memory_subsystem_dir(base: &Path) -> PathBuf {
    base.join("memory")
}

/// 解析 Agent 工作区路径。
///
/// - `default`（默认）→ `{base}/workspace`（目录名保持 `workspace` 不变）
/// - 其他 id → `{base}/workspace-{id}`
pub fn agent_workspace_dir(base: &Path, agent_id: &str) -> PathBuf {
    let _ = agent_id;
    base.join(DEFAULT_AGENT_WORKSPACE_DIR)
}

/// 默认 Agent 的配置目录名（`agents/default/`），与工作区目录名 `workspace/` 区分。
pub const DEFAULT_AGENT_CONFIG_DIR: &str = "default";

/// Agent 运行时配置目录：`{base}/agents/{id}/`（模型、工具等，不含 MCP 与工作区文件）。
///
/// 默认 Agent（id = `workspace`）的配置目录固定为 `agents/default/`，
/// 避免与工作区内容目录 `workspace/` 产生歧义。
pub fn agent_config_dir(base: &Path, agent_id: &str) -> PathBuf {
    let _ = agent_id;
    base.join("agents").join(DEFAULT_AGENT_CONFIG_DIR)
}

/// 从工作区目录名解析 agent id（`workspace` → `"default"`；`workspace-xxx` → `"xxx"`）
pub fn agent_id_from_workspace_dir_name(name: &str) -> Option<String> {
    if name == DEFAULT_AGENT_WORKSPACE_DIR {
        return Some(DEFAULT_AGENT_ID.to_string());
    }
    name.strip_prefix("workspace-")
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// 返回当前激活 Agent 的工作区（缺省 `workspace`）
pub fn default_agent_workspace_dir() -> PathBuf {
    let base = default_memory_dir();
    let id = active_agent_id(&base);
    agent_workspace_dir(&base, &id)
}

/// 由显示名派生可读 slug（ASCII 小写、`-` 连接、压缩连续分隔、限长）。
///
/// 纯非 ASCII（如中文）返回空串，由调用方用兜底段。仅用于**创建时**的可读快照，
/// 不参与身份判等——id 一旦生成即不可变，改名不影响。
pub fn slug_from_name(name: &str) -> String {
    let mut slug = String::new();
    let mut prev_dash = false;
    for c in name.trim().to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c);
            prev_dash = false;
        } else if (c == '-' || c == '_' || c.is_whitespace()) && !slug.is_empty() && !prev_dash {
            slug.push('-');
            prev_dash = true;
        }
    }
    let trimmed = slug.trim_matches('-');
    trimmed
        .chars()
        .take(AGENT_ID_SLUG_MAX)
        .collect::<String>()
        .trim_end_matches('-')
        .to_string()
}

/// 生成新 Agent 的不可变 id：`{slug}--{hex12}`（slug 来自显示名，可读；hex 保唯一）。
///
/// 纯非 ASCII 名回退为 `agent--{hex12}`。显示名另存 `AgentInfo.name` / config，
/// 与 id 解耦；改名不改 id、不改目录。
pub fn generate_agent_id(name: &str) -> String {
    let hex: String = uuid::Uuid::new_v4()
        .simple()
        .to_string()
        .chars()
        .take(AGENT_ID_HEX_LEN)
        .collect();
    let slug = slug_from_name(name);
    let head = if slug.is_empty() {
        AGENT_ID_FALLBACK_SLUG
    } else {
        &slug
    };
    format!("{head}{AGENT_ID_SUFFIX_SEP}{hex}")
}

/// 是否为新式生成 id（以 `--{12hex}` 结尾且前缀非空）。
pub fn is_generated_agent_id(id: &str) -> bool {
    let Some((head, hex)) = id.rsplit_once(AGENT_ID_SUFFIX_SEP) else {
        return false;
    };
    !head.is_empty() && hex.len() == AGENT_ID_HEX_LEN && hex.chars().all(|c| c.is_ascii_hexdigit())
}

/// 规范化已有 agent id（查找路径 / 激活 / 兼容旧数据）。
///
/// - 空、`workspace`（旧默认别名）→ `default`
/// - 新式 `{slug}--{hex}` 与旧 slug（如 `ppt-expert`）均小写清洗后透传
/// - 纯非 ASCII 遗留输入仍用 `agent-{hash}` 兜底（**新建**请用 [`generate_agent_id`]）
pub fn normalize_agent_id(raw: &str) -> String {
    let s = raw.trim().to_lowercase().replace(' ', "-");
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let cleaned = cleaned.trim_matches('-').trim_matches('_').to_string();
    if cleaned == DEFAULT_AGENT_ID || cleaned == DEFAULT_AGENT_WORKSPACE_DIR {
        return DEFAULT_AGENT_ID.to_string();
    }
    if cleaned.is_empty() {
        if raw.trim().is_empty() {
            return DEFAULT_AGENT_ID.to_string();
        }
        let mut hash: u32 = 2166136261;
        for b in raw.trim().bytes() {
            hash ^= u32::from(b);
            hash = hash.wrapping_mul(16777619);
        }
        return format!("agent-{:x}", hash);
    }
    cleaned
}

/// 读取当前激活的 Agent id（缺省为 `workspace`）
pub fn active_agent_id(base: &Path) -> String {
    let _ = base;
    DEFAULT_AGENT_ID.to_string()
}

/// 设置当前激活的 Agent（目标工作区必须已存在）
pub fn set_active_agent(base: &Path, agent_id: &str) -> anyhow::Result<String> {
    let id = normalize_agent_id(agent_id);
    if id != DEFAULT_AGENT_ID {
        anyhow::bail!("Astro 已切换为单专家模式，仅支持 default");
    }
    let _ = base;
    Ok(id)
}

/// 每日流水日记目录（相对工作区根），与 OpenClaw `memory/*.md` 对齐。
pub const DAILY_MEMORY_DIR: &str = "memory";

/// 某 Agent 工作区内的日记忆路径：`memory/YYYY-MM-DD.md`
pub fn daily_memory_path(workspace: &Path, date: &str) -> PathBuf {
    workspace.join(DAILY_MEMORY_DIR).join(format!("{date}.md"))
}

/// 今日日期（本地）`YYYY-MM-DD`
pub fn today_date_string() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 列出工作区内已有的日记忆文件名（不含扩展名），新→旧
pub fn list_daily_memory_dates(workspace: &Path) -> Vec<String> {
    let dir = workspace.join(DAILY_MEMORY_DIR);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut dates: Vec<String> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|x| x.to_str()) != Some("md") {
                return None;
            }
            path.file_stem().map(|s| s.to_string_lossy().into_owned())
        })
        .filter(|s| s.len() == 10 && s.chars().nth(4) == Some('-') && s.chars().nth(7) == Some('-'))
        .collect();
    dates.sort();
    dates.reverse();
    dates
}

/// 确保基础目录结构存在（仅创建目录，不初始化 SQLite 数据库）。
///
/// 适用于轻量工具（MCP 配置读写、日志初始化等），不需要完整工作区初始化。
/// 包含自动迁移：根级散落的 DB → `data/`，记忆相关文件 → `memory/`。
pub fn ensure_workspace_dirs(base: &Path) -> anyhow::Result<()> {
    fs::create_dir_all(base)?;
    fs::create_dir_all(base.join("agents"))?;
    // 迁移：旧版本默认 Agent 配置目录为 agents/workspace，
    // 新版本改为 agents/default 以避免与工作区内容目录 workspace/ 歧义。
    let old_config = base.join("agents").join(DEFAULT_AGENT_ID);
    let new_config = base.join("agents").join(DEFAULT_AGENT_CONFIG_DIR);
    if old_config.is_dir() && !new_config.exists() {
        let _ = fs::rename(&old_config, &new_config);
    }

    // 迁移：根级 DB → data/
    let data_dir = base.join("data");
    migrate_file(base, "usage.db", &data_dir);
    migrate_file(base, "subagents-v2.db", &data_dir);
    migrate_file(&base.join("sessions"), "state.db", &data_dir);
    migrate_file(&base.join("sessions"), "artifacts.db", &data_dir);
    migrate_file(&base.join("sessions"), "knowledge.db", &data_dir);
    migrate_file(&base.join("cron"), "cron.db", &data_dir);

    // 迁移：记忆相关 → memory/
    let memory_dir = base.join("memory");
    migrate_file(base, "dreaming.json", &memory_dir);
    migrate_dir(base, "audit", &memory_dir);
    migrate_dir(base, "learning", &memory_dir);
    migrate_dir(base, "pending", &memory_dir);

    Ok(())
}

fn migrate_file(old_parent: &Path, name: &str, new_parent: &Path) {
    let old = old_parent.join(name);
    let new = new_parent.join(name);
    if old.is_file() && !new.exists() {
        let _ = fs::create_dir_all(new_parent);
        let _ = fs::rename(&old, &new);
    }
}

fn migrate_dir(old_parent: &Path, name: &str, new_parent: &Path) {
    let old = old_parent.join(name);
    let new = new_parent.join(name);
    if old.is_dir() && !new.exists() {
        let _ = fs::create_dir_all(new_parent);
        let _ = fs::rename(&old, &new);
    }
}

/// 确保默认工作区基础目录存在（不初始化 SQLite）。
pub fn ensure_default_workspace_dirs() -> anyhow::Result<()> {
    ensure_workspace_dirs(&default_memory_dir())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_user_path_uses_tilde_and_forward_slash() {
        let home = user_home_dir().expect("home");
        let sample = home.join("Downloads").join("a b.png");
        let shown = display_user_path(&sample);
        assert!(
            shown.starts_with("~/"),
            "expected tilde prefix, got {shown}"
        );
        assert!(
            !shown.contains('\\'),
            "display path must use forward slashes: {shown}"
        );
        assert!(shown.ends_with("Downloads/a b.png") || shown.contains("Downloads/a b.png"));
    }

    #[test]
    fn user_downloads_dir_is_under_home() {
        let home = user_home_dir().expect("home");
        let dl = user_downloads_dir();
        let home_abs = home.canonicalize().unwrap_or(home);
        let dl_abs = dl.canonicalize().unwrap_or(dl.clone());
        assert!(
            dl_abs.starts_with(&home_abs) || std::env::var("XDG_DOWNLOAD_DIR").is_ok(),
            "downloads {:?} should be under home {:?}",
            dl_abs,
            home_abs
        );
    }

    #[test]
    fn slug_from_name_ascii_and_cjk() {
        assert_eq!(slug_from_name("PPT Expert"), "ppt-expert");
        assert_eq!(slug_from_name("  Code_Reviewer "), "code-reviewer");
        assert_eq!(slug_from_name("我的助手"), "");
        assert_eq!(slug_from_name("助手 Pro"), "pro");
    }

    #[test]
    fn generate_agent_id_is_slug_dashdash_hex12() {
        let id = generate_agent_id("PPT Expert");
        assert!(is_generated_agent_id(&id), "got {id}");
        assert!(id.starts_with("ppt-expert--"), "got {id}");
        let hex = id.rsplit_once("--").unwrap().1;
        assert_eq!(hex.len(), 12);

        let cjk = generate_agent_id("我的助手");
        assert!(is_generated_agent_id(&cjk), "got {cjk}");
        assert!(cjk.starts_with("agent--"), "got {cjk}");

        assert_ne!(generate_agent_id("A"), generate_agent_id("A"));
    }

    #[test]
    fn normalize_keeps_generated_and_legacy_ids() {
        assert_eq!(
            normalize_agent_id("ppt-expert--a1b2c3d4e5f6"),
            "ppt-expert--a1b2c3d4e5f6"
        );
        assert_eq!(normalize_agent_id("PPT Expert"), "ppt-expert");
        assert_eq!(normalize_agent_id(""), DEFAULT_AGENT_ID);
        assert_eq!(normalize_agent_id("default"), "default");
        assert_eq!(normalize_agent_id("workspace"), "default");
        assert!(is_generated_agent_id(&normalize_agent_id(
            "PPT-Expert--AABBCCDDEEFF"
        )));
        // 旧式 agt_ 前缀不再视为新式生成 id
        assert!(!is_generated_agent_id("agt_7f3a91c2d8e4b605"));
    }
}
