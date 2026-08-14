//! 编译期能力检查验证。

#[cfg(test)]
use crate::impls::deepseek::DeepSeek;
#[cfg(test)]
use crate::impls::ollama::Ollama;
#[cfg(test)]
use crate::impls::openai::OpenAI;
#[cfg(test)]
use crate::impls::zhipu::Zhipu;
#[cfg(test)]
use crate::traits::client::{ChatClient, ProviderClient};

#[test]
fn deepseek_has_chat() {
    let client = ProviderClient::new("test-key", DeepSeek);
    let _model = client.completion_model("deepseek-chat");
}

#[test]
fn openai_has_chat() {
    let client = ProviderClient::new("test-key", OpenAI);
    let _model = client.completion_model("gpt-4o");
}

#[test]
fn ollama_has_chat_no_key() {
    let client = ProviderClient::new("", Ollama);
    let _model = client.completion_model("llama3.3");
}

#[test]
fn zhipu_has_chat() {
    let client = ProviderClient::new("test-key", Zhipu);
    let _model = client.completion_model("glm-4");
}

// ── finalize_body 单元测试 ──

#[cfg(test)]
mod finalize_body {
    use crate::compat::OpenAICompatible;
    use serde_json::{json, Value};

    #[test]
    fn deepseek_thinking_enabled_maps_effort() {
        let ds = crate::impls::deepseek::DeepSeek;
        let mut body = json!({
            "model": "deepseek-v4",
            "thinking_config": {"enabled": true, "effort": "max"}
        });
        ds.finalize_body(&mut body);
        assert_eq!(body["thinking"]["type"], "enabled");
        assert_eq!(body["reasoning_effort"], "max");
        assert!(body.get("thinking_config").is_none());
    }

    #[test]
    fn deepseek_thinking_disabled() {
        let ds = crate::impls::deepseek::DeepSeek;
        let mut body = json!({
            "model": "deepseek-v4",
            "thinking_config": {"enabled": false}
        });
        ds.finalize_body(&mut body);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert!(body.get("reasoning_effort").is_none());
    }

    #[test]
    fn deepseek_effort_high() {
        let ds = crate::impls::deepseek::DeepSeek;
        let mut body = json!({"thinking_config": {"enabled": true, "effort": "high"}});
        ds.finalize_body(&mut body);
        assert_eq!(body["reasoning_effort"], "high");
    }

    #[test]
    fn minimax_sets_reasoning_split_and_adaptive() {
        let mm = crate::impls::minimax_chat::MiniMax;
        let mut body = json!({
            "model": "MiniMax-M3",
            "thinking_config": {"enabled": true}
        });
        mm.finalize_body(&mut body);
        assert_eq!(body["reasoning_split"], true);
        assert_eq!(body["thinking"]["type"], "adaptive");
        assert!(body.get("thinking_config").is_none());
    }

    #[test]
    fn minimax_thinking_disabled() {
        let mm = crate::impls::minimax_chat::MiniMax;
        let mut body = json!({
            "model": "MiniMax-M3",
            "thinking_config": {"enabled": false}
        });
        mm.finalize_body(&mut body);
        assert_eq!(body["thinking"]["type"], "disabled");
        assert_eq!(body["reasoning_split"], true);
    }

    #[test]
    fn azure_removes_model() {
        let az = crate::impls::azure::Azure;
        let mut body = json!({
            "model": "gpt-4o",
            "messages": [],
            "stream": true
        });
        az.finalize_body(&mut body);
        assert!(body.get("model").is_none());
        assert_eq!(body["stream"], true);
    }

    #[test]
    fn openai_reasoning_effort_from_thinking_config() {
        let oi = crate::impls::openai::OpenAI;
        let mut body = json!({
            "model": "o3",
            "thinking_config": {"enabled": true, "effort": "max"}
        });
        oi.finalize_body(&mut body);
        assert_eq!(body["reasoning_effort"], "high");
        assert!(body.get("thinking_config").is_none());
    }

    #[test]
    fn openai_thinking_disabled_no_reasoning_effort() {
        let oi = crate::impls::openai::OpenAI;
        let mut body = json!({
            "model": "o3",
            "thinking_config": {"enabled": false}
        });
        oi.finalize_body(&mut body);
        assert!(body.get("reasoning_effort").is_none());
        assert!(body.get("thinking_config").is_none());
    }

    #[test]
    fn openai_no_thinking_config_is_noop() {
        let oi = crate::impls::openai::OpenAI;
        let mut body = json!({"model": "gpt-4o", "messages": []});
        let expected = body.clone();
        oi.finalize_body(&mut body);
        assert_eq!(body, expected);
    }
}
