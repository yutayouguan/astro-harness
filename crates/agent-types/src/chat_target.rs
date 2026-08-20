use serde::{Deserialize, Serialize};

pub const MAX_CHAT_FALLBACKS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatTarget {
    pub provider_id: String,
    pub backend_id: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
    /// API 协议模式覆盖（空 = profile 默认；`"responses"` = Responses API）。
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_mode: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FallbackRef {
    pub provider_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

/// `lookup` 返回已解析凭据的条目（含 key）；None 则跳过。
pub fn expand_chat_targets<F>(
    primary: &ChatTarget,
    fallbacks: &[FallbackRef],
    mut lookup: F,
) -> Vec<ChatTarget>
where
    F: FnMut(&str) -> Option<ChatTarget>,
{
    let mut out = vec![primary.clone()];
    let mut seen = std::collections::HashSet::new();
    seen.insert(primary.provider_id.clone());
    for fr in fallbacks.iter().take(MAX_CHAT_FALLBACKS * 2) {
        // 多读一点以便跳过后仍能填满 3 条
        if out.len() > MAX_CHAT_FALLBACKS {
            break;
        }
        if fr.provider_id.is_empty() || !seen.insert(fr.provider_id.clone()) {
            continue;
        }
        let Some(mut t) = lookup(&fr.provider_id) else {
            continue;
        };
        if let Some(m) = fr.model.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            t.model = m.to_string();
        }
        // key 空且非本地无需 key 的 backend：仍允许 ollama（api_key 可空）
        let allow_empty_key = t.backend_id == "ollama";
        if t.api_key.trim().is_empty() && !allow_empty_key {
            continue;
        }
        out.push(t);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(id: &str, backend: &str, model: &str) -> ChatTarget {
        ChatTarget {
            provider_id: id.into(),
            backend_id: backend.into(),
            model: model.into(),
            api_key: format!("k-{id}"),
            base_url: format!("https://{id}.example"),
            api_mode: String::new(),
        }
    }

    #[test]
    fn expand_skips_missing_dedups_and_caps() {
        let primary = t("p0", "openai", "gpt");
        let catalog = [
            t("p0", "openai", "gpt"),
            t("p1", "claude", "opus"),
            t("p2", "deepseek", "chat"),
            t("p3", "zhipu", "glm"),
            t("p4", "ollama", "llama"), // 第 4 个后备应被截断
        ];
        let refs = vec![
            FallbackRef {
                provider_id: "p1".into(),
                model: Some("opus-x".into()),
            },
            FallbackRef {
                provider_id: "missing".into(),
                model: None,
            },
            FallbackRef {
                provider_id: "p1".into(),
                model: None,
            }, // dup
            FallbackRef {
                provider_id: "p0".into(),
                model: None,
            }, // self
            FallbackRef {
                provider_id: "p2".into(),
                model: None,
            },
            FallbackRef {
                provider_id: "p3".into(),
                model: None,
            },
            FallbackRef {
                provider_id: "p4".into(),
                model: None,
            },
        ];
        let lookup = |id: &str| catalog.iter().find(|c| c.provider_id == id).cloned();
        let chain = expand_chat_targets(&primary, &refs, lookup);
        assert_eq!(chain.len(), 1 + MAX_CHAT_FALLBACKS); // primary + 3
        assert_eq!(chain[0].provider_id, "p0");
        assert_eq!(chain[1].model, "opus-x");
        assert_eq!(chain[2].provider_id, "p2");
        assert_eq!(chain[3].provider_id, "p3");
    }
}
