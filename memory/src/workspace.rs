//! Agent 工作区与 Astro 数据根目录布局。
//!
//! 职责：
//! - 解析 `~/.astro`（或 `ASTRO_MEMORY_DIR`）下的多 Agent 目录约定
//! - 工作区（`workspace` / `workspace-{id}`）的创建、列举、激活与核心模板
//! - 日记忆（`mermaid/YYYY-MM-DD.md`）路径与初始化
//! - 首次启动时创建目录树、状态 JSON、会话库与公共技能
//!
//! 不变量：
//! - 默认 Agent id 恒为 `workspace`；其他 id 对应 `workspace-{id}` 目录
//! - 工作区 Markdown 在 `workspace-*`，模型/工具/MCP 配置在 `agents/{id}/config.json`
//! - `ensure_*` / `create_*` 不覆盖已存在的用户文件内容

use std::fs;
use std::path::{Path, PathBuf};

use crate::session_store::SessionStore;

/// 默认 Agent 的 id / 目录名：`~/.astro/workspace`
pub const DEFAULT_AGENT_ID: &str = "workspace";

/// 当前激活 Agent 的持久化文件名（位于数据根目录）
const ACTIVE_AGENT_FILE: &str = "active-agent.json";

/// 默认记忆/工作空间根目录：`$ASTRO_MEMORY_DIR` 或 `~/.astro`
pub fn default_memory_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ASTRO_MEMORY_DIR") {
        return PathBuf::from(dir);
    }
    std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .map(|home| PathBuf::from(home).join(".astro"))
        .unwrap_or_else(|_| PathBuf::from(".astro"))
}

/// 解析 Agent 工作区路径。
///
/// - `workspace`（默认）→ `{base}/workspace`
/// - 其他 id → `{base}/workspace-{id}`
pub fn agent_workspace_dir(base: &Path, agent_id: &str) -> PathBuf {
    let id = normalize_agent_id(agent_id);
    if id == DEFAULT_AGENT_ID {
        base.join(DEFAULT_AGENT_ID)
    } else {
        base.join(format!("workspace-{id}"))
    }
}

/// Agent 配置目录：`{base}/agents/{id}/`（模型、工具、MCP 等，不含工作区文件）
pub fn agent_config_dir(base: &Path, agent_id: &str) -> PathBuf {
    let id = normalize_agent_id(agent_id);
    base.join("agents").join(id)
}

