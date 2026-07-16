//! Provider 客户端、注册表与图片生成相关测试。

use providers::{
    client::{env_api_key_names, ProviderClient},
    image_gen::*,
    registry::ProviderRegistry,
    AuthKind, ChatProvider, VerifyProvider,
};

#[test]
fn test_registry_has_all_providers() {
    let registry = ProviderRegistry::default();
    assert!(registry.get("google").is_some());
    assert!(registry.get("openai").is_some());
    assert!(registry.get("claude").is_some());
    assert!(registry.get("deepseek").is_some());
    assert!(registry.get("minimax").is_some());
    assert!(registry.get("minmax").is_some()); // 旧名别名
    assert!(registry.get("anthropic").is_some()); // → claude
    assert!(registry.get("zhipu").is_some());
    assert!(registry.get("mimo").is_some());
    assert!(registry.get("ollama").is_some());
    assert!(registry.get("openrouter").is_some());
    assert!(registry.get("bailian").is_some());
    assert!(registry.get("nvidia").is_some());
    assert!(registry.get("moonshot").is_some());
    assert!(registry.get("volcengine").is_some());
    assert!(registry.get("azure").is_some());
}

#[test]
fn test_google_supports_image_gen() {
    let registry = ProviderRegistry::default();
    let google = registry.get("google").unwrap();
    assert!(google.supports_image_gen());
}

#[test]
fn test_claude_no_image_gen() {
    let registry = ProviderRegistry::default();
    let claude = registry.get("claude").unwrap();
    assert!(!claude.supports_image_gen());
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
    assert_eq!(req.model, "gemini-3.1-flash-image");
}

#[test]
fn test_default_image_models() {
    assert_eq!(default_image_model("google"), "gemini-3.1-flash-image");
    assert_eq!(default_image_model("openai"), "gpt-image-2");
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
fn test_provider_client_from_config() {
    let client = ProviderClient::from_config(
        "openai",
        "sk-test".into(),
        Some("https://api.openai.com/v1".into()),
    );
    assert_eq!(client.provider_id, "openai");
    assert_eq!(client.api_key, "sk-test");
    assert_eq!(client.auth, AuthKind::Bearer);

    let ollama = ProviderClient::from_config("ollama", String::new(), None);
    assert_eq!(ollama.auth, AuthKind::None);

    let aliased = ProviderClient::from_config("anthropic", "k".into(), None);
    assert_eq!(aliased.provider_id, "claude");
    assert_eq!(aliased.auth, AuthKind::AnthropicKey);
}

#[test]
fn test_provider_client_from_env_ollama() {
    let client = ProviderClient::from_env("ollama").expect("ollama needs no key");
    assert_eq!(client.provider_id, "ollama");
    assert_eq!(client.auth, AuthKind::None);
}

#[test]
fn test_env_api_key_names() {
    assert!(env_api_key_names("openai").contains(&"OPENAI_API_KEY"));
    assert!(env_api_key_names("claude").contains(&"ANTHROPIC_API_KEY"));
    assert!(env_api_key_names("bailian").contains(&"DASHSCOPE_API_KEY"));
    assert!(env_api_key_names("ollama").is_empty());
}

#[test]
fn test_providers_expose_chat_and_verify_traits() {
    let registry = ProviderRegistry::default();
    let openai = registry.get("openai").unwrap();
    // dyn AiProvider 同时是 ChatProvider + VerifyProvider
    let _: &dyn ChatProvider = openai.as_ref();
    let _: &dyn VerifyProvider = openai.as_ref();
    assert_eq!(openai.name(), "openai");
    assert_eq!(openai.auth_kind(), AuthKind::Bearer);
}
