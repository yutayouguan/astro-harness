//! 桌面宠物：参考图导入、AI 生成、持久化与独立透明窗口。

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::{ImageFormat, ImageReader};
use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, Runtime, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder,
};

const PET_WINDOW_LABEL: &str = "desktop-pet";
const PET_EVENT: &str = "desktop-pet-changed";
const MAX_SOURCE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_SOURCE_DIMENSION: u32 = 8192;
const MAX_DECODE_ALLOC: u64 = 128 * 1024 * 1024;
const BASE_WINDOW_WIDTH: f64 = 300.0;
const BASE_WINDOW_HEIGHT: f64 = 340.0;

pub type DesktopPetStateDto = types::DesktopPetState;

struct ValidatedPetImage {
    bytes: Vec<u8>,
    extension: &'static str,
    mime: &'static str,
}

fn pet_dir_at(base: &Path) -> PathBuf {
    types::desktop_pet_root(base)
}

fn load_state_at(base: &Path) -> DesktopPetStateDto {
    types::read_desktop_pet_state(base).unwrap_or_default()
}

fn load_state() -> DesktopPetStateDto {
    load_state_at(&home::default_memory_dir())
}

#[cfg(test)]
fn save_state_at(base: &Path, state: &DesktopPetStateDto) -> Result<(), String> {
    types::write_desktop_pet_state(base, state)
        .map_err(|error| format!("无法保存桌宠设置：{error}"))
}

fn validate_image_bytes(bytes: Vec<u8>) -> Result<ValidatedPetImage, String> {
    if bytes.is_empty() {
        return Err("宠物照片为空".to_string());
    }
    if bytes.len() as u64 > MAX_SOURCE_BYTES {
        return Err("宠物照片不能超过 25 MB".to_string());
    }
    let mut reader = ImageReader::new(Cursor::new(&bytes))
        .with_guessed_format()
        .map_err(|_| "无法识别宠物照片格式".to_string())?;
    let format = reader
        .format()
        .ok_or_else(|| "无法识别宠物照片格式".to_string())?;
    let (extension, mime) = match format {
        ImageFormat::Png => ("png", "image/png"),
        ImageFormat::Jpeg => ("jpg", "image/jpeg"),
        ImageFormat::WebP => ("webp", "image/webp"),
        _ => return Err("仅支持 PNG、JPEG 和 WebP 宠物照片".to_string()),
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_SOURCE_DIMENSION);
    limits.max_image_height = Some(MAX_SOURCE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| "宠物照片已损坏或尺寸过大".to_string())?;
    Ok(ValidatedPetImage {
        bytes,
        extension,
        mime,
    })
}

fn validate_source_path(source_path: &str) -> Result<ValidatedPetImage, String> {
    let source = PathBuf::from(source_path.trim());
    let metadata = fs::metadata(&source).map_err(|_| "找不到所选宠物照片".to_string())?;
    if !metadata.is_file() {
        return Err("请选择宠物图片文件".to_string());
    }
    if metadata.len() > MAX_SOURCE_BYTES {
        return Err("宠物照片不能超过 25 MB".to_string());
    }
    let bytes = fs::read(&source).map_err(|error| format!("无法读取宠物照片：{error}"))?;
    validate_image_bytes(bytes)
}

fn store_asset(
    base: &Path,
    prefix: &str,
    bytes: &[u8],
    extension: &str,
) -> Result<PathBuf, String> {
    let dir = pet_dir_at(base);
    fs::create_dir_all(&dir).map_err(|error| format!("无法创建桌宠目录：{error}"))?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let path = dir.join(format!("{prefix}-{id}.{extension}"));
    let temporary = dir.join(format!(".{prefix}-{id}.tmp"));
    fs::write(&temporary, bytes).map_err(|error| format!("无法写入桌宠图片：{error}"))?;
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法保存桌宠图片：{error}"));
    }
    Ok(path)
}

