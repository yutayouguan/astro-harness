//! 创建持久人设：在 `~/.astro` 下新建独立记忆空间与配置。
//!
//! 调用 [`home::create_agent_with_profile`]；若 `activate`，会就地更新
//! [`ToolContext`] 的工作区与 MemoryManager，便于后续写到新空间。
//!
//! **禁止**用本工具拆解当前回合任务——这类任务应启动 Agent Thread。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Persona / preference profile fields for AGENT / IDENTITY / SOUL / USER templates.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct AgentProfileArgs {
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub style: Option<String>,
    /// Primary ways to help the user.
    #[serde(default)]
    pub focus: Option<String>,
    #[serde(default)]
    pub avoid: Option<String>,
    /// How to address the user.
    #[serde(default)]
    pub call_me: Option<String>,
    #[serde(default)]
    pub preferences: Option<String>,
}

/// Arguments for the `persona_create` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct PersonaCreateArgs {
    /// Display name (any language; may collide; decoupled from immutable id).
    pub name: String,
    /// Optional explicit id (advanced / tests). Default `{slug}--{hex12}` from name.
    #[serde(default)]
    pub id: Option<String>,
    /// Switch to this agent after create (updates ASTRO_WORKSPACE / MEMORY); default `false`.
    #[serde(default)]
    pub activate: Option<bool>,
    /// Copy global tool gates into `agents/{id}/config.json` as a starting point; default `true`.
    #[serde(default)]
    pub inherit_config: Option<bool>,
    #[serde(default)]
    pub profile: Option<AgentProfileArgs>,
}

/// 向注册表登记 `persona_create`。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "persona_create".to_string(),
        toolset: "persona".to_string(),
        description:
            "Create a durable Agent persona with persistent workspace (MEMORY/IDENTITY/SOUL). \
FORBIDDEN for in-turn task splitting—use spawn_agent to create an Agent Thread. \
Prefer after loading the create-agent skill."
                .to_string(),
        schema: schema_for_args::<PersonaCreateArgs>(),
        check_fn: None,
        icon: "user-plus",
        ..ToolEntry::lifecycle_defaults().exclusive()
    });
}

// 单专家模式不再把 persona_create 暴露给模型；保留实现仅供旧数据迁移工具复用。

/// 将可选字符串规范为空串（缺省或仅空白视为空）。
fn opt_str(v: &Option<String>) -> String {
    v.as_deref().unwrap_or("").trim().to_string()
}

/// 创建 Agent 空间；若激活则刷新当前 [`ToolContext`] 工作区与记忆管理器。
///
/// # 错误
/// 缺少 `name`、参数无效，或底层 `create_agent_with_profile` 失败。
pub fn dispatch(ctx: &mut ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: PersonaCreateArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("persona_create 参数无效: {e}"))?;
    let name = parsed.name.trim();
    if name.is_empty() {
        anyhow::bail!("缺少 name 参数");
    }

    let id_override = parsed
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let activate = parsed.activate.unwrap_or(false);
    let inherit_config = parsed.inherit_config.unwrap_or(true);

    let profile = parsed.profile.as_ref().map(|p| home::AgentProfile {
        background: opt_str(&p.background),
        style: opt_str(&p.style),
        focus: opt_str(&p.focus),
        avoid: opt_str(&p.avoid),
        call_me: opt_str(&p.call_me),
        preferences: opt_str(&p.preferences),
    });

    let base = ctx.memory_dir.clone();
    let info = home::create_agent_with_profile(
        &base,
        name,
        id_override,
        profile.as_ref(),
        inherit_config,
        activate,
    )?;

    // 若已切换，更新当前 ToolContext 工作区，便于后续写到新空间
    if activate {
        ctx.workspace_dir = std::path::PathBuf::from(&info.path);
        skills::set_workspace_override(std::path::Path::new(&info.path));
        if let Ok(mgr) = memory::MemoryManager::for_agent(base.clone(), &info.id) {
            *ctx.memory_mut() = mgr;
        }
    }

    Ok(format!(
        "已创建 Agent「{name}」\n- id: {id}\n- 工作区: {path}\n- 配置: {cfg}\n- 已激活: {active}\n- 图标: {icon}\n\n可用 apply_patch 继续微调 AGENT.md / IDENTITY.md / SOUL.md / USER.md / MEMORY.md。",
        name = info.name,
        id = info.id,
        path = info.path,
        cfg = home::agent_config_dir(&base, &info.id)
            .join("config.json")
            .display(),
        active = activate,
        icon = info
            .emoji
            .as_deref()
            .map(|p| format!("已自动选择 Lucide（{p}）"))
            .unwrap_or_else(|| "未设置".into()),
    ))
}
