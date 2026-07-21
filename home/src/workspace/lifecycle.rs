//! Agent 工作区生命周期：激活、创建、列举与 ensure。

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{apply_auto_lucide_icon, apply_pending_agent_icons, resolve_icon_field};
use crate::GENERATED_SUBDIRS;

use super::agent_config::AgentRuntimeConfig;
use super::paths::{
    active_agent_id, agent_config_dir, agent_id_from_workspace_dir_name, agent_workspace_dir,
    daily_memory_path, default_memory_dir, generate_agent_id, normalize_agent_id, set_active_agent,
    DEFAULT_AGENT_ID,
};
use super::templates::{
    render_template, AGENT_SUBDIRS, CORE_FILES, ENSURED_DIRS, STATE_JSON_FILES,
};

/// 创建默认/继承自默认 Agent 的运行时配置
pub fn write_agent_config(
    base: &Path,
    agent_id: &str,
    name: &str,
    inherit: bool,
) -> anyhow::Result<AgentRuntimeConfig> {
    let id = normalize_agent_id(agent_id);
    let mut cfg = AgentRuntimeConfig {
        id: id.clone(),
        name: name.to_string(),
        inherit_from: if inherit && id != DEFAULT_AGENT_ID {
            Some(DEFAULT_AGENT_ID.to_string())
        } else {
            None
        },
        provider_id: None,
        model: None,
        temperature: None,
        max_turns: None,
        additional_params: None,
        tools_enabled: None,
        mcp: None,
        created_at: chrono::Local::now().to_rfc3339(),
    };

    // 若继承：把全局 tools / mcp 快照写入，便于之后单独改
    if inherit && id != DEFAULT_AGENT_ID {
        if let Ok(tools) = fs::read_to_string(base.join("tools-enabled.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&tools) {
                cfg.tools_enabled = Some(v);
            }
        }
        if let Ok(mcp) = fs::read_to_string(base.join("mcp.json")) {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(&mcp) {
                cfg.mcp = Some(v);
            }
        }
    }

    cfg.save(base)?;
    Ok(cfg)
}

/// 从 AGENT.md / IDENTITY.md 解析的人设字段（内部缓存结构）
#[derive(Debug, Clone, Default)]
struct AgentIdentityFields {
    name: Option<String>,
    emoji: Option<String>,
    avatar: Option<String>,
    vibe: Option<String>,
}

/// 清洗 IDENTITY 列表项中的 Markdown 装饰符号
fn clean_identity_value(raw: &str) -> String {
    raw.trim()
        .trim_matches('*')
        .trim()
        .trim_matches('_')
        .trim()
        .trim_matches('`')
        .trim()
        .to_string()
}

/// 解析 `- Key: value` 形式的 IDENTITY 行，返回小写 key 与清洗后的 value
fn parse_identity_field_line(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim();
    if !trimmed.starts_with('-') {
        return None;
    }
    let rest = trimmed.trim_start_matches('-').trim();
    let (key_part, value_part) = rest.split_once(':')?;
    let key = key_part.trim().trim_matches('*').trim().to_lowercase();
    let value = clean_identity_value(value_part);
    if key.is_empty() || value.is_empty() || value.starts_with("_(") {
        return None;
    }
    Some((key, value))
}

/// 合并读取 `IDENTITY.md` 与 `AGENT.md` 中的 Name/Emoji/Avatar/Vibe（前者优先）
fn read_agent_identity_fields(ws: &Path) -> AgentIdentityFields {
    let mut fields = AgentIdentityFields::default();
    for file in ["IDENTITY.md", "AGENT.md"] {
        let Ok(text) = fs::read_to_string(ws.join(file)) else {
            continue;
        };
        for line in text.lines() {
            let Some((key, value)) = parse_identity_field_line(line) else {
                continue;
            };
            match key.as_str() {
                "name" if fields.name.is_none() => fields.name = Some(value),
                "emoji" if fields.emoji.is_none() => fields.emoji = Some(value),
                "avatar" if fields.avatar.is_none() => fields.avatar = Some(value),
                "vibe" if fields.vibe.is_none() => fields.vibe = Some(value),
                _ => {}
            }
        }
    }
    fields
}

