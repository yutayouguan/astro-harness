//! 澄清工具：任务含糊时向用户提出选项问题。
//!
//! 返回带 `astro_hitl` 标记的 A2UI JSON；由 streaming 层经 `HitlGate` 同回合 park，
//! 用户提交后写入标准 tool result 并续跑（不再结束 run）。
//!
//! - 单题：`question` + `options` → ChoicePicker 卡
//! - 多题：`questions[]`（≥2）→ 叠层 Tab 向导 `ClarifyWizard`

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// 多题澄清中的一步。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ClarifyQuestion {
    /// 答案键；缺省时用 `q0` / `q1` …
    #[serde(default)]
    pub id: Option<String>,
    /// 向用户提出的问题。
    pub question: String,
    /// 可选选项列表。
    #[serde(default)]
    pub options: Vec<String>,
}

/// `clarify` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct ClarifyArgs {
    /// 单题模式：向用户提出的澄清问题。
    #[serde(default)]
    pub question: Option<String>,
    /// 单题模式选项；为空时提供「继续」单按钮。
    #[serde(default)]
    pub options: Vec<String>,
    /// 多题模式：一卡多问（≥2 时启用叠层 Tab 向导）。
    #[serde(default)]
    pub questions: Vec<ClarifyQuestion>,
    /// 多题向导标题；缺省为「请确认几项」。
    #[serde(default)]
    pub title: Option<String>,
}

/// 向注册表登记 `clarify` 工具（工具集 id 同名）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "clarify".to_string(),
        toolset: "clarify".to_string(),
        description: "Ask clarifying question(s) with choices before proceeding. \
Use `questions` (2+) for a stacked multi-step wizard; or `question`+`options` for a single picker."
            .to_string(),
        schema: schema_for_args::<ClarifyArgs>(),
        check_fn: None,
        icon: "circle-help",
        needs_confirmation: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

fn normalize_options(raw: Vec<String>, fallback: &str) -> Vec<String> {
    let options: Vec<String> = raw
        .into_iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect();
    if options.is_empty() {
        vec![fallback.into()]
    } else {
        options
    }
}

/// 构建 HITL clarify 载荷（A2UI operations + response schema）。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: ClarifyArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("clarify 参数无效: {e}"))?;

    let multi_steps: Vec<a2ui::templates::ClarifyStep> = parsed
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
                options: normalize_options(q.options.clone(), "继续"),
            })
        })
        .collect();

    let surface_id = format!("clarify-{}", Uuid::new_v4());

    if multi_steps.len() >= 2 {
        let title = parsed
            .title
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("请确认几项");
        let operations =
            a2ui::templates::build_multi_clarify_surface(&surface_id, title, &multi_steps);
        a2ui::validate_operations(&operations)
            .map_err(|e| anyhow::anyhow!("clarify A2UI 无效: {e}"))?;

        let message = format!(
            "{}（{} 项）",
            title,
            multi_steps.len()
        );
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
        return Ok(payload.to_string());
    }

    // Single-question path (legacy + questions[0] fallback).
    let (question, options) = if let Some(step) = multi_steps.into_iter().next() {
        (step.question, step.options)
    } else {
        let question = parsed
            .question
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("clarify 需要 question 或 questions"))?;
        (
            question.to_string(),
            normalize_options(parsed.options, "继续"),
        )
    };

    let operations = a2ui::templates::build_clarify_surface(&surface_id, &question, &options);
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
