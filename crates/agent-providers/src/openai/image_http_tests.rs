use super::*;
use serde_json::json;

#[test]
fn azure_foundry_v1_request_matches_official_contract() {
    let config = ProviderConfig {
        api_key: "azure-secret".to_string(),
        base_url: Some("https://example.services.ai.azure.com/openai/v1/".to_string()),
        model: "gpt-image-2".to_string(),
        ..ProviderConfig::default()
    };

    let base = openai_base(&config);
    assert_eq!(base, "https://example.services.ai.azure.com/openai/v1");
    let body =
        image_generation_request_body(&config.model, "A red fox", ImageApiFlavor::AzureFoundryV1);
    assert_eq!(
        body,
        json!({
            "model": "gpt-image-2",
            "prompt": "A red fox",
            "n": 1,
            "size": "1024x1024",
            "output_format": "png",
            "output_compression": 100,
        })
    );

    let request = build_image_generation_request(
        &Client::new(),
        &format!("{base}/images/generations"),
        &config.api_key,
        &body,
    )
    .build()
    .expect("request should build");
    assert_eq!(
        request.url().as_str(),
        "https://example.services.ai.azure.com/openai/v1/images/generations"
    );
    assert_eq!(
        request
            .headers()
            .get("authorization")
            .expect("authorization header should exist"),
        "Bearer azure-secret"
    );
    assert_eq!(
        request
            .headers()
            .get("content-type")
            .expect("content-type header should exist"),
        "application/json"
    );
}

#[test]
fn openai_compatible_request_does_not_add_azure_only_fields() {
    let body =
        image_generation_request_body("gpt-image-1", "A red fox", ImageApiFlavor::OpenAiCompatible);
    assert_eq!(
        body,
        json!({
            "model": "gpt-image-1",
            "prompt": "A red fox",
            "n": 1,
            "size": "1024x1024",
        })
    );
}

#[test]
fn image_error_message_prefers_structured_api_error() {
    assert_eq!(
        image_error_message(br#"{"error":{"message":"quota exceeded"}}"#),
        "quota exceeded"
    );
}

#[test]
fn response_url_drops_sensitive_query_parameters() {
    let url = reqwest::Url::parse(
        "https://example.services.ai.azure.com/openai/v1/images/generations?api-key=secret",
    )
    .expect("test URL should parse");
    assert_eq!(
        response_url_without_query(&url),
        "https://example.services.ai.azure.com/openai/v1/images/generations"
    );
}

#[tokio::test]
async fn azure_foundry_rejects_classic_deployment_endpoint() {
    let config = ProviderConfig {
        api_key: "azure-secret".to_string(),
        base_url: Some("https://example.openai.azure.com".to_string()),
        model: "gpt-image-2".to_string(),
        ..ProviderConfig::default()
    };

    let error = azure_foundry_generate_image(&Client::new(), "A red fox", &config)
        .await
        .expect_err("classic Azure endpoint should be rejected");
    assert!(error.to_string().contains("Foundry OpenAI v1 endpoint"));
    assert!(!error.to_string().contains("azure-secret"));
}
