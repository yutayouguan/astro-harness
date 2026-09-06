//! 工具选择策略下发 — 将统一的 `ToolChoice` 映射到各 provider 线路格式。

use serde_json::{json, Value};

use crate::types::ToolChoice;

pub(crate) fn apply_openai_chat(
    body: &mut Value,
    choice: Option<&ToolChoice>,
    parallel: Option<bool>,
) {
    if let Some(choice) = choice {
        body["tool_choice"] = match choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::Required => json!("required"),
            ToolChoice::None => json!("none"),
            ToolChoice::Specific(name) => {
                json!({"type": "function", "function": {"name": name}})
            }
        };
    }
    if let Some(parallel) = parallel {
        body["parallel_tool_calls"] = json!(parallel);
    }
}

pub(crate) fn apply_openai_responses(
    body: &mut Value,
    choice: Option<&ToolChoice>,
    parallel: Option<bool>,
) {
    if let Some(choice) = choice {
        body["tool_choice"] = match choice {
            ToolChoice::Auto => json!("auto"),
            ToolChoice::Required => json!("required"),
            ToolChoice::None => json!("none"),
            ToolChoice::Specific(name) => json!({"type": "function", "name": name}),
        };
    }
    if let Some(parallel) = parallel {
        body["parallel_tool_calls"] = json!(parallel);
    }
}

pub(crate) fn apply_anthropic(
    body: &mut Value,
    choice: Option<&ToolChoice>,
    parallel: Option<bool>,
) {
    if choice.is_none() && parallel.is_none() {
        return;
    }
    let mut policy = match choice {
        Some(ToolChoice::Auto) | None => json!({"type": "auto"}),
        Some(ToolChoice::Required) => json!({"type": "any"}),
        Some(ToolChoice::None) => json!({"type": "none"}),
        Some(ToolChoice::Specific(name)) => json!({"type": "tool", "name": name}),
    };
    if let Some(parallel) = parallel {
        policy["disable_parallel_tool_use"] = json!(!parallel);
    }
    body["tool_choice"] = policy;
}

pub(crate) fn apply_google_interactions(body: &mut Value, choice: Option<&ToolChoice>) {
    let Some(choice) = choice else {
        return;
    };
    let value = match choice {
        ToolChoice::Auto => json!("auto"),
        ToolChoice::Required => json!("any"),
        ToolChoice::None => json!("none"),
        ToolChoice::Specific(name) => {
            json!({"allowed_tools": {"mode": "any", "tools": [name]}})
        }
    };
    ensure_object(body, "generation_config")["tool_choice"] = value;
}

pub(crate) fn apply_gemini_native(body: &mut Value, choice: Option<&ToolChoice>) {
    let Some(choice) = choice else {
        return;
    };
    let function_calling = match choice {
        ToolChoice::Auto => json!({"mode": "AUTO"}),
        ToolChoice::Required => json!({"mode": "ANY"}),
        ToolChoice::None => json!({"mode": "NONE"}),
        ToolChoice::Specific(name) => {
            json!({"mode": "ANY", "allowedFunctionNames": [name]})
        }
    };
    ensure_object(body, "toolConfig")["functionCallingConfig"] = function_calling;
}

fn ensure_object<'a>(body: &'a mut Value, key: &str) -> &'a mut Value {
    if !body.get(key).is_some_and(Value::is_object) {
        body[key] = json!({});
    }
    &mut body[key]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn specific_tool_policy_uses_each_provider_native_shape() {
        let choice = ToolChoice::Specific("submit".into());

        let mut chat = json!({});
        apply_openai_chat(&mut chat, Some(&choice), Some(false));
        assert_eq!(chat["tool_choice"]["function"]["name"], "submit");
        assert_eq!(chat["parallel_tool_calls"], false);

        let mut responses = json!({});
        apply_openai_responses(&mut responses, Some(&choice), Some(false));
        assert_eq!(responses["tool_choice"]["name"], "submit");
        assert_eq!(responses["parallel_tool_calls"], false);

        let mut anthropic = json!({});
        apply_anthropic(&mut anthropic, Some(&choice), Some(false));
        assert_eq!(anthropic["tool_choice"]["name"], "submit");
        assert_eq!(anthropic["tool_choice"]["disable_parallel_tool_use"], true);

        let mut interactions = json!({});
        apply_google_interactions(&mut interactions, Some(&choice));
        assert_eq!(
            interactions["generation_config"]["tool_choice"]["allowed_tools"]["tools"][0],
            "submit"
        );

        let mut gemini = json!({});
        apply_gemini_native(&mut gemini, Some(&choice));
        assert_eq!(
            gemini["toolConfig"]["functionCallingConfig"]["allowedFunctionNames"][0],
            "submit"
        );
    }
}