fn remove_uniform_edge_background(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let image = image::load_from_memory(bytes)
        .map_err(|_| "生成的桌宠图片无法解码".to_string())?
        .to_rgba8();
    let (width, height) = image.dimensions();
    if width == 0 || height == 0 {
        return Err("生成的桌宠图片尺寸无效".to_string());
    }
    if image.pixels().any(|pixel| pixel[3] < 250) {
        return Ok(bytes.to_vec());
    }
    let corners = [
        image.get_pixel(0, 0),
        image.get_pixel(width - 1, 0),
        image.get_pixel(0, height - 1),
        image.get_pixel(width - 1, height - 1),
    ];
    let background = [
        corners.iter().map(|pixel| u32::from(pixel[0])).sum::<u32>() as f32 / 4.0,
        corners.iter().map(|pixel| u32::from(pixel[1])).sum::<u32>() as f32 / 4.0,
        corners.iter().map(|pixel| u32::from(pixel[2])).sum::<u32>() as f32 / 4.0,
    ];
    let distance = |pixel: &image::Rgba<u8>| {
        let red = f32::from(pixel[0]) - background[0];
        let green = f32::from(pixel[1]) - background[1];
        let blue = f32::from(pixel[2]) - background[2];
        (red * red + green * green + blue * blue).sqrt()
    };
    if corners.iter().any(|pixel| distance(pixel) > 36.0) {
        return Ok(bytes.to_vec());
    }

    let mut output = image;
    for pixel in output.pixels_mut() {
        let alpha = ((distance(pixel) - 22.0) / 44.0).clamp(0.0, 1.0);
        pixel[3] = (alpha * 255.0).round() as u8;
    }
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(output)
        .write_to(&mut encoded, ImageFormat::Png)
        .map_err(|error| format!("无法保存透明桌宠图片：{error}"))?;
    Ok(encoded.into_inner())
}

fn window_size(scale: f64) -> LogicalSize<f64> {
    LogicalSize::new(BASE_WINDOW_WIDTH * scale, BASE_WINDOW_HEIGHT * scale)
}

fn place_near_bottom_right<R: Runtime>(window: &WebviewWindow<R>) {
    let Ok(Some(monitor)) = window.current_monitor() else {
        return;
    };
    let monitor_size = monitor.size();
    let monitor_position = monitor.position();
    let Ok(window_size) = window.outer_size() else {
        return;
    };
    let x = monitor_position.x
        + i32::try_from(monitor_size.width.saturating_sub(window_size.width + 28)).unwrap_or(0);
    let y = monitor_position.y
        + i32::try_from(monitor_size.height.saturating_sub(window_size.height + 72)).unwrap_or(0);
    let _ = window.set_position(PhysicalPosition::new(x, y));
}

fn ensure_window<R: Runtime>(app: &AppHandle<R>, state: &DesktopPetStateDto) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(PET_WINDOW_LABEL) {
        window
            .set_always_on_top(state.always_on_top)
            .map_err(|error| error.to_string())?;
        window
            .set_size(window_size(state.scale))
            .map_err(|error| error.to_string())?;
        if state.enabled {
            window.show().map_err(|error| error.to_string())?;
        } else {
            window.hide().map_err(|error| error.to_string())?;
        }
        return Ok(());
    }
    if !state.enabled {
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        PET_WINDOW_LABEL,
        WebviewUrl::App("index.html?surface=desktop-pet".into()),
    )
    .title("Astro Desktop Pet")
    .inner_size(
        window_size(state.scale).width,
        window_size(state.scale).height,
    )
    .resizable(false)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(state.always_on_top)
    .skip_taskbar(true)
    .focused(false)
    .visible(true)
    .build()
    .map_err(|error| format!("无法创建桌宠窗口：{error}"))?;
    place_near_bottom_right(&window);
    Ok(())
}

fn emit_state<R: Runtime>(app: &AppHandle<R>, state: &DesktopPetStateDto) {
    let _ = app.emit_to(PET_WINDOW_LABEL, PET_EVENT, state);
    let _ = app.emit_to("main", PET_EVENT, state);
}

