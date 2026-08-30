//! 插件安装请求：通过 Astro 的 SkillHub 安装器安装指定 Skill。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `request_plugin_install` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct RequestPluginInstallArgs {
    /// SkillHub 技能标识符（例如 `owner/slug` 或 `skillhub:owner/slug`）。
    pub skill_id: String,
    /// 商店提供的安装引用；省略时从 `skill_id` 构造。
    #[serde(default)]
    pub install_ref: Option<String>,
    /// 展示名称，用于安装来源记录。
    #[serde(default)]
    pub name: Option<String>,
    /// 本地目录名，用于安装来源记录。
    #[serde(default)]
    pub folder: Option<String>,
    /// 安装作用域：`global`（`~/.astro/skills`）或 `project`（`<project>/.astro/skills`）。
    #[serde(default)]
    pub scope: Option<String>,
    /// 商店是否标记该 Skill 需要 API Key。
    #[serde(default)]
    pub requires_api_key: bool,
    /// 请求安装的可选理由。
    #[serde(default)]
    pub reason: Option<String>,
}

/// 向注册表注册 `request_plugin_install` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "request_plugin_install".to_string(),
        toolset: "system".to_string(),
        description: "Install a SkillHub Skill through Astro's native installer. Use scope=global for ~/.astro/skills or scope=project for <project>/.astro/skills. Never run the SkillHub CLI for this action.".to_string(),
        schema: schema_for_args::<RequestPluginInstallArgs>(),
        check_fn: None,
        icon: "download",
        ..ToolEntry::lifecycle_defaults()
            .with_confirmation()
            .deferred()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["request_plugin_install"],
    async_ctx: dispatch,
    args: RequestPluginInstallArgs,
}

fn normalized_install_ref(args: &RequestPluginInstallArgs) -> anyhow::Result<String> {
    let candidate = args
        .install_ref
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| args.skill_id.trim());
    if candidate.is_empty() {
        anyhow::bail!("request_plugin_install 需要 skill_id 或 install_ref");
    }
    if candidate.starts_with("skillhub:")
        || candidate.starts_with("https://skillhub.cn/")
        || candidate.starts_with("https://www.skillhub.cn/")
        || candidate.starts_with("https://api.skillhub.cn/")
    {
        Ok(candidate.to_string())
    } else {
        Ok(format!("skillhub:{candidate}"))
    }
}

fn normalized_scope(scope: Option<&str>) -> anyhow::Result<&'static str> {
    match scope.map(str::trim).filter(|value| !value.is_empty()) {
        None | Some("global") => Ok("global"),
        Some("project") => Ok("project"),
        Some(other) => anyhow::bail!("不支持的 Skill 安装作用域: {other}"),
    }
}

/// 通过与桌面一键安装相同的 SkillHub API 安装器执行安装。
pub async fn dispatch(
    ctx: &ToolContext<'_>,
    args: &RequestPluginInstallArgs,
) -> anyhow::Result<String> {
    let install_ref = normalized_install_ref(args)?;
    let scope = normalized_scope(args.scope.as_deref())?;
    let agent_id = ctx.agent_id();
    let project_root = ctx.project_root.clone();
    let result = skills::install_from_ref_scoped(
        &install_ref,
        Some(&agent_id),
        Some(skills::InstallOriginHint {
            name: args.name.clone(),
            folder: args.folder.clone(),
        }),
        scope,
        project_root.as_deref(),
    )
    .await?;

    if args.requires_api_key {
        Ok(format!(
            "{result}\n状态: 已安装，待配置 API Key。请读取已安装的 SKILL.md 确认准确的凭据名称；不要要求用户在对话中粘贴密钥，也不要把密钥写入 Skill 或项目文件。"
        ))
    } else {
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(skill_id: &str) -> RequestPluginInstallArgs {
        RequestPluginInstallArgs {
            skill_id: skill_id.into(),
            install_ref: None,
            name: None,
            folder: None,
            scope: None,
            requires_api_key: false,
            reason: None,
        }
    }

    #[test]
    fn normalizes_skillhub_reference_and_scope() {
        assert_eq!(
            normalized_install_ref(&args("owner/demo")).unwrap(),
            "skillhub:owner/demo"
        );
        assert_eq!(normalized_scope(None).unwrap(), "global");
        assert_eq!(normalized_scope(Some("project")).unwrap(), "project");
        assert!(normalized_scope(Some("machine")).is_err());
    }

    #[test]
    fn registers_as_confirmation_gated_deferred_tool() {
        let mut registry = ToolRegistry::new();
        register(&mut registry);
        let entry = registry
            .get("request_plugin_install")
            .expect("request_plugin_install registered");
        assert!(entry.needs_confirmation);
        assert_eq!(entry.exposure, types::ToolExposure::Deferred);
        let properties = entry.schema["properties"]
            .as_object()
            .expect("object properties");
        for field in [
            "skill_id",
            "install_ref",
            "folder",
            "scope",
            "requires_api_key",
        ] {
            assert!(
                properties.contains_key(field),
                "missing schema field {field}"
            );
        }
    }
}
