//! 模型工具调用描述与 Provider 回放元数据。

use serde::{Deserialize, Serialize};

/// 模型发起的一次工具调用。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// 调用 id，与工具结果对应。
    pub id: String,
    /// 工具名。
    pub name: String,
    /// Responses API 原生工具命名空间。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    /// JSON 参数。
    pub arguments: serde_json::Value,
    /// Google Interactions / Gemini 3：`function_call.signature`，无状态回放时必须原样回传。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// `reasoning_details` 中存放 Google Interactions `thought.signature` 的键。
pub const GOOGLE_THOUGHT_SIGNATURE_KEY: &str = "google_thought_signature";

/// 将 `thought.signature` 合并进 `reasoning_details`（供会话落盘）。
///
/// 没有 Google `thought.signature` 时原样返回 `details`：这里的 `details` 同时承载
/// Astro 自己的时间线（`astro_timeline_v1` / `astro_surfaces_v1`），不能因为不是
/// Google 模型就把整份 metadata 丢掉。
pub fn merge_google_thought_signature(
    details: Option<serde_json::Value>,
    signature: Option<&str>,
) -> Option<serde_json::Value> {
    let Some(sig) = signature.map(str::trim).filter(|s| !s.is_empty()) else {
        return details;
    };
    let mut obj = match details {
        Some(serde_json::Value::Object(obj)) => obj,
        _ => serde_json::Map::new(),
    };
    obj.insert(
        GOOGLE_THOUGHT_SIGNATURE_KEY.into(),
        serde_json::Value::String(sig.to_string()),
    );
    Some(serde_json::Value::Object(obj))
}

/// 从 `reasoning_details` 读出 Google `thought.signature`。
pub fn google_thought_signature_from_details(
    details: &Option<serde_json::Value>,
) -> Option<String> {
    details
        .as_ref()?
        .get(GOOGLE_THOUGHT_SIGNATURE_KEY)?
        .as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn google_signature_round_trips_through_metadata() {
        let details = merge_google_thought_signature(None, Some("sig-1"));
        assert_eq!(
            google_thought_signature_from_details(&details).as_deref(),
            Some("sig-1")
        );
    }

    #[test]
    fn non_google_providers_keep_astro_metadata() {
        let details = Some(serde_json::json!({"astro_timeline_v1": [{"type": "text"}]}));
        let merged = merge_google_thought_signature(details.clone(), None);
        assert_eq!(merged, details);
        assert_eq!(merge_google_thought_signature(None, None), None);
        assert_eq!(
            merge_google_thought_signature(None, Some("  ")).map(|value| value.to_string()),
            None
        );
    }
}