/// 从工作区目录名解析 agent id（`workspace` / `workspace-xxx`）
pub fn agent_id_from_workspace_dir_name(name: &str) -> Option<String> {
    if name == DEFAULT_AGENT_ID {
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

/// 兼容旧单参调用：等价于 `agent_workspace_dir(base, DEFAULT_AGENT_ID)`
pub fn default_workspace_dir(base: &Path) -> PathBuf {
    agent_workspace_dir(base, DEFAULT_AGENT_ID)
}

/// 规范化 agent id：小写、空格转 `-`，仅保留 `[a-z0-9_-]`
/// 纯非 ASCII 名称（如中文）用 `agent-{hash}` 兜底，避免落到默认 `workspace`
pub fn normalize_agent_id(raw: &str) -> String {
    let s = raw.trim().to_lowercase().replace(' ', "-");
    let cleaned: String = s
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    let cleaned = cleaned
        .trim_matches('-')
        .trim_matches('_')
        .to_string();
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
    let path = base.join(ACTIVE_AGENT_FILE);
    let Ok(text) = fs::read_to_string(&path) else {
        return DEFAULT_AGENT_ID.to_string();
    };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) else {
        return DEFAULT_AGENT_ID.to_string();
    };
    v.get("id")
        .and_then(|x| x.as_str())
        .map(normalize_agent_id)
        .filter(|id| agent_workspace_dir(base, id).is_dir())
        .unwrap_or_else(|| DEFAULT_AGENT_ID.to_string())
}

/// 设置当前激活的 Agent（目标工作区必须已存在）
pub fn set_active_agent(base: &Path, agent_id: &str) -> anyhow::Result<String> {
    let id = normalize_agent_id(agent_id);
    let dir = agent_workspace_dir(base, &id);
    if !dir.is_dir() {
        anyhow::bail!("Agent 工作区不存在: {id}");
    }
    let path = base.join(ACTIVE_AGENT_FILE);
    let json = serde_json::json!({ "id": id });
    fs::write(&path, format!("{}\n", serde_json::to_string_pretty(&json)?))?;
    Ok(id)
}

/// 单个 Agent 的运行时配置，持久化于 `agents/{id}/config.json`。
///
/// 为 null 的字段表示「继承全局默认」或由上层 Builder 回退；工作区 Markdown 不在此文件。
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AgentRuntimeConfig {
    /// 规范化后的 Agent id（与 `workspace-{id}` 目录名对应）
    pub id: String,
    /// 显示名称（UI / 列表用，可与 IDENTITY.md 的 Name 不同步）
    pub name: String,
    /// 继承哪个 Agent 的配置（通常为 `workspace`）；字段为 null 时用全局默认
    #[serde(default)]
    pub inherit_from: Option<String>,
    /// LLM Provider id；null 时用全局 `models.json` 默认
    #[serde(default)]
    pub provider_id: Option<String>,
    /// 模型名；null 时用 Provider 默认
    #[serde(default)]
    pub model: Option<String>,
    /// 采样温度；缺省由 ProviderConfig / AgentBuilder 回退
    #[serde(default)]
    pub temperature: Option<f32>,
    /// 工具循环最大轮次（对齐 Rig multi_turn）
    #[serde(default)]
    pub max_turns: Option<usize>,
    /// Provider 扩展参数（reasoning / vendor extras），对齐 Rig additional_params
    #[serde(default)]
    pub additional_params: Option<serde_json::Value>,
    /// 为 null 时使用全局 `tools-enabled.json`
    #[serde(default)]
    pub tools_enabled: Option<serde_json::Value>,
    /// 为 null 时使用全局 `mcp.json`
    #[serde(default)]
    pub mcp: Option<serde_json::Value>,
    /// ISO 8601 创建时间（本地时区 RFC3339）
    #[serde(default)]
    pub created_at: String,
}

impl AgentRuntimeConfig {
    /// 该 Agent 的 `config.json` 绝对路径
    pub fn path(base: &Path, agent_id: &str) -> PathBuf {
        agent_config_dir(base, agent_id).join("config.json")
    }

    /// 从磁盘加载配置；文件不存在时返回错误
    pub fn load(base: &Path, agent_id: &str) -> anyhow::Result<Self> {
        let path = Self::path(base, agent_id);
        if !path.is_file() {
            anyhow::bail!("Agent 配置不存在: {}", path.display());
        }
        let text = fs::read_to_string(&path)?;
        Ok(serde_json::from_str(&text)?)
    }

    /// 将配置写回 `agents/{id}/config.json`（自动创建目录）
    pub fn save(&self, base: &Path) -> anyhow::Result<()> {
        let dir = agent_config_dir(base, &self.id);
        fs::create_dir_all(&dir)?;
        let path = dir.join("config.json");
        fs::write(&path, format!("{}\n", serde_json::to_string_pretty(self)?))?;
        Ok(())
    }
}

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
    let key = key_part
        .trim()
        .trim_matches('*')
        .trim()
        .to_lowercase();
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
            ws.file_name()
                .and_then(|s| s.to_str())
                .unwrap_or(fallback),
        )
        .unwrap_or_else(|| fallback.to_string()),
    ) {
        if !cfg.name.is_empty() {
            return cfg.name;
        }
    }
    fallback.to_string()
}

