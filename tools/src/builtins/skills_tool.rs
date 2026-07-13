//! Skills 工具：按名称加载已安装 Skill 的 `SKILL.md` 说明。
//!
//! 实际读取逻辑委托 [`skills::load_skill_by_name`]；本模块负责参数校验与结果拼装。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `skills` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct SkillsArgs {
    /// Skill 名称 / id（不可为空）。
    pub skill_id: String,
    /// 可选结构化输入，会附在返回文本的「调用输入」小节。
    #[serde(default)]
    pub input: Option<serde_json::Value>,
}

/// 向注册表登记 `skills` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "skills".to_string(),
        toolset: "skills".to_string(),
        description: "Load an installed skill by name (skill_id matches skill name) and return its SKILL.md text. Does not execute the skill—only returns instructions. Body capped at 64KiB."
            .to_string(),
        schema: schema_for_args::<SkillsArgs>(),
        check_fn: None,
        icon: "puzzle",
    });
}

/// 加载 Skill 内容，并附上调用输入 JSON（输入过长时截断）。
///
/// # 错误
/// 参数无效、`skill_id` 为空，或 Skill 不存在 / 读取失败。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: SkillsArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("skills 参数无效: {e}"))?;
    let skill_id = parsed.skill_id.trim();
    if skill_id.is_empty() {
        anyhow::bail!("skills 需要 skill_id");
    }

    let loaded = skills::load_skill_by_name(skill_id)?;
    let input = parsed.input.unwrap_or(serde_json::json!({}));
    let input_str = serde_json::to_string_pretty(&input).unwrap_or_default();
    let input_capped = if input_str.len() > 4 * 1024 {
        common::truncate_tool_result(&input_str, 4 * 1024)
    } else {
        input_str
    };
    let body = format!(
        "# Skill: {}\n\n{}\n\n## 调用输入\n{}",
        loaded.metadata.name, loaded.content, input_capped
    );
    Ok(common::truncate_tool_result(
        &body,
        common::MAX_TOOL_RESULT_BYTES,
    ))
}
