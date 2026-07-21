//! 统一人机交互工具 `ask_user`：提问 / 批准 / 定位。
//!
//! 合并原 `ask`（question+confirm）与 `request_user_location`。返回带 `astro_hitl`
//! 标记的 A2UI JSON，由 streaming 层经 `HitlGate` 同回合 park。
//!
//! - `mode=question`：`questions` → ClarifyWizard；reason `input_required`
//! - `mode=confirm`：`title`+`body` → 批准卡；reason `confirmation`
//! - `mode=location`：可选 `message` → 定位卡；reason `location_required`
//!
//! Agent↔Plan 切换仍用独立工具 `switch_mode`（不同管线）。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;
use uuid::Uuid;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;

const DEFAULT_LOCATION_MESSAGE: &str =
    "查询本地天气或附近信息需要你的位置。请授权共享当前位置，或手动填写城市。";

/// `ask_user` 模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum AskUserMode {
    /// 澄清 / 收集输入（ClarifyWizard）。
    Question,
    /// 敏感操作批准 / 拒绝。
    Confirm,
    /// 请求 GPS 或城市（本地天气 / 附近）。
    Location,
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

/// Arguments for the `ask_user` tool.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct AskUserArgs {
    /// `question` | `confirm` | `location`. Omit only when unambiguous (question vs confirm).
    #[serde(default)]
    pub mode: Option<AskUserMode>,
    /// Question mode: one or more steps.
    #[serde(default)]
    pub questions: Vec<AskQuestion>,
    /// Wizard title (question) or confirmation card title (confirm).
    #[serde(default)]
    pub title: Option<String>,
    /// Confirm mode only: body text.
    #[serde(default)]
    pub body: Option<String>,
    /// Location mode: why location is needed (optional; default copy when empty).
    #[serde(default)]
    pub message: Option<String>,
}

/// 向注册表登记 `ask_user`（toolset 同名）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: "ask_user".to_string(),
        toolset: "ask_user".to_string(),
        description: "Ask the user before proceeding (same-turn HITL park). Named ask_user to avoid \
confusion with chat interaction mode \"ask\". \
Modes: question — `questions` (1+ steps; optional `options`) to clarify; \
confirm — mode=\"confirm\" with `title`+`body` for sensitive/irreversible actions; \
location — mode=\"location\" (optional `message`) before local weather/nearby; never assume a city. \
Omit mode only when unambiguous: questions only → question; title+body only → confirm. \
Location always requires mode=\"location\". Do not mix questions with body. \
Not for Agent↔Plan switching (use switch_mode). Prefer asking over guessing."
            .to_string(),
        schema: schema_for_args::<AskUserArgs>(),
        check_fn: None,
        icon: "circle-help",
        needs_confirmation: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["ask_user"],
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

fn has_body(parsed: &AskUserArgs) -> bool {
    parsed
        .body
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
}

fn has_location_hint(parsed: &AskUserArgs) -> bool {
    parsed
        .message
        .as_deref()
        .map(str::trim)
        .is_some_and(|s| !s.is_empty())
}

/// Resolve mode; explicit `mode` wins; omitted mode must be unambiguous (never infers location).
pub(crate) fn resolve_ask_user_mode(parsed: &AskUserArgs) -> anyhow::Result<AskUserMode> {
    let has_q = has_usable_questions(&parsed.questions);
    let has_b = has_body(parsed);
    let has_msg = has_location_hint(parsed);

    match parsed.mode {
        Some(AskUserMode::Confirm) => {
            if has_q {
                anyhow::bail!(
                    "ask_user confirm must not include questions; use mode=\"question\" or drop questions"
                );
            }
            if has_msg {
                anyhow::bail!(
                    "ask_user confirm must not include message; use mode=\"location\" for GPS/city"
                );
            }
            Ok(AskUserMode::Confirm)
        }
        Some(AskUserMode::Question) => {
            if has_b {
                anyhow::bail!(
                    "ask_user question must not include body; use mode=\"confirm\" for approvals"
                );
            }
            if has_msg {
                anyhow::bail!(
                    "ask_user question must not include message; use mode=\"location\" for GPS/city"
                );
            }
            Ok(AskUserMode::Question)
        }
        Some(AskUserMode::Location) => {
            if has_q || has_b {
                anyhow::bail!(
                    "ask_user location must not include questions or body; only optional message"
                );
            }
            Ok(AskUserMode::Location)
        }
        None => {
            if has_msg && !has_q && !has_b {
                anyhow::bail!(
                    "ask_user: location requires mode=\"location\" (optional message alone is ambiguous)"
                );
            }
            if has_q && has_b {
                anyhow::bail!(
                    "ask_user: do not mix questions and body; set mode=\"question\" or mode=\"confirm\""
                );
            }
            if has_q {
                return Ok(AskUserMode::Question);
            }
            if has_b {
                return Ok(AskUserMode::Confirm);
            }
            anyhow::bail!(
                "ask_user: set mode=\"question\"+questions, mode=\"confirm\"+title+body, or mode=\"location\""
            );
        }
    }
}