/// 将旧版 `agents/{id}/` 工作区迁移到 `workspace-{id}/`，配置留在 `agents/{id}/`
fn migrate_legacy_agent_workspaces(base: &Path) -> anyhow::Result<()> {
    let agents_root = base.join("agents");
    if !agents_root.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(&agents_root)?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        if id == DEFAULT_AGENT_ID {
            continue;
        }
        // 旧布局：agents/{id} 里直接放 MEMORY.md / IDENTITY.md
        let looks_like_workspace = path.join("MEMORY.md").is_file()
            || path.join("IDENTITY.md").is_file()
            || path.join("SOUL.md").is_file();
        if !looks_like_workspace {
            continue;
        }
        let new_ws = agent_workspace_dir(base, &id);
        if !new_ws.exists() {
            fs::rename(&path, &new_ws)?;
            // 重建空的 agents/{id} 放配置
            fs::create_dir_all(agent_config_dir(base, &id))?;
            let name = read_agent_display_name(&new_ws, &id);
            let _ = write_agent_config(base, &id, &name, true);
        }
    }
    Ok(())
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
    let _ = migrate_legacy_agent_workspaces(base);
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
            emoji: crate::agent_icons::resolve_icon_field(&ws, identity.emoji.as_deref()),
            avatar: crate::agent_icons::resolve_icon_field(&ws, identity.avatar.as_deref()),
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
/// `id` 为空时由 `name` 规范化生成；不能覆盖默认 `workspace` 或系统保留名。
pub fn create_agent_with_profile(
    base: &Path,
    name: &str,
    id: Option<&str>,
    profile: Option<&AgentProfile>,
    inherit_config: bool,
    activate: bool,
) -> anyhow::Result<AgentInfo> {
    let display = name.trim();
    let id = id
        .map(normalize_agent_id)
        .filter(|s| !s.is_empty() && s != DEFAULT_AGENT_ID)
        .unwrap_or_else(|| normalize_agent_id(display));
    if id == DEFAULT_AGENT_ID {
        anyhow::bail!("不能覆盖默认 Agent `workspace`，请换一个名称");
    }
    const RESERVED: &[&str] = &[
        "sessions", "skills", "cron", "logs", "uploads", "cache", "agents", "workspace",
    ];
    if RESERVED.contains(&id.as_str()) || id.starts_with("workspace-") {
        anyhow::bail!("名称 `{id}` 为系统保留，请换一个");
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
    let _ = crate::agent_icons::apply_pending_agent_icons(base, &ws);

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
        emoji: crate::agent_icons::resolve_icon_field(&ws, identity.emoji.as_deref()),
        avatar: crate::agent_icons::resolve_icon_field(&ws, identity.avatar.as_deref()),
        vibe: identity.vibe,
    })
}

/// 按 `AgentProfile` 写入 AGENT/IDENTITY/SOUL/USER/MEMORY 初始内容
fn apply_agent_profile(
    ws: &Path,
    id: &str,
    name: &str,
    p: &AgentProfile,
) -> anyhow::Result<()> {
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
- **每日记忆：** `mermaid/YYYY-MM-DD.md`
- **专属技能：** `skills/`
- **公共技能：** `~/.astro/skills`
- **运行配置：** `~/.astro/agents/{id}/config.json`
"#,
        name = name,
        id = id,
        focus = if p.focus.is_empty() { "_(待补充)_" } else { &p.focus },
        avoid = if p.avoid.is_empty() { "_(待补充)_" } else { &p.avoid },
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
        avoid = if p.avoid.is_empty() { "破坏性操作前先确认" } else { &p.avoid },
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
            avoid = if p.avoid.is_empty() { "破坏性操作前先确认" } else { &p.avoid },
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
        call_me = if p.call_me.is_empty() { "_(待补充)_" } else { &p.call_me },
        preferences = if p.preferences.is_empty() {
            "_(待补充)_"
        } else {
            &p.preferences
        },
    );
    fs::write(ws.join("USER.md"), user)?;

    let memory = format!(
        r#"# MEMORY.md — 长期精炼记忆

跨会话保留的结构化事实。日常流水请写入 `mermaid/YYYY-MM-DD.md`。

- Agent「{name}」已创建（id: {id}）
"#,
        name = name,
        id = id,
    );
    fs::write(ws.join("MEMORY.md"), memory)?;

    Ok(())
}

/// 某 Agent 工作区内的日记忆路径：`mermaid/YYYY-MM-DD.md`
pub fn daily_memory_path(workspace: &Path, date: &str) -> PathBuf {
    workspace.join("mermaid").join(format!("{date}.md"))
}

/// 今日日期（本地）`YYYY-MM-DD`
pub fn today_date_string() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 列出工作区内已有的日记忆文件名（不含扩展名），新→旧
pub fn list_daily_memory_dates(workspace: &Path) -> Vec<String> {
    let dir = workspace.join("mermaid");
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
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .filter(|s| s.len() == 10 && s.chars().nth(4) == Some('-') && s.chars().nth(7) == Some('-'))
        .collect();
    dates.sort();
    dates.reverse();
    dates
}

/// 确保日记忆文件存在（不存在则写模板）
pub fn ensure_daily_memory(workspace: &Path, date: &str) -> anyhow::Result<PathBuf> {
    let mermaid = workspace.join("mermaid");
    fs::create_dir_all(&mermaid)?;
    let path = daily_memory_path(workspace, date);
    if !path.exists() {
        let content = format!(
            "# {date} — 每日记忆\n\n_当日会话与事件的流水记录，可经提炼写入 MEMORY.md。_\n\n"
        );
        fs::write(&path, content)?;
    }
    Ok(path)
}

