//! Non-blocking structured questions shown to the user while a turn continues.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::context::ToolContext;
use crate::registry::ToolRegistry;
use crate::schema::schema_for_args;
use types::AsyncUserInputQuestion;

const TOOL_NAME: &str = "request_user_input_async";
const LEGACY_TOOL_NAME: &str = "send_user_message_async";
const ASYNC_USER_MESSAGE_MARKER: &str = "astro_async_user_message";

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestUserInputAsyncArgs {
    /// One or more self-contained questions, in display order.
    questions: Vec<AsyncUserInputQuestionArgs>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AsyncUserInputQuestionArgs {
    /// The complete question shown to the user.
    title: String,
    /// Suggested answers in display order. Free-text input is always available.
    #[serde(default)]
    options: Option<Vec<String>>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct LegacySendUserMessageAsyncArgs {
    message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AsyncUserMessagePayload {
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub questions: Option<Vec<AsyncUserInputQuestion>>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(crate::registry::ToolEntry {
        name: TOOL_NAME.into(),
        toolset: TOOL_NAME.into(),
        description: "Ask the user one or more concise, self-contained questions while work continues. Suggested options are optional and the user can always answer with free text. Returns immediately; replies arrive asynchronously as new user messages."
            .into(),
        schema: schema_for_args::<RequestUserInputAsyncArgs>(),
        check_fn: None,
        icon: "message-circle",
        exclusive_access: true,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });

    // Keep the retired name dispatchable for persisted/custom prompts without exposing it to
    // current models or the user-facing catalog.
    registry.register(crate::registry::ToolEntry {
        name: LEGACY_TOOL_NAME.into(),
        toolset: TOOL_NAME.into(),
        description: "Legacy alias for request_user_input_async".into(),
        schema: schema_for_args::<LegacySendUserMessageAsyncArgs>(),
        check_fn: None,
        icon: "message-circle",
        exclusive_access: true,
        exposure: types::ToolExposure::Hidden,
        ..crate::registry::ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["request_user_input_async", "send_user_message_async"],
    sync_named: dispatch,
}

fn dispatch(
    _ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    let payload = if name == LEGACY_TOOL_NAME {
        let args: LegacySendUserMessageAsyncArgs = serde_json::from_value(args.clone())?;
        let message = args.message.trim();
        if message.is_empty() {
            anyhow::bail!("send_user_message_async requires a non-empty message");
        }
        AsyncUserMessagePayload {
            message: message.into(),
            questions: None,
        }
    } else {
        let args: RequestUserInputAsyncArgs = serde_json::from_value(args.clone())?;
        validate_questions(&args.questions)?;
        let questions = args
            .questions
            .into_iter()
            .map(|question| AsyncUserInputQuestion {
                title: question.title.trim().to_string(),
                options: question.options.map(|options| {
                    options
                        .into_iter()
                        .map(|option| option.trim().to_string())
                        .collect()
                }),
            })
            .collect::<Vec<_>>();
        AsyncUserMessagePayload {
            message: render_questions(&questions),
            questions: Some(questions),
        }
    };

    Ok(json!({
        ASYNC_USER_MESSAGE_MARKER: true,
        "message": payload.message,
        "questions": payload.questions,
    })
    .to_string())
}

fn validate_questions(questions: &[AsyncUserInputQuestionArgs]) -> anyhow::Result<()> {
    if questions.is_empty() {
        anyhow::bail!("questions must not be empty");
    }
    for question in questions {
        if question.title.trim().is_empty() {
            anyhow::bail!("question titles must not be empty");
        }
        if let Some(options) = &question.options {
            if options.is_empty() || options.iter().any(|option| option.trim().is_empty()) {
                anyhow::bail!("options must contain at least one non-empty answer");
            }
        }
    }
    Ok(())
}

fn render_questions(questions: &[AsyncUserInputQuestion]) -> String {
    questions
        .iter()
        .map(|question| {
            let mut lines = vec![question.title.trim().to_string()];
            if let Some(options) = &question.options {
                lines.extend(options.iter().map(|option| format!("- {}", option.trim())));
            }
            lines.join("\n")
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

pub fn parse_async_user_message(result: &str) -> Option<AsyncUserMessagePayload> {
    let value: serde_json::Value = serde_json::from_str(result).ok()?;
    if value.get(ASYNC_USER_MESSAGE_MARKER)?.as_bool() != Some(true) {
        return None;
    }
    let message = value.get("message")?.as_str()?.trim();
    if message.is_empty() {
        return None;
    }
    let questions = value
        .get("questions")
        .filter(|value| !value.is_null())
        .and_then(|value| serde_json::from_value(value.clone()).ok());
    Some(AsyncUserMessagePayload {
        message: message.into(),
        questions,
    })
}

#[cfg(test)]
#[path = "request_user_input_async_tests.rs"]
mod tests;
