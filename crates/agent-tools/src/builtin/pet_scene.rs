//! Shared validated wallpaper attachment for Desktop and chat scene workflows.
use std::{io::Cursor, path::Path};
use types::{
    pet_scene::{atomic_write, PetScene},
    UiStyleManifest, UiStyleWallpaper,
};

pub fn attach_wallpaper(
    base: &Path,
    expected: &PetScene,
    bytes: &[u8],
    source: Option<String>,
) -> anyhow::Result<types::DesktopPetState> {
    expected.validate()?;
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() <= 25 * 1024 * 1024,
        "壁纸为空或超过 25 MB"
    );
    let mut reader = image::ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    let extension = match reader.format() {
        Some(image::ImageFormat::Png) => "png",
        Some(image::ImageFormat::Jpeg) => "jpg",
        Some(image::ImageFormat::WebP) => "webp",
        Some(image::ImageFormat::Bmp) => "bmp",
        _ => anyhow::bail!("不支持的壁纸格式"),
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(128 * 1024 * 1024);
    reader.limits(limits);
    reader.decode()?;
    let root = types::ui_style_root(base);
    let revision = uuid::Uuid::new_v4().simple().to_string();
    let relative = format!("themes/{}/wallpaper-{revision}.{extension}", expected.id);
    let path = root.join(&relative);
    let style = UiStyleManifest {
        schema_version: types::UI_STYLE_SCHEMA_VERSION,
        id: expected.id.clone(),
        name: expected.name.clone(),
        revision,
        updated_at: chrono::Utc::now().to_rfc3339(),
        tokens: Default::default(),
        icons: Default::default(),
        wallpaper: Some(UiStyleWallpaper {
            path: relative,
            fit: Default::default(),
            shade: 18,
            blur: 0,
            adaptive_color: true,
            recommended_theme: None,
            accent_color: None,
            secondary_color: None,
        }),
    };
    style.validate().map_err(anyhow::Error::msg)?;
    atomic_write(&path, bytes)?;
    let updated = types::update_desktop_pet_state(base, |state| {
        let index = state
            .scenes
            .iter()
            .position(|s| s.id == expected.id)
            .ok_or_else(|| anyhow::anyhow!("场景已不存在"))?;
        let scene = &state.scenes[index];
        anyhow::ensure!(
            scene == expected,
            "生成期间场景已更新，未覆盖新绑定；请重试"
        );
        atomic_write(
            &root.join("themes").join(&scene.id).join("theme.json"),
            &serde_json::to_vec_pretty(&style)?,
        )?;
        let mut scene = state.scenes.remove(index);
        scene.style = Some(style);
        scene.wallpaper_source_path = source;
        // Most recently bound scene wins for a shared original wallpaper.
        state.scenes.push(scene);
        Ok(())
    });
    if updated.is_err() {
        let _ = std::fs::remove_file(path);
    }
    updated
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pet_scene_failed_and_stale_wallpapers_preserve_saved_pet() {
        let dir = tempfile::tempdir().unwrap();
        let pet_path = dir.path().join("ui/pet.png");
        atomic_write(&pet_path, b"already validated pet").unwrap();
        let scene = PetScene {
            id: "pet-test".into(),
            name: "Forest".into(),
            pet: types::pet_scene::PetIdentity {
                pet_path: pet_path.to_string_lossy().into_owned(),
                source_path: None,
                sprite_version_number: None,
                display_name: None,
                description: None,
                provider: None,
                model: None,
            },
            style: None,
            wallpaper_source_path: None,
        };
        let saved = types::pet_scene::save_scene(dir.path(), scene.clone()).unwrap();
        assert!(attach_wallpaper(dir.path(), &scene, b"not an image", None).is_err());
        assert_eq!(types::read_desktop_pet_state(dir.path()).unwrap(), saved);
        let image = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            8,
            6,
            image::Rgba([120, 140, 160, 255]),
        ));
        let mut png = Cursor::new(Vec::new());
        image.write_to(&mut png, image::ImageFormat::Png).unwrap();
        let updated = attach_wallpaper(dir.path(), &scene, png.get_ref(), None).unwrap();
        assert!(updated.scenes[0].style.is_some());
        assert!(
            updated.pet_path.is_none(),
            "attachment is still preview-only"
        );
        assert!(attach_wallpaper(dir.path(), &scene, png.get_ref(), None).is_err());
        assert_eq!(types::read_desktop_pet_state(dir.path()).unwrap(), updated);
    }
}