pub fn restore_window<R: Runtime>(app: &AppHandle<R>) {
    let state = load_state();
    if let Err(error) = ensure_window(app, &state) {
        tracing::warn!(%error, "restore desktop pet window failed");
    }
}

fn refresh_from_disk<R: Runtime>(app: &AppHandle<R>) {
    let state = load_state();
    if let Err(error) = ensure_window(app, &state) {
        tracing::warn!(%error, "refresh desktop pet window failed");
        return;
    }
    emit_state(app, &state);
}

fn schedule_refresh(app: &AppHandle) {
    let scheduler = app.clone();
    let callback_app = scheduler.clone();
    if let Err(error) = scheduler.run_on_main_thread(move || refresh_from_disk(&callback_app)) {
        tracing::warn!(%error, "schedule desktop pet refresh failed");
    }
}

/// Bridge Agent-tool state writes into the native window. The callback is immediate for the
/// embedded backend; the file watcher covers `ASTRO_EMBED_BACKEND=0` and external writers.
pub fn install_change_bridge(app: &AppHandle) {
    let callback_app = app.clone();
    types::set_desktop_pet_change_handler(Arc::new(move || {
        schedule_refresh(&callback_app);
    }));

    let watcher_app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut previous = load_state().updated_at;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let current = load_state().updated_at;
            if current == previous {
                continue;
            }
            previous = current;
            schedule_refresh(&watcher_app);
        }
    });
}

#[tauri::command]
pub fn get_desktop_pet_state() -> DesktopPetStateDto {
    load_state()
}

#[tauri::command]
pub fn import_desktop_pet_photo(
    app: AppHandle,
    source_path: String,
) -> Result<DesktopPetStateDto, String> {
    let image = validate_source_path(&source_path)?;
    let base = home::default_memory_dir();
    let path = store_asset(&base, "source", &image.bytes, image.extension)?;
    let path = path.to_string_lossy().into_owned();
    let state = types::update_desktop_pet_state(&base, |state| {
        state.source_path = Some(path);
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    })
    .map_err(|error| format!("无法保存桌宠设置：{error}"))?;
    emit_state(&app, &state);
    Ok(state)
}

#[tauri::command]
pub async fn generate_desktop_pet(
    app: AppHandle,
    description: Option<String>,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    let state = load_state_at(&base);
    let source_path = state
        .source_path
        .clone()
        .ok_or_else(|| "请先上传宠物照片".to_string())?;
    let image = validate_source_path(&source_path)?;
    let details = description.unwrap_or_default();
    if details.chars().count() > 2_000 {
        return Err("桌宠风格描述不能超过 2000 个字符".to_string());
    }
    let prompt = format!(
        "Use the uploaded pet photo as the only identity reference. Create one adorable personalized desktop companion that clearly preserves the pet's species, face shape, coat colors, markings, eye color, and distinctive features. Render a polished soft 3D chibi character, full body, centered, facing slightly toward the viewer, with clean readable silhouette and fine fur detail. Use a plain pale mint background with no scenery and no ground shadow so the subject can be isolated for a desktop overlay. No text, logo, watermark, accessories not present in the reference, extra animals, duplicate limbs, or distorted anatomy. User preference: {}",
        if details.trim().is_empty() {
            "warm, friendly, calm expression"
        } else {
            details.trim()
        }
    );
    let filename = Path::new(&source_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("pet-reference.png");
    let generated = crate::commands::chat::generate_image_data_with_reference(
        &prompt,
        1024,
        1024,
        Some((&image.bytes, image.mime, filename)),
    )
    .await?;
    let generated_image = validate_image_bytes(generated.data)?;
    let transparent = remove_uniform_edge_background(&generated_image.bytes)?;
    let pet_path = store_asset(&base, "pet", &transparent, "png")?;
    let pet_path_string = pet_path.to_string_lossy().into_owned();
    let updated = types::update_desktop_pet_state(&base, |state| {
        anyhow::ensure!(
            state.source_path.as_deref() == Some(source_path.as_str()),
            "生成期间宠物照片已更换，请重新生成"
        );
        state.pet_path = Some(pet_path_string);
        state.enabled = true;
        state.provider = Some(generated.provider);
        state.model = Some(generated.model);
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    });
    let state = match updated {
        Ok(state) => state,
        Err(error) => {
            let _ = fs::remove_file(&pet_path);
            return Err(error.to_string());
        }
    };
    ensure_window(&app, &state)?;
    emit_state(&app, &state);
    Ok(state)
}

#[tauri::command]
pub fn set_desktop_pet_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    let state = types::update_desktop_pet_state(&base, |state| {
        state.enabled = enabled;
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    })
    .map_err(|error| format!("无法保存桌宠设置：{error}"))?;
    ensure_window(&app, &state)?;
    emit_state(&app, &state);
    Ok(state)
}

