//! 轻量 JSON Schema 校验（HITL resume payload）。
//!
//! 仅支持 Astro HITL 所需子集：`type`、`required`、`properties`（及嵌套 object/array/string/number/boolean）。

use serde_json::Value;

/// 用 `schema` 校验 `instance`；失败返回人类可读错误。
pub fn validate_against_schema(schema: &Value, instance: &Value) -> Result<(), String> {
    if schema.is_null() || schema.as_object().map(|o| o.is_empty()).unwrap_or(false) {
        return Ok(());
    }
    validate_node(schema, instance, "$")
}

fn validate_node(schema: &Value, instance: &Value, path: &str) -> Result<(), String> {
    let Some(obj) = schema.as_object() else {
        return Ok(());
    };

    if let Some(ty) = obj.get("type").and_then(|v| v.as_str()) {
        check_type(ty, instance, path)?;
    }

    if let Some(required) = obj.get("required").and_then(|v| v.as_array()) {
        let Some(map) = instance.as_object() else {
            return Err(format!("{path}: expected object for required fields"));
        };
        for key in required {
            let Some(name) = key.as_str() else { continue };
            if !map.contains_key(name) {
                return Err(format!("{path}: missing required field `{name}`"));
            }
        }
    }

    if let Some(props) = obj.get("properties").and_then(|v| v.as_object()) {
        if let Some(map) = instance.as_object() {
            for (key, sub_schema) in props {
                if let Some(val) = map.get(key) {
                    let child = format!("{path}.{key}");
                    validate_node(sub_schema, val, &child)?;
                }
            }
        }
    }

    if let Some(items_schema) = obj.get("items") {
        if let Some(arr) = instance.as_array() {
            for (i, item) in arr.iter().enumerate() {
                let child = format!("{path}[{i}]");
                validate_node(items_schema, item, &child)?;
            }
        }
    }

    Ok(())
}

fn check_type(expected: &str, instance: &Value, path: &str) -> Result<(), String> {
    let ok = match expected {
        "object" => instance.is_object(),
        "array" => instance.is_array(),
        "string" => instance.is_string(),
        "number" => instance.is_number(),
        "integer" => instance.as_i64().is_some() || instance.as_u64().is_some(),
        "boolean" => instance.is_boolean(),
        "null" => instance.is_null(),
        _ => true,
    };
    if ok {
        Ok(())
    } else {
        Err(format!("{path}: expected type `{expected}`"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn confirm_schema_ok() {
        let schema = json!({
            "type": "object",
            "properties": { "approved": { "type": "boolean" } },
            "required": ["approved"]
        });
        validate_against_schema(&schema, &json!({"approved": true})).unwrap();
    }

    #[test]
    fn confirm_schema_missing() {
        let schema = json!({
            "type": "object",
            "required": ["approved"],
            "properties": { "approved": { "type": "boolean" } }
        });
        let err = validate_against_schema(&schema, &json!({})).unwrap_err();
        assert!(err.contains("approved"));
    }

    #[test]
    fn clarify_payload_requires_answers_and_value() {
        let schema = json!({
            "type": "object",
            "properties": {
                "answers": { "type": "object" },
                "value": { "type": "string" }
            },
            "required": ["answers", "value"]
        });
        assert!(validate_against_schema(&schema, &json!({"value": "ok"})).is_err());
        assert!(validate_against_schema(&schema, &json!({"answers": {}, "value": 1})).is_err());
        validate_against_schema(&schema, &json!({"answers": {"q0": "ok"}, "value": "ok"})).unwrap();
    }
}
