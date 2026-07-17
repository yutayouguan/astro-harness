//! 澄清工具：任务含糊时向用户提出选项问题。
//!
//! 返回带 `astro_hitl` 标记的 A2UI JSON；由 streaming 层经 `HitlGate` 同回合 park，
//! 用户提交后写入标准 tool result 并续跑（不再结束 run）。
//!
//! 统一走叠层 Tab 向导 `ClarifyWizard`（仅 `questions`）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// 澄清中的一步。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ClarifyQuestion {
    /// 答案键；缺省时用 `q0` / `q1` …
    #[serde(default)]
    pub id: Option<String>,
    /// 向用户提出的问题。
    pub question: String,
    /// 预设选项；留空则前端仅展示自由输入框。
    #[serde(default)]
    pub options: Vec<String>,
}

/// `clarify` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ClarifyArgs {
    /// 澄清步骤列表；1 题也走同一向导。
    #[serde(default)]
    pub questions: Vec<ClarifyQuestion>,
    /// 向导标题；缺省：多题「请确认几项」，单题用问题正文。
    #[serde(default)]
    pub title: Option<String>,
}

/// 向注册表登记 `clarify` 工具（工具集 id 同名）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "clarify".to_string(),
        toolset: "clarify".to_string(),
        description: "Ask clarifying question(s) before proceeding. \
Use `questions` (1+ steps). Each step may include `options`; empty options show a free-text field. \
When options are provided, the UI always adds a custom text input as the last choice."
            .to_string(),
        schema: schema_for_args::<ClarifyArgs>(),
        check_fn: None,
        icon: "circle-help",
        needs_confirmation: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["clarify"],
    sync_ctx: dispatch,
}

fn normalize_options(raw: Vec<String>) -> Vec<String> {
    raw.into_iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect()
}

/// 构建 HITL clarify 载荷（A2UI operations + response schema）。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ClarifyArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("clarify 参数无效: {e}"))?;

    let steps: Vec<a2ui::templates::ClarifyStep> = parsed
        .questions
        .iter()
        .enumerate()
        .filter_map(|(i, q)| {
            let question = q.question.trim();
            if question.is_empty() {
                return None;
            }
            let id = q
                .id
                .as_ref()
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| format!("q{i}"));
            Some(a2ui::templates::ClarifyStep {
                id,
                question: question.to_string(),
                options: normalize_options(q.options.clone()),
            })
        })
        .collect();

    if steps.is_empty() {
        anyhow::bail!("clarify 需要至少一个 question");
    }

    let title = parsed
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| {
            if steps.len() == 1 {
                steps[0].question.clone()
            } else {
                "请确认几项".into()
            }
        });

    let surface_id = format!("clarify-{}", Uuid::new_v4());
    let operations = a2ui::templates::build_clarify_surface(&surface_id, &title, &steps);
    a2ui::validate_operations(&operations)
        .map_err(|e| anyhow::anyhow!("clarify A2UI 无效: {e}"))?;

    let message = if steps.len() == 1 {
        steps[0].question.clone()
    } else {
        format!("{}（{} 项）", title, steps.len())
    };

    let payload = json!({
        "astro_hitl": true,
        "reason": "input_required",
        "message": message,
        "operations": operations,
        "response_schema": {
            "type": "object",
            "properties": {
                "answers": { "type": "object" },
                "value": { "type": "string" }
            },
            "required": ["answers", "value"]
        }
    });
    Ok(payload.to_string())
}