#[tauri::command]
pub fn set_desktop_pet_scale(app: AppHandle, scale: f64) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    let state = types::update_desktop_pet_state(&base, |state| {
        state.scale = scale.clamp(0.65, 1.35);
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    })
    .map_err(|error| format!("无法保存桌宠设置：{error}"))?;
    ensure_window(&app, &state)?;
    emit_state(&app, &state);
    Ok(state)
}

#[tauri::command]
pub fn set_desktop_pet_always_on_top(
    app: AppHandle,
    always_on_top: bool,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    let state = types::update_desktop_pet_state(&base, |state| {
        state.always_on_top = always_on_top;
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    })
    .map_err(|error| format!("无法保存桌宠设置：{error}"))?;
    ensure_window(&app, &state)?;
    emit_state(&app, &state);
    Ok(state)
}

#[tauri::command]
pub fn open_desktop_pet_main(app: AppHandle) {
    crate::ui::tray::show_main_window(&app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};

    fn tiny_png() -> Vec<u8> {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba([1, 2, 3, 255])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::Png).unwrap();
        bytes.into_inner()
    }

    #[test]
    fn validates_and_stores_pet_assets_inside_app_home() {
        let temp = tempfile::tempdir().unwrap();
        let image = validate_image_bytes(tiny_png()).unwrap();
        let path = store_asset(temp.path(), "source", &image.bytes, image.extension).unwrap();
        assert!(path.starts_with(pet_dir_at(temp.path())));
        assert!(path.is_file());
        assert_eq!(image.mime, "image/png");
    }

    #[test]
    fn persists_desktop_pet_state() {
        let temp = tempfile::tempdir().unwrap();
        let state = DesktopPetStateDto {
            enabled: true,
            scale: 1.2,
            pet_path: Some("/tmp/pet.png".to_string()),
            ..DesktopPetStateDto::default()
        };
        save_state_at(temp.path(), &state).unwrap();
        let loaded = load_state_at(temp.path());
        assert!(loaded.enabled);
        assert_eq!(loaded.scale, 1.2);
        assert_eq!(loaded.pet_path.as_deref(), Some("/tmp/pet.png"));
    }

    #[test]
    fn removes_a_uniform_generated_background() {
        let mut image = RgbaImage::from_pixel(12, 12, Rgba([220, 250, 235, 255]));
        for y in 4..8 {
            for x in 4..8 {
                image.put_pixel(x, y, Rgba([120, 65, 35, 255]));
            }
        }
        let mut encoded = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut encoded, ImageFormat::Png)
            .unwrap();
        let output = image::load_from_memory(
            &remove_uniform_edge_background(&encoded.into_inner()).unwrap(),
        )
        .unwrap()
        .to_rgba8();
        assert_eq!(output.get_pixel(0, 0)[3], 0);
        assert_eq!(output.get_pixel(6, 6)[3], 255);
    }
}
