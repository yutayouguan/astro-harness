//! 从 `~/.astro/litellm-model-meta.json` 读取单价并估算 LLM 费用（USD）。

use serde_json::Value;
use std::fs;

use crate::workspace::default_memory_dir;

fn cache_path() -> std::path::PathBuf {
    default_memory_dir().join("litellm-model-meta.json")
}

/// 估算：`prompt * input_cost_per_token + completion * output_cost_per_token`。
/// 未知模型或缺字段返回 `0.0`。
pub fn estimate_llm_cost(model: &str, prompt_tokens: u32, completion_tokens: u32) -> f64 {
    let model = model.trim().to_ascii_lowercase();
    if model.is_empty() {
        return 0.0;
    }
    let Ok(raw) = fs::read_to_string(cache_path()) else {
        return 0.0;
    };
    let Ok(v) = serde_json::from_str::<Value>(&raw) else {
        return 0.0;
    };
    let Some(obj) = v.as_object() else {
        return 0.0;
    };
    // 精确键或后缀匹配（provider/model）；比较时统一小写
    let entry = obj
        .get(&model)
        .or_else(|| {
            obj.iter()
                .find(|(k, _)| {
                    let k = k.to_ascii_lowercase();
                    k == model || k.ends_with(&format!("/{model}"))
                })
                .map(|(_, v)| v)
        });
    let Some(entry) = entry else {
        return 0.0;
    };
    let input = entry
        .get("input_cost_per_token")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    let output = entry
        .get("output_cost_per_token")
        .and_then(|x| x.as_f64())
        .unwrap_or(0.0);
    input * f64::from(prompt_tokens) + output * f64::from(completion_tokens)
}