/// 从 AGENT.md / IDENTITY.md 提取显示名
fn read_agent_display_name(ws: &Path, fallback: &str) -> String {
    let fields = read_agent_identity_fields(ws);
    if let Some(name) = fields.name.filter(|n| !n.is_empty()) {
        return name;
    }
    // 回退：读 agents/{id}/config.json
    if let Ok(cfg) = AgentRuntimeConfig::load(
        ws.parent().unwrap_or(ws),
        &agent_id_from_workspace_dir_name(
            ws.file_name().and_then(|s| s.to_str()).unwrap_or(fallback),
        )
        .unwrap_or_else(|| fallback.to_string()),
    ) {
        if !cfg.name.is_empty() {
            return cfg.name;
        }
    }
    fallback.to_string()
}

/// Agent 记忆空间元信息（`workspace` / `workspace-*`），供 UI 列举与切换
#[derive(Debug, Clone, serde::Serialize)]
pub struct AgentInfo {
    /// 规范化 id
    pub id: String,
    /// 显示名（IDENTITY → config.json → id 回退）
    pub name: String,
    /// 工作区目录绝对路径
    pub path: String,
    /// 是否为默认 `workspace` Agent
    pub is_default: bool,
    /// 是否为当前激活 Agent
    pub is_active: bool,
    /// IDENTITY.md 的 Emoji（可为 emoji 字符或图标 URL）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub emoji: Option<String>,
    /// IDENTITY.md 的 Avatar（图标 / 头像 URL）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar: Option<String>,
    /// IDENTITY.md 的 Vibe 一句话
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vibe: Option<String>,
}

/// 列出所有记忆空间（默认 workspace + workspace-*）
pub fn list_agents(base: &Path) -> Vec<AgentInfo> {
    let active = active_agent_id(base);
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let Ok(entries) = fs::read_dir(base) else {
        return out;
    };
    let mut dirs: Vec<_> = entries.flatten().filter(|e| e.path().is_dir()).collect();
    dirs.sort_by_key(|e| e.file_name());

    for entry in dirs {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(id) = agent_id_from_workspace_dir_name(&name) else {
            continue;
        };
        if !seen.insert(id.clone()) {
            continue;
        }
        let ws = entry.path();
        let identity = read_agent_identity_fields(&ws);
        let display = identity
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| read_agent_display_name(&ws, &id));
        out.push(AgentInfo {
            id: id.clone(),
            name: display,
            path: ws.to_string_lossy().into_owned(),
            is_default: id == DEFAULT_AGENT_ID,
            is_active: active == id,
            emoji: resolve_icon_field(&ws, identity.emoji.as_deref()),
            avatar: resolve_icon_field(&ws, identity.avatar.as_deref()),
            vibe: identity.vibe,
        });
    }

    // 默认排最前
    out.sort_by(|a, b| match (a.is_default, b.is_default) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    out
}

/// 创建 Agent 时由技能/工具填入的人设模板字段
#[derive(Debug, Clone, Default)]
pub struct AgentProfile {
    /// 背景经历（写入 AGENT.md）
    pub background: String,
    /// 说话风格（写入 SOUL.md / IDENTITY.md）
    pub style: String,
    /// 主要职责/擅长领域
    pub focus: String,
    /// 明确不做的事（边界）
    pub avoid: String,
    /// 对用户的称呼
    pub call_me: String,
    /// 协作偏好（写入 USER.md）
    pub preferences: String,
}

/// 新建 Agent = 新建 `workspace-{id}` + `agents/{id}/config.json`
pub fn create_agent(base: &Path, name: &str) -> anyhow::Result<AgentInfo> {
    create_agent_with_profile(base, name, None, None, true, true)
}

