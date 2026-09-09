//! Explicit, billable acceptance: two Azure requests using only the repository's generated sample.
//! Run with --ignored --exact azure_pet_and_wallpaper_live and ASTRO_PET_LIVE_TEST=1.
use providers::{
    dispatch::generate_image_with_options,
    types::{ImageGenConfig, ImageInput, ProviderConfig},
};

#[tokio::test]
#[ignore = "requires explicit opt-in, Azure credentials, and two billable image requests"]
async fn azure_pet_and_wallpaper_live() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(std::env::var("ASTRO_PET_LIVE_TEST").as_deref(), Ok("1"));
    let base = std::env::var("OPENAI_BASE_URL")?;
    let url = reqwest::Url::parse(&base)?;
    assert!(
        url.scheme() == "https"
            && url
                .host_str()
                .is_some_and(|h| h.ends_with(".openai.azure.com")
                    || h.ends_with(".cognitiveservices.azure.com"))
    );
    let config = ProviderConfig {
        api_key: std::env::var("OPENAI_API_KEY")?,
        base_url: Some(base),
        model: "gpt-image-2".into(),
        ..Default::default()
    };
    let output = std::path::PathBuf::from(std::env::var("ASTRO_PET_QA_OUTPUT")?).canonicalize()?;
    assert!(
        output.starts_with(std::env::temp_dir().canonicalize()?)
            || output.starts_with(std::path::Path::new("/tmp").canonicalize()?)
    );
    assert!(
        std::fs::read_dir(&output)?.next().is_none(),
        "acceptance output must be empty"
    );
    let sample =
        include_bytes!("../../../apps/desktop/src/assets/generated/desktop-pet-concept.png");
    let mut options = ImageGenConfig {
        width: Some(1024),
        height: Some(1024),
        n: 1,
        output_format: Some("png".into()),
        input_images: vec![ImageInput {
            data: sample.to_vec(),
            mime_type: "image/png".into(),
            filename: "generated-concept.png".into(),
        }],
        ..Default::default()
    };
    let pet = tokio::time::timeout(std::time::Duration::from_secs(240), generate_image_with_options("azure", "Extract the orange kitten's identity from the reference into one adorable full-body chibi desktop pet, centered, clean pale mint background. No interface, text, objects, or other animals.", &config, &options)).await??;
    assert_eq!(pet.len(), 1);
    assert!(pet[0].data.starts_with(b"\x89PNG\r\n\x1a\n"));
    std::fs::write(output.join("pet.png"), &pet[0].data)?;
    options.width = Some(1536);
    options.height = Some(1024);
    options.input_images = vec![ImageInput {
        data: pet[0].data.clone(),
        mime_type: pet[0].mime_type.clone(),
        filename: "pet.png".into(),
    }];
    let wallpaper = tokio::time::timeout(std::time::Duration::from_secs(240), generate_image_with_options("azure", "An environment-only cozy woodland home wallpaper matching the reference kitten's soft 3D style and palette. Do not depict animals or characters. Quiet center and lower-right space for a separate floating desktop pet. Landscape, no text or watermark.", &config, &options)).await??;
    assert_eq!(wallpaper.len(), 1);
    assert!(wallpaper[0].data.starts_with(b"\x89PNG\r\n\x1a\n"));
    std::fs::write(output.join("wallpaper.png"), &wallpaper[0].data)?;
    println!(
        "Azure gpt-image-2: pet and referenced wallpaper saved to {}",
        output.display()
    );
    Ok(())
}
