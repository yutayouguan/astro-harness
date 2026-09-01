//! Provider dispatch 与 profile 相关测试。

use providers::{
    image_gen::*,
    profile::{env_api_key_names, resolve},
    AuthKind,
};

#[test]
fn test_profile_has_all_providers() {
    let ids = [
        "google",
        "openai",
        "claude",
        "deepseek",
        "minimax",
        "zhipu",
        "mimo",
        "ollama",
        "openrouter",
        "bailian",
        "nvidia",
        "moonshot",
        "volcengine",
        "azure",
        "hunyuan",
    ];
    for id in ids {
        assert!(resolve(id).is_some(), "profile missing for {id}");
    }
    assert!(resolve("anthropic").is_some());
    assert!(resolve("minmax").is_some());
}

#[test]
fn test_image_generation_provider_support() {
    assert!(providers::dispatch::supports_image_gen("google"));
    assert!(providers::dispatch::supports_image_gen("azure"));
}

#[test]
fn test_claude_no_image_gen() {
    assert!(!providers::dispatch::supports_image_gen("claude"));
}

#[test]
fn test_image_request_builder() {
    let req = ImageGenRequest::builder()
        .prompt("一只在宇宙中漂浮的猫")
        .provider("google")
        .size(1024, 1024)
        .count(1)
        .build();
    assert_eq!(req.provider, "google");
    assert_eq!(req.width, 1024);
    assert_eq!(req.model, "gemini-3.6-flash");
}

#[test]
fn test_default_image_models() {
    assert_eq!(default_image_model("google"), "gemini-3.6-flash");
    assert_eq!(default_image_model("openai"), "gpt-image-2");
    assert_eq!(default_image_model("azure"), "gpt-image-2");
    let openai_req = ImageGenRequest::builder()
        .prompt("cat")
        .provider("openai")
        .build();
    assert_eq!(openai_req.model, "gpt-image-2");
}

#[test]
fn test_auth_kind_for_providers() {
    assert_eq!(AuthKind::for_provider("ollama"), AuthKind::None);
    assert_eq!(AuthKind::for_provider("claude"), AuthKind::AnthropicKey);
    assert_eq!(AuthKind::for_provider("anthropic"), AuthKind::AnthropicKey);
    assert_eq!(AuthKind::for_provider("google"), AuthKind::GoogleApiKey);
    assert_eq!(AuthKind::for_provider("azure"), AuthKind::AzureHeader);
    assert_eq!(AuthKind::for_provider("openai"), AuthKind::Bearer);
    assert_eq!(AuthKind::for_provider("deepseek"), AuthKind::Bearer);
}

#[test]
fn test_env_api_key_names() {
    assert!(env_api_key_names("openai").contains(&"OPENAI_API_KEY"));
    assert!(env_api_key_names("claude").contains(&"ANTHROPIC_API_KEY"));
    assert!(env_api_key_names("bailian").contains(&"DASHSCOPE_API_KEY"));
    assert!(env_api_key_names("ollama").is_empty());
}

#[test]
fn test_default_model_lookup() {
    let model = providers::dispatch::default_model("openai");
    assert!(!model.is_empty());
}
