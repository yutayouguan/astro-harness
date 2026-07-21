//! 统一人机交互工具 `ask`：向用户提问（question）或请求批准（confirm）。
//!
//! 合并原 `clarify` + `confirm`。返回带 `astro_hitl` 标记的 A2UI JSON，由 streaming
//! 层经 `HitlGate` 同回合 park；用户提交后写入标准 tool result 并续跑（不结束 run）。
//!
//! - `mode=question`（默认推断）：`questions` 数组 → 叠层 Tab 向导；
//!   reason `input_required`，response schema `{answers, value}`。
//! - `mode=confirm`：`title`+`body` → 批准/拒绝卡；reason `confirmation`，
//!   response schema `{approved}`。
//!
//! 模式解析：显式 `mode` 优先；省略时仅允许「纯 questions」或「纯 title+body」，
//! 混传或两者皆空则报错（不再静默猜测）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

/// `ask` 模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AskMode {
    /// 澄清 / 收集输入（ClarifyWizard）。
    Question,
    /// 敏感操作批准 / 拒绝。
    Confirm,
}

/// One question step in `question` mode.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AskQuestion {
    /// Answer key; defaults to `q0` / `q1` …
    #[serde(default)]
    pub id: Option<String>,
    pub question: String,
    /// Preset options; empty means free-text input only.
    #[serde(default)]
    pub options: Vec<String>,
}

/// Arguments for the `ask` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AskArgs {
    /// `question` | `confirm`. Omit only when args are unambiguous (see tool description).
    #[serde(default)]
    pub mode: Option<AskMode>,
    /// Question mode: one or more steps. Each may include `options`; empty options = free text.
    #[serde(default)]
    pub questions: Vec<AskQuestion>,
    /// Wizard title (question) or confirmation card title (confirm).
    #[serde(default)]
    pub title: Option<String>,
    /// Confirm mode only: body text for the confirmation card.
    #[serde(default)]
    pub body: Option<String>,
}

/// 向注册表登记 `ask` 工具（工具集 id 同名）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "ask".to_string(),
        toolset: "ask".to_string(),
        description: "Ask the user before proceeding (same-turn HITL park). \
Modes: question — `questions` (1+ steps; optional `options`, empty = free text) to clarify requirements; \
confirm — mode=\"confirm\" with `title`+`body` to approve a sensitive/irreversible action. \
Omit mode only when unambiguous: questions only → question; title+body only → confirm. \
Do not mix questions with body. \
Not for Agent↔Plan switching (use request_mode_switch) or GPS/city for local weather/nearby (use request_user_location). \
Prefer asking over guessing when requirements are ambiguous."
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

fn has_usable_questions(questions: &[AskQuestion]) -> bool {
    questions.iter().any(|q| !q.question.trim().is_empty())
}

fn has_body(parsed: &AskArgs) -> bool {
    parsed
        .body
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
}

/// Resolve question vs confirm; explicit `mode` wins; omitted mode must be unambiguous.
pub(crate) fn resolve_ask_mode(parsed: &AskArgs) -> anyhow::Result<AskMode> {
    let has_q = has_usable_questions(&parsed.questions);
    let has_b = has_body(parsed);

    match parsed.mode {
        Some(AskMode::Confirm) => {
            if has_q {
                anyhow::bail!(
                    "ask confirm mode must not include questions; use mode=\"question\" or drop questions"
                );
            }
            Ok(AskMode::Confirm)
        }
        Some(AskMode::Question) => {
            if has_b {
                anyhow::bail!(
                    "ask question mode must not include body; use mode=\"confirm\" for approvals"
                );
            }
            Ok(AskMode::Question)
        }
        None => {
            if has_q && has_b {
                anyhow::bail!(
                    "ask: do not mix questions and body; set mode=\"question\" or mode=\"confirm\""
                );
            }
            if has_q {
                return Ok(AskMode::Question);
            }
            if has_b {
                return Ok(AskMode::Confirm);
            }
            anyhow::bail!(
                "ask: set mode=\"question\" with questions, or mode=\"confirm\" with title+body"
            );
        }
    }
}

/// 按 `mode`（或严格推断）分派到 question / confirm 载荷构建。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: AskArgs =
        serde_json::from_value(args.clone()).map_err(|e| anyhow::anyhow!("ask 参数无效: {e}"))?;

    match resolve_ask_mode(&parsed)? {
        AskMode::Confirm => build_confirm(&parsed),
        AskMode::Question => build_question(&parsed),
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
                "answers": {
                    "type": "object",
                    "description": "Map of step id → user answer"
                },
                "value": {
                    "type": "string",
                    "description": "Single-step answer or multi-step summary string"
                }
            },
            "required": ["answers", "value"]
        }
    });
    Ok(payload.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: serde_json::Value) -> AskArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn omit_mode_questions_only_is_question() {
        let a = args(json!({
            "questions": [{ "question": "Which env?" }]
        }));
        assert_eq!(resolve_ask_mode(&a).unwrap(), AskMode::Question);
    }

    #[test]
    fn omit_mode_body_only_is_confirm() {
        let a = args(json!({
            "title": "Delete?",
            "body": "Really?"
        }));
        assert_eq!(resolve_ask_mode(&a).unwrap(), AskMode::Confirm);
    }

    #[test]
    fn omit_mode_mix_errors() {
        let a = args(json!({
            "questions": [{ "question": "x" }],
            "body": "y"
        }));
        let err = resolve_ask_mode(&a).unwrap_err().to_string();
        assert!(err.contains("mix"), "{err}");
    }

    #[test]
    fn omit_mode_empty_errors() {
        let a = args(json!({ "title": "only title" }));
        let err = resolve_ask_mode(&a).unwrap_err().to_string();
        assert!(err.contains("mode="), "{err}");
    }

    #[test]
    fn explicit_confirm_rejects_questions() {
        let a = args(json!({
            "mode": "confirm",
            "title": "t",
            "body": "b",
            "questions": [{ "question": "x" }]
        }));
        assert!(resolve_ask_mode(&a)
            .unwrap_err()
            .to_string()
            .contains("confirm"));
    }

    #[test]
    fn explicit_question_rejects_body() {
        let a = args(json!({
            "mode": "question",
            "questions": [{ "question": "x" }],
            "body": "b"
        }));
        assert!(resolve_ask_mode(&a)
            .unwrap_err()
            .to_string()
            .contains("question"));
    }
}