/// 工作区核心 Markdown 模板（文件名 → 模板正文，含 `{{ID}}` / `{{NAME}}` 占位符）
const CORE_FILES: &[(&str, &str)] = &[
    ("AGENT.md", TEMPLATE_AGENT),
    ("IDENTITY.md", TEMPLATE_IDENTITY),
    ("USER.md", TEMPLATE_USER),
    ("SOUL.md", TEMPLATE_SOUL),
    ("AGENTS.md", TEMPLATE_AGENTS),
    ("TOOLS.md", TEMPLATE_TOOLS),
    ("MEMORY.md", TEMPLATE_MEMORY),
];

/// Agent 工作区内需要确保存在的子目录
const AGENT_SUBDIRS: &[&str] = &["mermaid", "skills"];

const TEMPLATE_AGENT: &str = r#"# AGENT.md — 本记忆空间的 Agent

_描述这个 Agent 的定位、擅长领域与边界。新建 Agent 时请改写。_

- **Name:** {{NAME}}
- **Id:** {{ID}}
- **Focus:** _(擅长什么？服务哪类任务？)_
- **Scope:** _(不做什么？)_

## 记忆空间

- **长期精炼：** `MEMORY.md` — 跨会话稳定事实与决策
- **每日记忆：** `mermaid/YYYY-MM-DD.md` — 当日流水，可再提炼进 MEMORY.md
- **专属技能：** `skills/` — 仅本 Agent 可用
- **公共技能：** `~/.astro/skills` — 所有 Agent 共享

## 启动检查

会话开始时优先依赖运行时注入的上下文；需要细节时再读本目录文件。
"#;

const TEMPLATE_IDENTITY: &str = r#"# IDENTITY.md — Astro Agent 是谁

_首次对话时填写，并随协作一起演化。_

- **Name:** {{NAME}}
- **Role:** 自我进化的 AI 助手与数字搭档
- **Vibe:** 干练、有主见、务实
- **Emoji:** _(可选：上传到 assets/emoji.png)_
- **Avatar:** _(可选：上传到 assets/avatar.png)_

## 职责

- 理解用户目标，用工具真正落地，而不是口头计划
- 维护本记忆空间中的记忆与规范文件
- 在质量与速度之间做清晰取舍，并说明理由
"#;

const TEMPLATE_USER: &str = r#"# USER.md — 关于用户

_了解你在帮助的人。随协作持续更新。_

- **Name:**
- **What to call them:**
- **Timezone:** Asia/Shanghai
- **Language:** 中文为主
- **Notes:**

## 协作偏好

- _(喜欢怎样的回复长度？是否偏好先给结论？)_
- _(常用技术栈 / 项目背景)_
- _(需要避免的风格或行为)_

## Context

_(他们在做什么项目？关心什么？什么会让他们烦？什么会让他们笑？)_
"#;

const TEMPLATE_SOUL: &str = r#"# SOUL.md — 表达风格与行为准则

你不是复读机式的客服。你是正在形成稳定人格的协作者。

## 核心原则

**真正有用，而不是表演有用。** 跳过「好问题！」「我很乐意帮忙！」——直接做事。

**有主见。** 允许不同意、有偏好、觉得某事有趣或无聊。没有个性的助手只是多了几步的搜索引擎。

**先自助再提问。** 先读文件、查上下文、搜索；卡住了再问。目标是带着答案回来，而不是带着问题。

**用能力赢得信任。** 对外部动作（发信、公开发布）谨慎；对内部动作（阅读、整理、学习）大胆。

## 边界

- 隐私默认不外泄
- 不确定时，先问再对外行动
- 不伪造执行结果、测试报告或 API 响应
- 破坏性操作前先确认

## 气质

该短则短，该细则细。不是企业话术，不是谄媚，只是靠谱。

## 连续性

每次会话你都是「醒来」的。这些文件就是你的记忆——读它们，更新它们。
"#;

const TEMPLATE_AGENTS: &str = r#"# AGENTS.md — 本记忆空间的工作方式

本目录是这个 Agent 的家。按家的标准对待它。

## 会话启动

优先使用运行时注入的启动上下文。其中可能已包含：

