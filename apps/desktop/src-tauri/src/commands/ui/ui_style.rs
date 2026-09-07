//! 文件态用户界面样式：Desktop 只读取已校验清单，并在无效时回退系统样式。

use std::fs;
use std::path::Path;

use types::{active_ui_style_path, read_active_ui_style, ui_style_root, UiStyleManifest};

fn active_style_at(base: &Path) -> Result<Option<UiStyleManifest>, String> {
    let Some(mut manifest) = read_active_ui_style(base)? else {
        return Ok(None);
    };
    if let Some(wallpaper) = manifest.wallpaper.as_mut() {
        let root = ui_style_root(base)
            .canonicalize()
            .map_err(|_| "界面样式目录不存在".to_string())?;
        let path = root.join(&wallpaper.path);
        let canonical = path
            .canonicalize()
            .map_err(|_| "界面样式壁纸不存在".to_string())?;
        if !canonical.starts_with(&root) || !canonical.is_file() {
            return Err("界面样式壁纸不在受控目录内".to_string());
        }
        let analysis = super::wallpaper::analyze_wallpaper_at(base, &canonical)?;
        wallpaper.path = canonical.to_string_lossy().into_owned();
        wallpaper.recommended_theme = Some(analysis.recommended_theme);
        wallpaper.accent_color = Some(analysis.accent_color);
        wallpaper.secondary_color = Some(analysis.secondary_color);
    }
    Ok(Some(manifest))
}

fn reset_active_style_at(base: &Path) -> Result<bool, String> {
    let active = active_ui_style_path(base);
    if !active.is_file() {
        return Ok(false);
    }
    let history = ui_style_root(base).join("history");
    fs::create_dir_all(&history).map_err(|error| format!("无法创建样式历史目录：{error}"))?;
    let backup = history.join(format!(
        "{}-{}.json",
        chrono::Utc::now().format("%Y%m%dT%H%M%S"),
        uuid::Uuid::new_v4().simple()
    ));
    fs::copy(&active, &backup).map_err(|error| format!("无法备份当前样式：{error}"))?;
    fs::remove_file(&active).map_err(|error| format!("无法恢复系统样式：{error}"))?;
    Ok(true)
}

#[tauri::command]
pub async fn get_active_ui_style() -> Result<Option<UiStyleManifest>, String> {
    tokio::task::spawn_blocking(|| active_style_at(&home::default_memory_dir()))
        .await
        .map_err(|error| format!("读取界面样式任务失败：{error}"))?
}

#[tauri::command]
pub async fn reset_active_ui_style() -> Result<bool, String> {
    let reset = tokio::task::spawn_blocking(|| reset_active_style_at(&home::default_memory_dir()))
        .await
        .map_err(|error| format!("恢复系统样式任务失败：{error}"))??;
    if reset {
        types::notify_ui_style_changed();
    }
    Ok(reset)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use types::{
        UiStyleIcons, UiStyleTokens, UiStyleWallpaper, UiStyleWallpaperFit, UI_STYLE_SCHEMA_VERSION,
    };

    fn write_active(base: &Path) {
        let root = ui_style_root(base);
        let theme = root.join("themes/test");
        fs::create_dir_all(&theme).unwrap();
        DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 3, Rgba([8, 12, 20, 255])))
            .save_with_format(theme.join("wallpaper.png"), ImageFormat::Png)
            .unwrap();
        let manifest = UiStyleManifest {
            schema_version: UI_STYLE_SCHEMA_VERSION,
            id: "test".into(),
            name: "Test".into(),
            revision: "r1".into(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            tokens: UiStyleTokens {
                light: BTreeMap::new(),
                dark: BTreeMap::new(),
            },
            icons: UiStyleIcons::default(),
            wallpaper: Some(UiStyleWallpaper {
                path: "themes/test/wallpaper.png".into(),
                fit: UiStyleWallpaperFit::Cover,
                shade: 18,
                blur: 0,
                adaptive_color: true,
                recommended_theme: None,
                accent_color: None,
                secondary_color: None,
            }),
        };
        fs::write(
            active_ui_style_path(base),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn resolves_owned_wallpaper_and_enriches_palette() {
        let temp = tempfile::tempdir().unwrap();
        write_active(temp.path());
        let manifest = active_style_at(temp.path()).unwrap().unwrap();
        let wallpaper = manifest.wallpaper.unwrap();
        assert!(PathBuf::from(&wallpaper.path).is_absolute());
        assert_eq!(wallpaper.recommended_theme.as_deref(), Some("dark"));
        assert!(wallpaper.accent_color.is_some());
    }

    #[test]
    fn reset_archives_active_manifest() {
        let temp = tempfile::tempdir().unwrap();
        write_active(temp.path());
        assert!(reset_active_style_at(temp.path()).unwrap());
        assert!(!active_ui_style_path(temp.path()).exists());
        assert_eq!(
            fs::read_dir(ui_style_root(temp.path()).join("history"))
                .unwrap()
                .count(),
            1
        );
    }
}
