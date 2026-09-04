//! 全局壁纸资产：安全导入用户图片，或复用已配置的图片 Provider 生成后原子落盘。

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{ImageFormat, ImageReader};
use serde::Serialize;

const MAX_WALLPAPER_BYTES: u64 = 25 * 1024 * 1024;
const MAX_WALLPAPER_DIMENSION: u32 = 16_384;
const MAX_DECODE_ALLOC: u64 = 128 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperAssetDto {
    pub id: String,
    pub path: String,
    pub name: String,
    pub source: String,
    pub width: u32,
    pub height: u32,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

fn wallpapers_dir_at(base: &Path) -> PathBuf {
    base.join("ui").join("wallpapers")
}

fn supported_format(format: ImageFormat) -> Option<(&'static str, &'static str)> {
    match format {
        ImageFormat::Png => Some(("png", "image/png")),
        ImageFormat::Jpeg => Some(("jpg", "image/jpeg")),
        ImageFormat::WebP => Some(("webp", "image/webp")),
        _ => None,
    }
}

fn validate_image(bytes: &[u8]) -> Result<(ImageFormat, u32, u32), String> {
    if bytes.is_empty() {
        return Err("图片文件为空".to_string());
    }
    if bytes.len() as u64 > MAX_WALLPAPER_BYTES {
        return Err("图片不能超过 25 MB".to_string());
    }

    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "无法识别图片格式".to_string())?;
    let format = reader
        .format()
        .filter(|format| supported_format(*format).is_some())
        .ok_or_else(|| "仅支持 PNG、JPEG 和 WebP 图片".to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_WALLPAPER_DIMENSION);
    limits.max_image_height = Some(MAX_WALLPAPER_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let decoded = reader
        .decode()
        .map_err(|_| "图片已损坏或尺寸过大".to_string())?;
    Ok((format, decoded.width(), decoded.height()))
}

fn store_wallpaper_at(
    base: &Path,
    bytes: &[u8],
    display_name: String,
    source: &str,
    provider: Option<String>,
    model: Option<String>,
) -> Result<WallpaperAssetDto, String> {
    let (format, width, height) = validate_image(bytes)?;
    let (ext, _) = supported_format(format).expect("validated wallpaper format");
    let id = uuid::Uuid::new_v4().simple().to_string();
    let dir = wallpapers_dir_at(base);
    fs::create_dir_all(&dir).map_err(|e| format!("无法创建壁纸目录：{e}"))?;
    let path = dir.join(format!("wallpaper-{id}.{ext}"));
    let temp = dir.join(format!(".wallpaper-{id}.tmp"));

    fs::write(&temp, bytes).map_err(|e| format!("无法写入壁纸：{e}"))?;
    if let Err(error) = fs::rename(&temp, &path) {
        let _ = fs::remove_file(&temp);
        return Err(format!("无法保存壁纸：{error}"));
    }

    Ok(WallpaperAssetDto {
        id,
        path: path.to_string_lossy().into_owned(),
        name: display_name,
        source: source.to_string(),
        width,
        height,
        created_at: chrono::Utc::now().to_rfc3339(),
        provider,
        model,
    })
}

fn validate_prompt(prompt: &str) -> Result<&str, String> {
    let prompt = prompt.trim();
    if prompt.is_empty() {
        return Err("请描述你想要的壁纸".to_string());
    }
    if prompt.chars().count() > 8_000 {
        return Err("壁纸描述不能超过 8000 个字符".to_string());
    }
    Ok(prompt)
}

#[tauri::command]
pub async fn import_wallpaper(source_path: String) -> Result<WallpaperAssetDto, String> {
    let source = PathBuf::from(source_path.trim());
    let metadata = fs::metadata(&source).map_err(|_| "找不到所选图片".to_string())?;
    if !metadata.is_file() {
        return Err("请选择图片文件".to_string());
    }
    if metadata.len() > MAX_WALLPAPER_BYTES {
        return Err("图片不能超过 25 MB".to_string());
    }
    let bytes = fs::read(&source).map_err(|e| format!("无法读取所选图片：{e}"))?;
    let name = source
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("自定义壁纸")
        .to_string();
    store_wallpaper_at(
        &home::default_memory_dir(),
        &bytes,
        name,
        "upload",
        None,
        None,
    )
}

#[tauri::command]
pub async fn generate_wallpaper(prompt: String) -> Result<WallpaperAssetDto, String> {
    let prompt = validate_prompt(&prompt)?;
    let generated = super::chat::generate_image_data(prompt, 1536, 1024).await?;
    let name = format!("AI 壁纸 {}", chrono::Local::now().format("%Y-%m-%d %H:%M"));
    store_wallpaper_at(
        &home::default_memory_dir(),
        &generated.data,
        name,
        "ai",
        Some(generated.provider),
        Some(generated.model),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};

    fn tiny_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 3, Rgba([1, 2, 3, 255])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn validates_and_stores_wallpaper_inside_app_home() {
        let temp = tempfile::tempdir().unwrap();
        let asset = store_wallpaper_at(
            temp.path(),
            &tiny_png(),
            "test.png".to_string(),
            "upload",
            None,
            None,
        )
        .unwrap();
        assert_eq!((asset.width, asset.height), (4, 3));
        assert_eq!(asset.source, "upload");
        let path = PathBuf::from(asset.path);
        assert!(path.starts_with(wallpapers_dir_at(temp.path())));
        assert!(path.is_file());
    }

    #[test]
    fn rejects_non_image_bytes() {
        let error = validate_image(b"not an image").unwrap_err();
        assert!(error.contains("图片"));
    }

    #[test]
    fn failed_validation_does_not_create_wallpaper_directory() {
        let temp = tempfile::tempdir().unwrap();
        let error = store_wallpaper_at(
            temp.path(),
            b"bad",
            "bad.png".to_string(),
            "upload",
            None,
            None,
        )
        .unwrap_err();
        assert!(!error.is_empty());
        assert!(!wallpapers_dir_at(temp.path()).exists());
    }

    #[test]
    fn temp_files_are_not_left_after_success() {
        let temp = tempfile::tempdir().unwrap();
        store_wallpaper_at(
            temp.path(),
            &tiny_png(),
            "test.png".to_string(),
            "upload",
            None,
            None,
        )
        .unwrap();
        let entries = fs::read_dir(wallpapers_dir_at(temp.path()))
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        assert_eq!(entries.len(), 1);
        assert!(!entries[0].starts_with('.'));
    }

    #[test]
    fn prompt_validation_rejects_blank_and_oversized_inputs() {
        assert!(validate_prompt("  ").is_err());
        assert_eq!(validate_prompt(" landscape ").unwrap(), "landscape");
        assert!(validate_prompt(&"x".repeat(8_001)).is_err());
    }
}