- `AGENT.md` / `IDENTITY.md` / `SOUL.md` / `USER.md`
- `MEMORY.md`（长期精炼记忆）
- 当日 `mermaid/YYYY-MM-DD.md`（每日记忆）

不要重复通读启动文件，除非：

1. 用户明确要求
2. 注入上下文缺失你需要的信息
3. 需要比启动上下文更深的跟进阅读

## 记忆空间

- **长期精炼：** `MEMORY.md` — 跨会话稳定事实与决策（提炼后的结论）
- **每日记忆：** `mermaid/YYYY-MM-DD.md` — 当日流水与事件
- **用户档案：** `USER.md` — 称呼、背景、协作偏好
- **会话检索：** `~/.astro/sessions/`（全局会话库）

想记住的事必须写入文件。「心里记一下」撑不过重启。

## 技能

- **专属：** 本目录 `skills/` — 仅本 Agent
- **公共：** `~/.astro/skills` — 所有 Agent 共享

## 职责范围

**可自由做：**

- 读文件、探索、整理、学习
- 在本记忆空间内工作
- 用工具验证后再下结论

**先问再做：**

- 对外发送（邮件、社媒、公开帖）
- 破坏性命令、不可逆删除
- 改动系统级配置（crontab、shell rc 等）

## 协作原则

1. 结论先行，细节按需展开
2. 改文件前先读现有内容
3. 用绝对路径操作文件
4. 犯错就记进相关文件，避免未来的自己重蹈覆辙
"#;

const TEMPLATE_TOOLS: &str = r#"# TOOLS.md — 工具与环境备忘

Skills 定义工具「怎么用」。本文件记录「你这台机器上的具体细节」，避免口头记住却从未写入。

## 写什么

- SSH 主机与别名
- 常用目录与绝对路径
- 偏好的模型 / Provider
- 设备昵称、TTS 音色
- 任何环境相关、不宜写进共享 Skill 的信息

## 示例

```markdown
### 路径

- 本记忆空间 → 当前 Agent 工作区
- 公共技能 → ~/.astro/skills
- 数据根目录 → ~/.astro

### Provider

- 默认：……
```

## 为什么单独放

Skills 可共享；你的环境是你的。分开后更新 Skill 不会冲掉本地备忘，分享 Skill 也不会泄露基础设施。
"#;

const TEMPLATE_MEMORY: &str = r#"# MEMORY.md — 长期精炼记忆

跨会话保留的结构化事实。只写提炼后的结论，日常流水请写入 `mermaid/YYYY-MM-DD.md`。

- Astro 记忆空间已初始化
"#;

/// 数据根下需要确保存在的目录（相对 `~/.astro`）
const ENSURED_DIRS: &[&str] = &[
    "workspace",
    "agents",
    "sessions",
    "skills",
    "cron",
    "cron/output",
    "logs",
    "uploads",
    "cache/images",
    "cache/videos",
    "cache/audio",
];

/// 数据根下需要确保存在的空 JSON 状态文件
const STATE_JSON_FILES: &[(&str, &str)] = &[
    ("skills-enabled.json", "{\n}\n"),
    ("tools-enabled.json", "{\n}\n"),
    ("mcp.json", "{\n  \"servers\": []\n}\n"),
    ("models.json", "{\n  \"providers\": {}\n}\n"),
    ("dreaming.json", "{\n  \"enabled\": false\n}\n"),
    ("cron/jobs.json", "{\n  \"jobs\": []\n}\n"),
    ("active-agent.json", "{\n  \"id\": \"workspace\"\n}\n"),
];

/// 将模板占位符替换为实际 id 与显示名
fn render_template(template: &str, agent_id: &str, display_name: &str) -> String {
    template
        .replace("{{ID}}", agent_id)
        .replace("{{NAME}}", display_name)
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

    let name = display_name
        .map(|s| s.to_string())
        .unwrap_or_else(|| {
            if id == DEFAULT_AGENT_ID {
                "Astro".to_string()
            } else {
                id.clone()
            }
        });

    for (filename, template) in CORE_FILES {
        let dest = workspace.join(filename);
        let legacy = base.join(filename);
        if !dest.exists() {
            if id == DEFAULT_AGENT_ID && legacy.is_file() {
                fs::rename(&legacy, &dest)?;
            } else {
                let content = render_template(template, &id, &name);
                fs::write(&dest, content)?;
            }
        } else if id == DEFAULT_AGENT_ID && legacy.is_file() {
            let _ = fs::remove_file(&legacy);
        }
    }

    if !AgentRuntimeConfig::path(base, &id).is_file() {
        let _ = write_agent_config(base, &id, &name, id != DEFAULT_AGENT_ID);
    }

    Ok(workspace)
}