/// 新建 Agent（完整参数）：工作区 + 配置 + 可选人设 + 是否继承全局配置并激活
///
/// `id` 为空时生成 `{slug}--{hex}`（slug 取自显示名，可读；与显示名解耦、改名不变）；
/// 显式 `id` 仍规范化后使用（测试 / 高级覆盖）。不能覆盖默认 `workspace` 或系统保留名。
pub fn create_agent_with_profile(
    base: &Path,
    name: &str,
    id: Option<&str>,
    profile: Option<&AgentProfile>,
    inherit_config: bool,
    activate: bool,
) -> anyhow::Result<AgentInfo> {
    let display = name.trim();
    if display.is_empty() {
        anyhow::bail!("Agent 名称不能为空");
    }

    let id = match id.map(str::trim).filter(|s| !s.is_empty()) {
        Some(raw) => {
            let normalized = normalize_agent_id(raw);
            if normalized.is_empty() || normalized == DEFAULT_AGENT_ID {
                anyhow::bail!("不能覆盖默认 Agent `workspace`，请换一个 id");
            }
            normalized
        }
        None => allocate_agent_id(base, display)?,
    };

    if id == DEFAULT_AGENT_ID {
        anyhow::bail!("不能覆盖默认 Agent `workspace`，请换一个名称");
    }
    const RESERVED: &[&str] = &[
        "sessions",
        "skills",
        "cron",
        "logs",
        "uploads",
        "cache",
        "agents",
        "workspace",
    ];
    if RESERVED.contains(&id.as_str()) || id.starts_with("workspace-") {
        anyhow::bail!("id `{id}` 为系统保留，请换一个");
    }

    let ws = agent_workspace_dir(base, &id);
    if ws.exists() {
        anyhow::bail!("Agent 工作区已存在: {}", ws.display());
    }

    ensure_agent_space(base, &id, Some(display))?;
    write_agent_config(base, &id, display, inherit_config)?;

    if let Some(p) = profile {
        apply_agent_profile(&ws, &id, display, p)?;
    }

    // 创建引导页上传的 Emoji/Avatar → assets/ + IDENTITY.md
    let _ = apply_pending_agent_icons(base, &ws);

    // 技能 / 工具创建且未手动选图标时：按名称与 profile 自动挑 Lucide 图标
    {
        let focus = profile.map(|p| p.focus.as_str()).unwrap_or("");
        let style = profile.map(|p| p.style.as_str()).unwrap_or("");
        let _ = apply_auto_lucide_icon(&ws, display, focus, style);
    }

    if activate {
        let _ = set_active_agent(base, &id);
    }

    let identity = read_agent_identity_fields(&ws);
    Ok(AgentInfo {
        id: id.clone(),
        name: display.to_string(),
        path: ws.to_string_lossy().into_owned(),
        is_default: false,
        is_active: activate,
        emoji: resolve_icon_field(&ws, identity.emoji.as_deref()),
        avatar: resolve_icon_field(&ws, identity.avatar.as_deref()),
        vibe: identity.vibe,
    })
}

/// 生成不与现有工作区冲突的 `{slug}--{hex}` id
fn allocate_agent_id(base: &Path, name: &str) -> anyhow::Result<String> {
    for _ in 0..16 {
        let id = generate_agent_id(name);
        if !agent_workspace_dir(base, &id).exists() {
            return Ok(id);
        }
    }
    anyhow::bail!("无法生成唯一 Agent id，请重试")
}