/// 按 `mode`（或严格推断）分派载荷。
pub fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: AskUserArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("ask_user 参数无效: {e}"))?;

    match resolve_ask_user_mode(&parsed)? {
        AskUserMode::Confirm => build_confirm(&parsed),
        AskUserMode::Question => build_question(&parsed),
        AskUserMode::Location => build_location(&parsed),
    }
}

fn build_confirm(parsed: &AskUserArgs) -> anyhow::Result<String> {
    let title = parsed.title.as_deref().unwrap_or("").trim();
    let body = parsed.body.as_deref().unwrap_or("").trim();
    if title.is_empty() {
        anyhow::bail!("ask_user confirm 需要 title");
    }
    if body.is_empty() {
        anyhow::bail!("ask_user confirm 需要 body");
    }

    let surface_id = format!("confirm-{}", Uuid::new_v4());
    let operations = a2ui::templates::build_confirm_surface(&surface_id, title, body);
    a2ui::validate_operations(&operations)
        .map_err(|e| anyhow::anyhow!("ask_user A2UI 无效: {e}"))?;

    Ok(json!({
        "astro_hitl": true,
        "reason": "confirmation",
        "message": title,
        "operations": operations,
        "response_schema": {
            "type": "object",
            "properties": { "approved": { "type": "boolean" } },
            "required": ["approved"]
        }
    })
    .to_string())
}

fn build_question(parsed: &AskUserArgs) -> anyhow::Result<String> {
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
        anyhow::bail!("ask_user question 需要至少一个 question");
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
        .map_err(|e| anyhow::anyhow!("ask_user A2UI 无效: {e}"))?;

    let message = if steps.len() == 1 {
        steps[0].question.clone()
    } else {
        format!("{}（{} 项）", title, steps.len())
    };

    Ok(json!({
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
    })
    .to_string())
}

fn build_location(parsed: &AskUserArgs) -> anyhow::Result<String> {
    let message = parsed
        .message
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or(DEFAULT_LOCATION_MESSAGE)
        .to_string();

    let surface_id = format!("location-{}", Uuid::new_v4());
    let operations = a2ui::templates::build_location_request_surface(&surface_id, &message);
    a2ui::validate_operations(&operations)
        .map_err(|e| anyhow::anyhow!("ask_user location A2UI 无效: {e}"))?;

    Ok(json!({
        "astro_hitl": true,
        "reason": "location_required",
        "message": message,
        "operations": operations,
        "response_schema": {
            "type": "object",
            "description": "Exactly one shape: coords, city, or denied",
            "properties": {
                "latitude": { "type": "number" },
                "longitude": { "type": "number" },
                "accuracy_m": { "type": "number" },
                "city": { "type": "string" },
                "denied": { "type": "boolean" }
            },
            "oneOf": [
                {
                    "required": ["latitude", "longitude"],
                    "description": "Geolocation granted"
                },
                {
                    "required": ["city"],
                    "description": "User typed a city"
                },
                {
                    "required": ["denied"],
                    "properties": { "denied": { "const": true } },
                    "description": "User denied location"
                }
            ]
        }
    })
    .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: serde_json::Value) -> AskUserArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn omit_mode_questions_only_is_question() {
        let a = args(json!({
            "questions": [{ "question": "Which env?" }]
        }));
        assert_eq!(resolve_ask_user_mode(&a).unwrap(), AskUserMode::Question);
    }

    #[test]
    fn omit_mode_body_only_is_confirm() {
        let a = args(json!({
            "title": "Delete?",
            "body": "Really?"
        }));
        assert_eq!(resolve_ask_user_mode(&a).unwrap(), AskUserMode::Confirm);
    }

    #[test]
    fn omit_mode_mix_errors() {
        let a = args(json!({
            "questions": [{ "question": "x" }],
            "body": "y"
        }));
        assert!(resolve_ask_user_mode(&a).unwrap_err().to_string().contains("mix"));
    }

    #[test]
    fn message_alone_does_not_infer_location() {
        let a = args(json!({ "message": "need weather" }));
        let err = resolve_ask_user_mode(&a).unwrap_err().to_string();
        assert!(err.contains("location"), "{err}");
    }

    #[test]
    fn explicit_location_ok() {
        let a = args(json!({ "mode": "location" }));
        assert_eq!(resolve_ask_user_mode(&a).unwrap(), AskUserMode::Location);
    }

    #[test]
    fn location_rejects_questions() {
        let a = args(json!({
            "mode": "location",
            "questions": [{ "question": "x" }]
        }));
        assert!(resolve_ask_user_mode(&a)
            .unwrap_err()
            .to_string()
            .contains("location"));
    }
}
