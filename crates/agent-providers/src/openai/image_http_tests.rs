use super::*;
use serde_json::json;

#[test]
fn gpt_image_25_preserves_explicit_model_and_production_options() {
    for model in ["gpt-image-2.5-flare", "gpt-image-2.5-sunburst"] {
        for flavor in [
            ImageApiFlavor::AzureFoundryV1,
            ImageApiFlavor::OpenAiCompatible,
        ] {
            let config = ImageGenConfig {
                model: model.into(),
                width: Some(1536),
                height: Some(1024),
                quality: Some("medium".into()),
                output_format: Some("jpeg".into()),
                output_compression: Some(100),
                additional_params: json!({"model": "must-not-override"}),
                ..ImageGenConfig::default()
            };
            let body = image_generation_request_body(model, "A quiet pet scene", &config, flavor)
                .expect("new model uses the existing Images protocol");
            assert_eq!(body["model"], model);
            assert_eq!(body["size"], "1536x1024");
            assert_eq!(body["quality"], "medium");
            assert_eq!(body["output_format"], "jpeg");
            assert_eq!(body["output_compression"], 100);
            assert!(body.get("input_fidelity").is_none());
        }
    }
}

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
    let body = image_generation_request_body(
        &config.model,
        "A red fox",
        &ImageGenConfig::default(),
        ImageApiFlavor::AzureFoundryV1,
    )
    .expect("valid request");
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
    let body = image_generation_request_body(
        "gpt-image-1",
        "A red fox",
        &ImageGenConfig::default(),
        ImageApiFlavor::OpenAiCompatible,
    )
    .expect("valid request");
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
fn image_options_are_forwarded_and_validated() {
    let options = ImageGenConfig {
        width: Some(1536),
        height: Some(1024),
        n: 2,
        output_format: Some("webp".to_string()),
        output_compression: Some(80),
        quality: Some("high".to_string()),
        background: Some("transparent".to_string()),
        additional_params: serde_json::json!({
            "output_format": "jpeg",
            "moderation": "low",
        }),
        ..ImageGenConfig::default()
    };
    let body = image_generation_request_body(
        "gpt-image-2",
        "A red fox",
        &options,
        ImageApiFlavor::AzureFoundryV1,
    )
    .expect("valid request");
    assert_eq!(body["size"], "1536x1024");
    assert_eq!(body["n"], 2);
    assert_eq!(body["output_format"], "webp");
    assert_eq!(body["output_compression"], 80);
    assert_eq!(body["quality"], "high");
    assert_eq!(body["background"], "transparent");
    assert_eq!(body["moderation"], "low");

    let invalid = ImageGenConfig {
        n: 11,
        ..ImageGenConfig::default()
    };
    assert!(image_generation_request_body(
        "gpt-image-2",
        "A red fox",
        &invalid,
        ImageApiFlavor::AzureFoundryV1,
    )
    .is_err());

    let invalid_compression = ImageGenConfig {
        output_compression: Some(101),
        ..ImageGenConfig::default()
    };
    assert!(image_generation_request_body(
        "gpt-image-2",
        "A red fox",
        &invalid_compression,
        ImageApiFlavor::AzureFoundryV1,
    )
    .expect_err("compression over 100 must fail")
    .to_string()
    .contains("0..=100"));
}

#[test]
fn reference_images_are_validated_for_edit_requests() {
    let valid = ImageInput {
        data: vec![1, 2, 3],
        mime_type: "image/png".to_string(),
        filename: "pet.png".to_string(),
    };
    validate_image_inputs(&[valid]).expect("PNG reference should be accepted");

    let invalid = ImageInput {
        data: vec![1, 2, 3],
        mime_type: "image/svg+xml".to_string(),
        filename: "pet.svg".to_string(),
    };
    assert!(validate_image_inputs(&[invalid])
        .expect_err("SVG reference must be rejected")
        .to_string()
        .contains("PNG"));
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

#[test]
fn azure_foundry_accepts_both_current_host_styles() {
    for endpoint in [
        "https://example.services.ai.azure.com/openai/v1",
        "https://example.openai.azure.com/openai/v1",
        "https://example.openai.azure.com",
    ] {
        let config = ProviderConfig {
            base_url: Some(endpoint.to_string()),
            ..ProviderConfig::default()
        };
        let base = crate::impls::azure::azure_openai_v1_base(
            config.base_url.as_deref().expect("endpoint"),
        );
        assert!(base.ends_with("/openai/v1"), "{base}");
    }
}

#[test]
fn image_url_validation_rejects_local_targets() {
    for url in [
        "file:///tmp/image.png",
        "http://localhost/image.png",
        "http://127.0.0.1/image.png",
        "http://10.0.0.1/image.png",
        "http://[::1]/image.png",
    ] {
        assert!(validate_remote_image_url(url).is_err(), "{url}");
    }
    assert!(validate_remote_image_url("https://example.com/image.png").is_ok());
}
