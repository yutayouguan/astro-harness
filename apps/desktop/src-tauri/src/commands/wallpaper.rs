//! 全局壁纸资产：安全导入用户图片，或复用已配置的图片 Provider 生成后原子落盘。

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::{DynamicImage, ImageFormat, ImageReader};
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
    pub luminance: f32,
    pub recommended_theme: String,
    pub accent_color: String,
    pub secondary_color: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WallpaperAnalysisDto {
    pub luminance: f32,
    pub recommended_theme: String,
    pub accent_color: String,
    pub secondary_color: String,
}

#[derive(Debug)]
struct ValidatedImage {
    format: ImageFormat,
    width: u32,
    height: u32,
    analysis: WallpaperAnalysisDto,
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

#[derive(Default)]
struct ColorBucket {
    count: u32,
    red: u64,
    green: u64,
    blue: u64,
}

fn rgb_to_hsl([red, green, blue]: [u8; 3]) -> (f32, f32, f32) {
    let red = f32::from(red) / 255.0;
    let green = f32::from(green) / 255.0;
    let blue = f32::from(blue) / 255.0;
    let max = red.max(green).max(blue);
    let min = red.min(green).min(blue);
    let delta = max - min;
    let lightness = (max + min) / 2.0;
    if delta <= f32::EPSILON {
        return (0.0, 0.0, lightness);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if max == red {
        60.0 * (((green - blue) / delta) % 6.0)
    } else if max == green {
        60.0 * ((blue - red) / delta + 2.0)
    } else {
        60.0 * ((red - green) / delta + 4.0)
    };
    (
        if hue < 0.0 { hue + 360.0 } else { hue },
        saturation,
        lightness,
    )
}

fn hsl_to_rgb(hue: f32, saturation: f32, lightness: f32) -> [u8; 3] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let section = (hue.rem_euclid(360.0)) / 60.0;
    let x = chroma * (1.0 - (section % 2.0 - 1.0).abs());
    let (red, green, blue) = match section.floor() as u8 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let offset = lightness - chroma / 2.0;
    [red, green, blue].map(|channel| ((channel + offset) * 255.0).round() as u8)
}

fn normalized_accent(rgb: [u8; 3], dark_theme: bool) -> String {
    let (sample_hue, sample_saturation, _) = rgb_to_hsl(rgb);
    let low_saturation = sample_saturation < 0.18;
    let saturation = if low_saturation {
        0.68
    } else {
        sample_saturation.clamp(0.5, 0.86)
    };
    let hue = if low_saturation { 215.0 } else { sample_hue };
    let lightness = if dark_theme { 0.62 } else { 0.48 };
    let [red, green, blue] = hsl_to_rgb(hue, saturation, lightness);
    format!("#{red:02x}{green:02x}{blue:02x}")
}

fn hue_distance(left: f32, right: f32) -> f32 {
    let distance = (left - right).abs();
    distance.min(360.0 - distance)
}

fn extract_palette(image: &DynamicImage, dark_theme: bool) -> (String, String) {
    let thumbnail = image.thumbnail(64, 64).to_rgba8();
    let mut buckets: HashMap<u16, ColorBucket> = HashMap::new();
    for pixel in thumbnail.pixels() {
        let alpha = f32::from(pixel[3]) / 255.0;
        if alpha < 0.1 {
            continue;
        }
        let blend = |channel: u8| (f32::from(channel) * alpha + 128.0 * (1.0 - alpha)) as u8;
        let rgb = [blend(pixel[0]), blend(pixel[1]), blend(pixel[2])];
        let (_, saturation, lightness) = rgb_to_hsl(rgb);
        if !(0.05..=0.95).contains(&lightness) || saturation < 0.08 {
            continue;
        }
        let key =
            (u16::from(rgb[0] / 32) << 6) | (u16::from(rgb[1] / 32) << 3) | u16::from(rgb[2] / 32);
        let bucket = buckets.entry(key).or_default();
        bucket.count += 1;
        bucket.red += u64::from(rgb[0]);
        bucket.green += u64::from(rgb[1]);
        bucket.blue += u64::from(rgb[2]);
    }

    let mut candidates = buckets
        .into_values()
        .filter(|bucket| bucket.count > 0)
        .map(|bucket| {
            let rgb = [
                (bucket.red / u64::from(bucket.count)) as u8,
                (bucket.green / u64::from(bucket.count)) as u8,
                (bucket.blue / u64::from(bucket.count)) as u8,
            ];
            let (hue, saturation, lightness) = rgb_to_hsl(rgb);
            let middle_weight = 1.0 - (lightness - 0.5).abs() * 0.7;
            let score = bucket.count as f32 * (0.35 + saturation * 1.65) * middle_weight;
            (score, hue, rgb)
        })
        .collect::<Vec<_>>();
    candidates.sort_by(|left, right| right.0.total_cmp(&left.0));

    let primary = candidates
        .first()
        .map(|candidate| candidate.2)
        .unwrap_or([37, 99, 235]);
    let primary_hue = rgb_to_hsl(primary).0;
    let secondary = candidates
        .iter()
        .skip(1)
        .find(|candidate| hue_distance(primary_hue, candidate.1) >= 28.0)
        .map(|candidate| candidate.2)
        .unwrap_or_else(|| hsl_to_rgb((primary_hue + 42.0) % 360.0, 0.68, 0.56));

    (
        normalized_accent(primary, dark_theme),
        normalized_accent(secondary, dark_theme),
    )
}

fn analyze_image(image: &DynamicImage) -> WallpaperAnalysisDto {
    let thumbnail = image.thumbnail(64, 64).to_rgba8();
    let mut samples = thumbnail
        .pixels()
        .map(|pixel| {
            let alpha = f32::from(pixel[3]) / 255.0;
            let blend = |channel: u8| f32::from(channel) * alpha + 128.0 * (1.0 - alpha);
            let red = blend(pixel[0]);
            let green = blend(pixel[1]);
            let blue = blend(pixel[2]);
            (0.2126 * red + 0.7152 * green + 0.0722 * blue) / 255.0
        })
        .collect::<Vec<_>>();
    samples.sort_by(f32::total_cmp);
    let mean = samples.iter().sum::<f32>() / samples.len().max(1) as f32;
    let median = samples.get(samples.len() / 2).copied().unwrap_or(0.5);
    let luminance = (mean * 0.65 + median * 0.35).clamp(0.0, 1.0);
    let dark_theme = luminance < 0.46;
    let (accent_color, secondary_color) = extract_palette(image, dark_theme);
    WallpaperAnalysisDto {
        luminance,
        recommended_theme: if dark_theme { "dark" } else { "light" }.to_string(),
        accent_color,
        secondary_color,
    }
}

fn validate_image(bytes: &[u8]) -> Result<ValidatedImage, String> {
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
    Ok(ValidatedImage {
        format,
        width: decoded.width(),
        height: decoded.height(),
        analysis: analyze_image(&decoded),
    })
}

fn store_wallpaper_at(
    base: &Path,
    bytes: &[u8],
    display_name: String,
    source: &str,
    provider: Option<String>,
    model: Option<String>,
) -> Result<WallpaperAssetDto, String> {
    let validated = validate_image(bytes)?;
    let (ext, _) = supported_format(validated.format).expect("validated wallpaper format");
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
        width: validated.width,
        height: validated.height,
        created_at: chrono::Utc::now().to_rfc3339(),
        luminance: validated.analysis.luminance,
        recommended_theme: validated.analysis.recommended_theme,
        accent_color: validated.analysis.accent_color,
        secondary_color: validated.analysis.secondary_color,
        provider,
        model,
    })
}

