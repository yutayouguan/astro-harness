//! 创建 Agent 工具：在 `~/.astro` 下新建独立记忆空间与配置。
//!
//! 调用 [`memory::create_agent_with_profile`]；若 `activate`，会就地更新
//! [`ToolContext`] 的工作区与 MemoryManager，便于后续 file_ops 写到新空间。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 人格 / 偏好档案字段，用于填充 AGENT / IDENTITY / SOUL / USER 等模板。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema, Default)]
pub struct AgentProfileArgs {
    /// 背景经历。
    #[serde(default)]
    pub background: Option<String>,
    /// 说话风格。
    #[serde(default)]
    pub style: Option<String>,
    /// 主要帮用户做的事。
    #[serde(default)]
    pub focus: Option<String>,
    /// 明确不要做的事。
    #[serde(default)]
    pub avoid: Option<String>,
    /// 请如何称呼用户。
    #[serde(default)]
    pub call_me: Option<String>,
    /// 其他偏好。
    #[serde(default)]
    pub preferences: Option<String>,
}

/// `create_agent` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CreateAgentArgs {
    /// 助手显示名；若未给 `id` 则用于派生 workspace id。
    pub name: String,
    /// 可选 ASCII slug（`workspace-{id}`）；缺省由 name 生成。
    #[serde(default)]
    pub id: Option<String>,
    /// 创建后是否切换为当前 Agent；缺省 `true`。
    #[serde(default)]
    pub activate: Option<bool>,
    /// 是否把全局 tools/MCP 拷入 `agents/{id}/config.json` 作起点；缺省 `true`。
    #[serde(default)]
    pub inherit_config: Option<bool>,
    /// 可选人格档案。
    #[serde(default)]
    pub profile: Option<AgentProfileArgs>,
}

/// 向注册表登记 `create_agent`（归入 `multi_agent` 工具集）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "create_agent".to_string(),
        toolset: "multi_agent".to_string(),
        description: "Create a new Agent memory space at ~/.astro/workspace-{id}/ with agents/{id}/config.json, and optionally fill AGENT/IDENTITY/SOUL/USER/MEMORY from a profile. Use after loading the create-agent skill.".to_string(),
        schema: schema_for_args::<CreateAgentArgs>(),
        check_fn: None,
        icon: "bot",
    });
}

/// 将可选字符串规范为空串（缺省或仅空白视为空）。
fn opt_str(v: &Option<String>) -> String {
    v.as_deref().unwrap_or("").trim().to_string()
}

/// 创建 Agent 空间；若激活则刷新当前 [`ToolContext`] 工作区与记忆管理器。
///
/// # 错误
/// 缺少 `name`、参数无效，或底层 `create_agent_with_profile` 失败。
pub fn dispatch(ctx: &mut ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: CreateAgentArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("create_agent 参数无效: {e}"))?;
    let name = parsed.name.trim();
    if name.is_empty() {
        anyhow::bail!("缺少 name 参数");
    }

    let id_override = parsed
        .id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty());

    let activate = parsed.activate.unwrap_or(true);
    let inherit_config = parsed.inherit_config.unwrap_or(true);

    let profile = parsed.profile.as_ref().map(|p| memory::AgentProfile {
        background: opt_str(&p.background),
        style: opt_str(&p.style),
        focus: opt_str(&p.focus),
        avoid: opt_str(&p.avoid),
        call_me: opt_str(&p.call_me),
        preferences: opt_str(&p.preferences),
    });

    let base = ctx.memory_dir.clone();
    let info = memory::create_agent_with_profile(
        &base,
        name,
        id_override,
        profile.as_ref(),
        inherit_config,
        activate,
    )?;

    // 若已切换，更新当前 ToolContext 工作区，便于后续 file_ops 写到新空间
    if activate {
        ctx.workspace_dir = std::path::PathBuf::from(&info.path);
        std::env::set_var("ASTRO_WORKSPACE", &info.path);
        if let Ok(mgr) = memory::MemoryManager::for_agent(base.clone(), &info.id) {
            *ctx.memory = mgr;
        }
    }

    Ok(format!(
        "已创建 Agent「{name}」\n- id: {id}\n- 工作区: {path}\n- 配置: {cfg}\n- 已激活: {active}\n\n可用 file_ops 继续微调 AGENT.md / IDENTITY.md / SOUL.md / USER.md / MEMORY.md。",
        name = info.name,
        id = info.id,
        path = info.path,
        cfg = memory::agent_config_dir(&base, &info.id)
            .join("config.json")
            .display(),
        active = activate,
    ))
}
