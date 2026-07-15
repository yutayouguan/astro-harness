//! 澄清工具：任务含糊时向用户提出选项问题。
//!
//! 返回带 `astro_hitl` 标记的 A2UI JSON；由 streaming 层经 `HitlGate` 同回合 park，
//! 用户提交后写入标准 tool result 并续跑（不再结束 run）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `clarify` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ClarifyArgs {
    /// 向用户提出的澄清问题（不可为空）。
    pub question: String,
    /// 可选选项列表；为空时提供「继续」单按钮。
    #[serde(default)]
    pub options: Vec<String>,
}

/// 向注册表登记 `clarify` 工具（工具集 id 同名）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "clarify".to_string(),
        toolset: "clarify".to_string(),
        description: "Ask the user a clarifying question with optional choices before proceeding."
            .to_string(),
        schema: schema_for_args::<ClarifyArgs>(),
        check_fn: None,
        icon: "circle-help",
    });
}

/// 构建 HITL clarify 载荷（A2UI operations + response schema）。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ClarifyArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("clarify 参数无效: {e}"))?;
    let question = parsed.question.trim();
    if question.is_empty() {
        anyhow::bail!("clarify 需要 question");
    }

    let options: Vec<String> = if parsed.options.is_empty() {
        vec!["继续".into()]
    } else {
        parsed
            .options
            .into_iter()
            .map(|o| o.trim().to_string())
            .filter(|o| !o.is_empty())
            .collect()
    };
    if options.is_empty() {
        anyhow::bail!("clarify 需要至少一个非空 option");
    }

    let surface_id = format!("clarify-{}", Uuid::new_v4());
    let operations = a2ui::templates::build_clarify_surface(&surface_id, question, &options);
    a2ui::validate_operations(&operations)
        .map_err(|e| anyhow::anyhow!("clarify A2UI 无效: {e}"))?;

    let payload = json!({
        "astro_hitl": true,
        "reason": "input_required",
        "message": question,
        "operations": operations,
        "response_schema": {
            "type": "object",
            "properties": { "value": { "type": "string" } },
            "required": ["value"]
        }
    });
    Ok(payload.to_string())
}