/// 首次启动时创建目录与核心模板（已存在的文件不覆盖）
///
/// 布局：
/// ```text
/// ~/.astro/
/// ├── workspace/                 ← 默认 Agent 工作区
/// ├── workspace-{id}/            ← 其他 Agent 工作区（同构）
/// ├── agents/{id}/config.json    ← 每 Agent 的模型/工具/MCP 配置
/// ├── skills/                    ← 公共技能（含 create-agent）
/// ├── sessions/
/// └── active-agent.json
/// ```
pub fn ensure_workspace(base: &Path) -> anyhow::Result<EnsureWorkspaceReport> {
    fs::create_dir_all(base)?;
    let _ = migrate_legacy_agent_workspaces(base);

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
        let legacy = base.join(name);
        if !dest.exists() {
            if legacy.is_file() {
                created_files.push(format!("workspace/{name} (migrated)"));
            } else {
                created_files.push(format!("workspace/{name}"));
            }
        }
    }
    let _ = ensure_agent_space(base, DEFAULT_AGENT_ID, Some("Astro"))?;

    // 初始化会话数据库（含旧 state.db / sessions.db 迁移）
    let _ = SessionStore::open_with_legacy_migration(&base.join("sessions"))?;

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

    // 沉淀公共 create-agent 技能
    if seed_create_agent_skill(base)? {
        created_files.push("skills/create-agent/SKILL.md".to_string());
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

const CREATE_AGENT_SKILL: &str = r#"---
name: create-agent
description: 根据用户填写的助手模板，创建新的 Agent 工作区（workspace-{id}）、写入 agents/{id}/config.json，并填充 AGENT/IDENTITY/SOUL/USER/MEMORY 等 md 文件。用户说「帮我创建一个助手」或点击「新建 Agent」时使用。
---

# 创建 Agent（记忆空间）

当用户用下面模板（或等价描述）要求新建助手时，执行本技能。

## 用户模板

```
帮我创建一个助手：名称是「」，背景经历是「」，说话风格是「」，主要帮我做「」，不要做「」，请称呼我为「」，我的偏好是「」
```

## 目录约定

| 路径 | 用途 |
|------|------|
| `~/.astro/workspace/` | 默认 Agent 工作区 |
| `~/.astro/workspace-{id}/` | 其他 Agent 工作区 |
| `~/.astro/agents/{id}/config.json` | 该 Agent 的模型 / 工具 / MCP 配置 |
| `~/.astro/skills/` | 公共技能（本技能所在） |
| `workspace-{id}/skills/` | 该 Agent 专属技能 |

## 步骤

1. **解析**模板里「」中的字段；空字段可追问，或先用合理默认再写入。
2. **调用工具** `create_agent`，传入：
   - `name`：名称
   - `activate`: false（默认不切换；需要立刻用新 Agent 时再传 true）
   - `inherit_config`: true（默认继承全局工具/MCP，可再改）
   - `profile`：background / style / focus / avoid / call_me / preferences
3. 工具会创建 `workspace-{id}/` 与 `agents/{id}/config.json`，并按 profile 填充各 md。
4. 若还需微调，用 `file_ops` 编辑对应 md（不要改错工作区）。
5. 用一两句话告诉用户：新 Agent 的 id、工作区路径、已切换为当前 Agent。

## 注意

- 不要覆盖默认 `workspace`。
- 配置（模型/工具/MCP）写在 `agents/{id}/`，工作区文件写在 `workspace-{id}/`。
- 专属技能放 `workspace-{id}/skills/`；公共技能继续用 `~/.astro/skills/`。
"#;

/// 将 create-agent 技能写入 `~/.astro/skills/`（已存在则不覆盖，便于用户定制）
pub fn seed_create_agent_skill(base: &Path) -> anyhow::Result<bool> {
    let dir = base.join("skills").join("create-agent");
    let path = dir.join("SKILL.md");
    if path.is_file() {
        return Ok(false);
    }
    fs::create_dir_all(&dir)?;
    fs::write(&path, CREATE_AGENT_SKILL)?;
    Ok(true)
}

/// 对默认目录执行 [`ensure_workspace`]
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
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn ensure_workspace_creates_core_layout() {
        let dir = TempDir::new().unwrap();
        let report = ensure_workspace(dir.path()).unwrap();

        let ws = dir.path().join("workspace");
        assert!(ws.is_dir());
        assert!(ws.join("mermaid").is_dir());
        assert!(ws.join("skills").is_dir());
        for rel in ENSURED_DIRS {
            assert!(dir.path().join(rel).is_dir(), "missing dir {rel}");
        }
        assert!(dir.path().join("sessions").join("state.db").is_file());
        // sessions.db 为旧库；新布局仅权威 state.db（旁路文件可保留备份）
        for (name, _) in CORE_FILES {
            assert!(ws.join(name).is_file(), "missing workspace/{name}");
            assert!(!dir.path().join(name).exists(), "{name} should not be at root");
        }
        assert!(dir.path().join("active-agent.json").is_file());
        assert_eq!(
            report.created_files.len(),
            CORE_FILES.len() + STATE_JSON_FILES.len() + 1 // + create-agent skill
        );
        assert!(report
            .created_files
            .iter()
            .any(|f| f == "skills/create-agent/SKILL.md"));
        assert!(report.created_files.iter().any(|f| f == "cron/jobs.json"));
        assert!(report.created_files.iter().any(|f| f == "models.json"));
        assert!(report.created_files.iter().any(|f| f == "skills-enabled.json"));
        assert!(report.created_files.iter().any(|f| f == "tools-enabled.json"));
        assert!(report.created_files.iter().any(|f| f == "mcp.json"));
        assert!(report.created_files.iter().any(|f| f == "active-agent.json"));
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
    fn ensure_workspace_migrates_legacy_root_files() {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("MEMORY.md"), "- legacy memory").unwrap();

        ensure_workspace(dir.path()).unwrap();
        let migrated = dir.path().join("workspace").join("MEMORY.md");
        assert!(migrated.is_file());
        assert_eq!(fs::read_to_string(&migrated).unwrap(), "- legacy memory");
        assert!(!dir.path().join("MEMORY.md").exists());
    }

    #[test]
    fn create_and_list_agents() {
        let dir = TempDir::new().unwrap();
        ensure_workspace(dir.path()).unwrap();

        let info = create_agent(dir.path(), "PPT Expert").unwrap();
        assert_eq!(info.id, "ppt-expert");
        assert!(info.path.ends_with("workspace-ppt-expert"));

        let agents = list_agents(dir.path());
        assert!(agents.iter().any(|a| a.is_default));
        assert!(agents.iter().any(|a| a.id == "ppt-expert"));

        let ws = PathBuf::from(&info.path);
        assert!(ws.join("AGENT.md").is_file());
        assert!(ws.join("MEMORY.md").is_file());
        assert!(ws.join("mermaid").is_dir());
        assert!(ws.join("skills").is_dir());
        assert!(dir.path().join("agents/ppt-expert/config.json").is_file());
        assert!(dir.path().join("skills/create-agent/SKILL.md").is_file());

        set_active_agent(dir.path(), &info.id).unwrap();
        assert_eq!(active_agent_id(dir.path()), info.id);
        assert_eq!(
            agent_workspace_dir(dir.path(), &active_agent_id(dir.path())),
            agent_workspace_dir(dir.path(), "ppt-expert")
        );
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
        let info =
            create_agent_with_profile(dir.path(), "演示专家", Some("demo-expert"), Some(&profile), true, true).unwrap();
        assert_eq!(info.id, "demo-expert");
        assert!(info.path.ends_with("workspace-demo-expert"));
        let ws = PathBuf::from(&info.path);
        let agent = fs::read_to_string(ws.join("AGENT.md")).unwrap();
        assert!(agent.contains("十年 PPT"));
        let user = fs::read_to_string(ws.join("USER.md")).unwrap();
        assert!(user.contains("老板"));
        assert_eq!(active_agent_id(dir.path()), info.id);
    }

    #[test]
    fn daily_memory_roundtrip() {
        let dir = TempDir::new().unwrap();
        ensure_workspace(dir.path()).unwrap();
        let ws = dir.path().join("workspace");
        let path = ensure_daily_memory(&ws, "2026-07-11").unwrap();
        assert!(path.is_file());
        assert_eq!(list_daily_memory_dates(&ws), vec!["2026-07-11".to_string()]);
    }
}