/// 按 `AgentProfile` 写入 AGENT/IDENTITY/SOUL/USER/MEMORY 初始内容
fn apply_agent_profile(ws: &Path, id: &str, name: &str, p: &AgentProfile) -> anyhow::Result<()> {
    let agent_md = format!(
        r#"# AGENT.md — 本记忆空间的 Agent

- **Name:** {name}
- **Id:** {id}
- **Focus:** {focus}
- **Scope / 不要做:** {avoid}

## 背景经历

{background}

## 记忆空间

- **长期精炼：** `MEMORY.md`
- **每日记忆：** `memory/YYYY-MM-DD.md`
- **专属技能：** `skills/`
- **公共技能：** `~/.astro/skills`
- **运行配置：** `~/.astro/agents/{id}/config.json`
"#,
        name = name,
        id = id,
        focus = if p.focus.is_empty() {
            "_(待补充)_"
        } else {
            &p.focus
        },
        avoid = if p.avoid.is_empty() {
            "_(待补充)_"
        } else {
            &p.avoid
        },
        background = if p.background.is_empty() {
            "_(待补充)_"
        } else {
            &p.background
        },
    );
    fs::write(ws.join("AGENT.md"), agent_md)?;

    let identity = format!(
        r#"# IDENTITY.md — {name} 是谁

- **Name:** {name}
- **Role:** {focus}
- **Vibe:** {style}
- **Emoji:** _(可选：上传到 assets/emoji.png)_
- **Avatar:** _(可选：上传到 assets/avatar.png)_

## 职责

- {focus}
- 维护本记忆空间中的记忆与规范文件
- 不越界：{avoid}
"#,
        name = name,
        focus = if p.focus.is_empty() {
            "自我进化的 AI 助手"
        } else {
            &p.focus
        },
        style = if p.style.is_empty() {
            "干练、有主见、务实"
        } else {
            &p.style
        },
        avoid = if p.avoid.is_empty() {
            "破坏性操作前先确认"
        } else {
            &p.avoid
        },
    );
    fs::write(ws.join("IDENTITY.md"), identity)?;

    if !p.style.is_empty() {
        let soul = format!(
            r#"# SOUL.md — 表达风格与行为准则

## 说话风格

{style}

## 核心原则

**真正有用，而不是表演有用。** 跳过客套——直接做事。

**有主见。** 允许不同意、有偏好。

**先自助再提问。** 先读文件、查上下文；卡住了再问。

## 边界

- 隐私默认不外泄
- 不确定时，先问再对外行动
- 不伪造执行结果
- 不要做：{avoid}
"#,
            style = p.style,
            avoid = if p.avoid.is_empty() {
                "破坏性操作前先确认"
            } else {
                &p.avoid
            },
        );
        fs::write(ws.join("SOUL.md"), soul)?;
    }

    let user = format!(
        r#"# USER.md — 关于用户

- **What to call them:** {call_me}
- **Timezone:** Asia/Shanghai
- **Language:** 中文为主

## 协作偏好

{preferences}

## Context

_(随协作持续更新)_
"#,
        call_me = if p.call_me.is_empty() {
            "_(待补充)_"
        } else {
            &p.call_me
        },
        preferences = if p.preferences.is_empty() {
            "_(待补充)_"
        } else {
            &p.preferences
        },
    );
    fs::write(ws.join("USER.md"), user)?;

    let memory = format!(
        r#"# MEMORY.md — 长期精炼记忆

跨会话保留的结构化事实。日常流水请写入 `memory/YYYY-MM-DD.md`。

- Agent「{name}」已创建（id: {id}）
"#,
        name = name,
        id = id,
    );
    fs::write(ws.join("MEMORY.md"), memory)?;

    Ok(())
}

/// 确保日记忆文件存在（不存在则写模板）
pub fn ensure_daily_memory(workspace: &Path, date: &str) -> anyhow::Result<PathBuf> {
    let dir = workspace.join(super::paths::DAILY_MEMORY_DIR);
    fs::create_dir_all(&dir)?;
    let path = daily_memory_path(workspace, date);
    if !path.exists() {
        let content = format!(
            "# {date} — 每日记忆\n\n_当日会话与事件的流水记录，可经提炼写入 MEMORY.md。_\n\n"
        );
        fs::write(&path, content)?;
    }
    Ok(path)
}