fn analyze_wallpaper_at(base: &Path, path: &Path) -> Result<WallpaperAnalysisDto, String> {
    let root = wallpapers_dir_at(base)
        .canonicalize()
        .map_err(|_| "壁纸目录不存在".to_string())?;
    let target = path
        .canonicalize()
        .map_err(|_| "壁纸文件不存在".to_string())?;
    if !target.starts_with(&root) || !target.is_file() {
        return Err("只能分析 Astro 壁纸目录中的图片".to_string());
    }
    let metadata = fs::metadata(&target).map_err(|e| format!("无法读取壁纸信息：{e}"))?;
    if metadata.len() > MAX_WALLPAPER_BYTES {
        return Err("图片不能超过 25 MB".to_string());
    }
    let bytes = fs::read(&target).map_err(|e| format!("无法读取壁纸：{e}"))?;
    Ok(validate_image(&bytes)?.analysis)
}

#[tauri::command]
pub async fn analyze_wallpaper(path: String) -> Result<WallpaperAnalysisDto, String> {
    analyze_wallpaper_at(&home::default_memory_dir(), Path::new(path.trim()))
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
        assert_eq!(asset.recommended_theme, "dark");
        assert!(asset.luminance < 0.1);
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
    fn luminance_recommends_matching_theme() {
        let dark = DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 16, Rgba([8, 12, 20, 255])));
        let light =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(16, 16, Rgba([245, 248, 252, 255])));
        let dark_analysis = analyze_image(&dark);
        let light_analysis = analyze_image(&light);
        assert_eq!(dark_analysis.recommended_theme, "dark");
        assert_eq!(light_analysis.recommended_theme, "light");
        assert!(dark_analysis.luminance < light_analysis.luminance);
        assert!(dark_analysis.accent_color.starts_with('#'));
        assert_eq!(dark_analysis.accent_color.len(), 7);
        assert!(light_analysis.secondary_color.starts_with('#'));
    }

    #[test]
    fn analysis_only_reads_owned_wallpapers() {
        let temp = tempfile::tempdir().unwrap();
        let asset = store_wallpaper_at(
            temp.path(),
            &tiny_png(),
            "owned.png".to_string(),
            "upload",
            None,
            None,
        )
        .unwrap();
        let owned = analyze_wallpaper_at(temp.path(), Path::new(&asset.path)).unwrap();
        assert_eq!(owned.recommended_theme, "dark");

        let outside = temp.path().join("outside.png");
        fs::write(&outside, tiny_png()).unwrap();
        assert!(analyze_wallpaper_at(temp.path(), &outside).is_err());
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
