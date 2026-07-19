//! 统一人机交互工具 `ask`：向用户提问（question）或请求批准（confirm）。
//!
//! 合并原 `clarify` + `confirm`。返回带 `astro_hitl` 标记的 A2UI JSON，由 streaming
//! 层经 `HitlGate` 同回合 park；用户提交后写入标准 tool result 并续跑（不结束 run）。
//!
//! - `mode=question`（默认）：`questions` 数组 → 叠层 Tab 向导 `ClarifyWizard`；
//!   reason `input_required`，response schema `{answers, value}`。
//! - `mode=confirm`：`title`+`body` → 批准/拒绝卡；reason `confirmation`，
//!   response schema `{approved}`。
//!
//! 保留两套 reason / response schema 以对齐 streaming park、interrupt 校验与
//! 危险命令网关；`ask` 仅作为统一入口按 mode 分派。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// question 模式下的一步提问。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AskQuestion {
    /// 答案键；缺省时用 `q0` / `q1` …
    #[serde(default)]
    pub id: Option<String>,
    /// 向用户提出的问题。
    pub question: String,
    /// 预设选项；留空则前端仅展示自由输入框。
    #[serde(default)]
    pub options: Vec<String>,
}

/// `ask` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AskArgs {
    /// 交互模式：`question`（向用户提问，默认）| `confirm`（敏感操作前请求批准）。
    /// 省略时按参数推断：给了 `questions` → question；只给了 `body` → confirm。
    #[serde(default)]
    pub mode: Option<String>,
    /// question 模式：提问步骤（1+）。每步可带 `options`；`options` 为空则只显示自由输入框。
    #[serde(default)]
    pub questions: Vec<AskQuestion>,
    /// 标题：question 向导标题 / confirm 确认卡标题。
    #[serde(default)]
    pub title: Option<String>,
    /// confirm 模式：确认卡正文说明。
    #[serde(default)]
    pub body: Option<String>,
}

/// 向注册表登记 `ask` 工具（工具集 id 同名）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "ask".to_string(),
        toolset: "ask".to_string(),
        description: "Ask the user before proceeding. Two modes: \
(1) question — set `questions` (1+ steps; each may include `options`, empty options show a free-text field) to clarify unclear requirements; \
(2) confirm — set mode=\"confirm\" with `title`+`body` to request approval of a sensitive or irreversible action. \
Prefer asking over guessing when requirements are ambiguous or key info is missing."
            .to_string(),
        schema: schema_for_args::<AskArgs>(),
        check_fn: None,
        icon: "circle-help",
        needs_confirmation: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["ask"],
    sync_ctx: dispatch,
}

fn normalize_options(raw: Vec<String>) -> Vec<String> {
    raw.into_iter()
        .map(|o| o.trim().to_string())
        .filter(|o| !o.is_empty())
        .collect()
}

/// 按 `mode`（或参数推断）分派到 question / confirm 载荷构建。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: AskArgs =
        serde_json::from_value(args.clone()).map_err(|e| anyhow::anyhow!("ask 参数无效: {e}"))?;

    let mode = parsed.mode.as_deref().map(|s| s.trim().to_lowercase());
    let is_confirm = match mode.as_deref() {
        Some("confirm") => true,
        Some("question") => false,
        _ => parsed.questions.is_empty() && parsed.body.is_some(),
    };

    if is_confirm {
        build_confirm(&parsed)
    } else {
        build_question(&parsed)
    }
}

/// confirm 模式：批准/拒绝卡（reason=confirmation, schema={approved}）。
fn build_confirm(parsed: &AskArgs) -> anyhow::Result<String> {
    let title = parsed.title.as_deref().unwrap_or("").trim();
    let body = parsed.body.as_deref().unwrap_or("").trim();
    if title.is_empty() {
        anyhow::bail!("ask confirm 模式需要 title");
    }
    if body.is_empty() {
        anyhow::bail!("ask confirm 模式需要 body");
    }

    let surface_id = format!("confirm-{}", Uuid::new_v4());
    let operations = a2ui::templates::build_confirm_surface(&surface_id, title, body);
    a2ui::validate_operations(&operations).map_err(|e| anyhow::anyhow!("ask A2UI 无效: {e}"))?;

    let payload = json!({
        "astro_hitl": true,
        "reason": "confirmation",
        "message": title,
        "operations": operations,
        "response_schema": {
            "type": "object",
            "properties": { "approved": { "type": "boolean" } },
            "required": ["approved"]
        }
    });
    Ok(payload.to_string())
}

/// question 模式：澄清向导（reason=input_required, schema={answers, value}）。
fn build_question(parsed: &AskArgs) -> anyhow::Result<String> {
    let steps: Vec<a2ui::templates::ClarifyStep> = parsed
        .questions
        .iter()
        .enumerate()
        .filter_map(|(i, q)| {
            let question = q.question.trim();
            if question.is_empty() {
                return None;
            }
            let id =
                q.id.as_ref()
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
        anyhow::bail!("ask question 模式需要至少一个 question");
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
    a2ui::validate_operations(&operations).map_err(|e| anyhow::anyhow!("ask A2UI 无效: {e}"))?;

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