/// 确保单个 Agent 记忆空间的核心文件与子目录
pub fn ensure_agent_space(
    base: &Path,
    agent_id: &str,
    display_name: Option<&str>,
) -> anyhow::Result<PathBuf> {
    let id = normalize_agent_id(agent_id);
    let workspace = agent_workspace_dir(base, &id);
    fs::create_dir_all(&workspace)?;
    fs::create_dir_all(agent_config_dir(base, &id))?;

    for sub in AGENT_SUBDIRS {
        fs::create_dir_all(workspace.join(sub))?;
    }

    for rel in GENERATED_SUBDIRS {
        fs::create_dir_all(workspace.join(rel))?;
    }

    let name = display_name.map(|s| s.to_string()).unwrap_or_else(|| {
        if id == DEFAULT_AGENT_ID {
            "Astro".to_string()
        } else {
            id.clone()
        }
    });

    for (filename, template) in CORE_FILES {
        let dest = workspace.join(filename);
        if !dest.exists() {
            let content = render_template(template, &id, &name);
            fs::write(&dest, content)?;
        }
    }

    if !AgentRuntimeConfig::path(base, &id).is_file() {
        let _ = write_agent_config(base, &id, &name, id != DEFAULT_AGENT_ID);
    }

    Ok(workspace)
}

/// 创建目录与核心模板（已存在的文件不覆盖）。
///
/// 不含会话库初始化与技能播种——完整引导见 `memory::ensure_workspace`。
///
/// 布局：
/// ```text
/// ~/.astro/
/// ├── workspace/                 ← 默认 Agent 工作区
/// ├── workspace-{id}/            ← 其他 Agent 工作区（同构）
/// ├── agents/{id}/config.json    ← 每 Agent 的模型/工具/MCP 配置
/// └── active-agent.json
/// ```
pub fn ensure_workspace(base: &Path) -> anyhow::Result<EnsureWorkspaceReport> {
    fs::create_dir_all(base)?;

    let mut ensured_dirs = Vec::new();
    for rel in ENSURED_DIRS {
        let dir = base.join(rel);
        fs::create_dir_all(&dir)?;
        ensured_dirs.push((*rel).to_string());
    }

    let mut created_files = Vec::new();

    // 统计默认空间本次新建的核心文件
    let workspace = agent_workspace_dir(base, DEFAULT_AGENT_ID);
    for (name, _) in CORE_FILES {
        let dest = workspace.join(name);
        if !dest.exists() {
            created_files.push(format!("workspace/{name}"));
        }
    }
    let _ = ensure_agent_space(base, DEFAULT_AGENT_ID, Some("Astro"))?;

    for (rel, content) in STATE_JSON_FILES {
        let path = base.join(rel);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        if !path.exists() {
            fs::write(&path, content)?;
            created_files.push((*rel).to_string());
        }
    }

    // 确保已有额外 agents 也具备子目录与缺失的核心文件
    for info in list_agents(base) {
        if info.id != DEFAULT_AGENT_ID {
            let _ = ensure_agent_space(base, &info.id, Some(&info.name))?;
        }
    }

    Ok(EnsureWorkspaceReport {
        base_dir: base.to_path_buf(),
        workspace_dir: agent_workspace_dir(base, &active_agent_id(base)),
        created_files,
        ensured_dirs,
    })
}

/// 对默认目录执行 [`ensure_workspace`]（不含会话库与技能播种）。
pub fn ensure_default_workspace() -> anyhow::Result<EnsureWorkspaceReport> {
    ensure_workspace(&default_memory_dir())
}

/// `ensure_workspace` 的返回摘要：新建文件列表与已确保的目录
#[derive(Debug, Clone)]
pub struct EnsureWorkspaceReport {
    /// 数据根目录（`~/.astro` 或 `ASTRO_MEMORY_DIR`）
    pub base_dir: PathBuf,
    /// 当前激活 Agent 的工作区路径
    pub workspace_dir: PathBuf,
    /// 本次新建的文件相对路径（已存在的不列入）
    pub created_files: Vec<String>,
    /// 本次确保存在的目录相对路径
    pub ensured_dirs: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::super::paths::{
        active_agent_id, agent_workspace_dir, is_generated_agent_id, list_daily_memory_dates,
        set_active_agent,
    };
    use super::super::templates::{CORE_FILES, ENSURED_DIRS, STATE_JSON_FILES};
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn generated_paths_helpers() {
        use crate::{generated_dir, GeneratedKind, GENERATED_SUBDIRS};
        use std::path::Path;

        let ws = Path::new("/agent/ws");
        assert_eq!(
            generated_dir(ws, GeneratedKind::Videos),
            Path::new("/agent/ws/generated/videos")
        );
        assert_eq!(GENERATED_SUBDIRS.len(), 8);
        for rel in GENERATED_SUBDIRS {
            assert!(rel.starts_with("generated/"));
        }
    }

    #[test]
    fn ensure_workspace_creates_core_layout() {
        let dir = TempDir::new().unwrap();
        let report = ensure_workspace(dir.path()).unwrap();

        let ws = dir.path().join("workspace");
        assert!(ws.is_dir());
        assert!(ws.join("memory").is_dir());
        assert!(ws.join("skills").is_dir());
        for rel in GENERATED_SUBDIRS {
            assert!(ws.join(rel).is_dir(), "missing workspace/{rel}");
        }
        for rel in ENSURED_DIRS {
            assert!(dir.path().join(rel).is_dir(), "missing dir {rel}");
        }
        for (name, _) in CORE_FILES {
            assert!(ws.join(name).is_file(), "missing workspace/{name}");
        }
        assert!(dir.path().join("active-agent.json").is_file());
        assert_eq!(
            report.created_files.len(),
            CORE_FILES.len() + STATE_JSON_FILES.len()
        );
        assert!(report.created_files.iter().any(|f| f == "cron/jobs.json"));
        assert!(report.created_files.iter().any(|f| f == "models.json"));
        assert!(report
            .created_files
            .iter()
            .any(|f| f == "skills-enabled.json"));
        assert!(report
            .created_files
            .iter()
            .any(|f| f == "tools-enabled.json"));
        assert!(report.created_files.iter().any(|f| f == "mcp.json"));
        assert!(report
            .created_files
            .iter()
            .any(|f| f == "active-agent.json"));
        assert_eq!(report.workspace_dir, ws);
        assert_eq!(report.ensured_dirs.len(), ENSURED_DIRS.len());
        assert!(dir.path().join("cron/jobs.json").is_file());
        assert!(dir.path().join("cron/output").is_dir());
        assert!(dir.path().join("models.json").is_file());
        assert!(dir.path().join("skills-enabled.json").is_file());
        assert!(dir.path().join("tools-enabled.json").is_file());
        assert!(dir.path().join("mcp.json").is_file());
    }

    #[test]
    fn ensure_workspace_does_not_overwrite() {
        let dir = TempDir::new().unwrap();
        let ws = dir.path().join("workspace");
        fs::create_dir_all(&ws).unwrap();
        let soul = ws.join("SOUL.md");
        fs::write(&soul, "custom soul").unwrap();

        ensure_workspace(dir.path()).unwrap();
        let content = fs::read_to_string(&soul).unwrap();
        assert_eq!(content, "custom soul");
        assert!(ws.join("IDENTITY.md").is_file());
        assert!(ws.join("AGENT.md").is_file());
    }

    #[test]
    fn create_and_list_agents() {
        let dir = TempDir::new().unwrap();
        ensure_workspace(dir.path()).unwrap();

        let info = create_agent(dir.path(), "PPT Expert").unwrap();
        assert!(
            is_generated_agent_id(&info.id),
            "expected {{slug}}--{{hex}} id, got {}",
            info.id
        );
        assert!(
            info.id.starts_with("ppt-expert--"),
            "expected readable slug prefix, got {}",
            info.id
        );
        assert_eq!(info.name, "PPT Expert");
        assert!(info.path.ends_with(&format!("workspace-{}", info.id)));

        let agents = list_agents(dir.path());
        assert!(agents.iter().any(|a| a.is_default));
        assert!(agents
            .iter()
            .any(|a| a.id == info.id && a.name == "PPT Expert"));

        let ws = PathBuf::from(&info.path);
        assert!(ws.join("AGENT.md").is_file());
        assert!(ws.join("MEMORY.md").is_file());
        assert!(ws.join("memory").is_dir());
        assert!(ws.join("skills").is_dir());
        assert!(dir
            .path()
            .join(format!("agents/{}/config.json", info.id))
            .is_file());

        set_active_agent(dir.path(), &info.id).unwrap();
        assert_eq!(active_agent_id(dir.path()), info.id);
        assert_eq!(
            agent_workspace_dir(dir.path(), &active_agent_id(dir.path())),
            agent_workspace_dir(dir.path(), &info.id)
        );

        // 同名可再建（id 不同）
        let info2 = create_agent(dir.path(), "PPT Expert").unwrap();
        assert_ne!(info.id, info2.id);
        assert!(is_generated_agent_id(&info2.id));
    }

    #[test]
    fn create_agent_keeps_legacy_explicit_id() {
        let dir = TempDir::new().unwrap();
        ensure_workspace(dir.path()).unwrap();
        let info = create_agent_with_profile(
            dir.path(),
            "遗留助手",
            Some("legacy-slug"),
            None,
            true,
            false,
        )
        .unwrap();
        assert_eq!(info.id, "legacy-slug");
        assert!(info.path.ends_with("workspace-legacy-slug"));
    }

    #[test]
    fn list_agents_reads_identity_icons() {
        let dir = TempDir::new().unwrap();
        ensure_workspace(dir.path()).unwrap();
        let ws = dir.path().join("workspace-ima");
        fs::create_dir_all(ws.join("assets")).unwrap();
        fs::write(ws.join("assets/emoji.png"), b"emoji").unwrap();
        fs::write(ws.join("assets/avatar.png"), b"avatar").unwrap();
        fs::write(
            ws.join("IDENTITY.md"),
            r#"# IDENTITY.md
- Name: ima知识库检索专家
- Emoji: assets/emoji.png
- Vibe: 找文档翻到崩溃？说句话我秒级定位
- Avatar: assets/avatar.png
"#,
        )
        .unwrap();
        write_agent_config(dir.path(), "ima", "ima知识库检索专家", false).unwrap();

        let agents = list_agents(dir.path());
        let ima = agents.iter().find(|a| a.id == "ima").expect("ima agent");
        assert_eq!(ima.name, "ima知识库检索专家");
        assert!(ima.emoji.as_ref().unwrap().ends_with("assets/emoji.png"));
        assert!(ima.avatar.as_ref().unwrap().ends_with("assets/avatar.png"));
        assert_eq!(
            ima.vibe.as_deref(),
            Some("找文档翻到崩溃？说句话我秒级定位")
        );
    }

    #[test]
    fn create_agent_with_profile_fills_md() {
        let dir = TempDir::new().unwrap();
        ensure_workspace(dir.path()).unwrap();
        let profile = AgentProfile {
            background: "十年 PPT 设计师".into(),
            style: "简洁专业".into(),
            focus: "做演示文稿".into(),
            avoid: "改系统配置".into(),
            call_me: "老板".into(),
            preferences: "先给大纲".into(),
        };
        let info = create_agent_with_profile(
            dir.path(),
            "演示专家",
            Some("demo-expert"),
            Some(&profile),
            true,
            true,
        )
        .unwrap();
        assert_eq!(info.id, "demo-expert");
        assert!(info.path.ends_with("workspace-demo-expert"));
        let ws = PathBuf::from(&info.path);
        let agent = fs::read_to_string(ws.join("AGENT.md")).unwrap();
        assert!(agent.contains("十年 PPT"));
        let user = fs::read_to_string(ws.join("USER.md")).unwrap();
        assert!(user.contains("老板"));
        assert_eq!(active_agent_id(dir.path()), info.id);
        // 未手动选图标时自动写入 Lucide SVG
        assert!(
            ws.join("assets/emoji.svg").is_file(),
            "expected auto lucide emoji.svg"
        );
        assert!(info.emoji.as_ref().unwrap().ends_with("assets/emoji.svg"));
    }

    #[test]
    fn daily_memory_roundtrip() {
        let dir = TempDir::new().unwrap();
        ensure_workspace(dir.path()).unwrap();
        let ws = dir.path().join("workspace");
        let path = ensure_daily_memory(&ws, "2026-07-11").unwrap();
        assert!(path.is_file());
        assert!(path.starts_with(ws.join("memory")));
        assert_eq!(list_daily_memory_dates(&ws), vec!["2026-07-11".to_string()]);
    }
}
