//! 桌面宠物：参考图导入、AI 生成、持久化与独立透明窗口。

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

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
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;
const MAX_ATLAS_BYTES: u64 = 50 * 1024 * 1024;
const BASE_WINDOW_WIDTH: f64 = 300.0;
const BASE_WINDOW_HEIGHT: f64 = 340.0;
static PET_WINDOW_SYNC: Mutex<()> = Mutex::new(());

pub type DesktopPetStateDto = types::DesktopPetState;

struct ValidatedPetImage {
    bytes: Vec<u8>,
    extension: &'static str,
    mime: &'static str,
}

fn pet_dir_at(base: &Path) -> PathBuf {
    types::desktop_pet_root(base)
}

fn load_state_at(base: &Path) -> Result<DesktopPetStateDto, String> {
    types::read_desktop_pet_state(base).map_err(|error| format!("无法读取桌宠设置：{error}"))
}

fn load_state() -> Result<DesktopPetStateDto, String> {
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
    let bytes = types::desktop_pet::read_limited_pet_file(&source, MAX_SOURCE_BYTES)
        .map_err(|error| format!("无法读取宠物照片：{error}"))?;
    validate_image_bytes(bytes)
}

fn validate_pet_id(id: &str) -> Result<&str, String> {
    let id = id.trim();
    if id.is_empty()
        || id.len() > 80
        || !id.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric)
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("宠物包 id 只能包含字母、数字、- _ .".to_string());
    }
    Ok(id)
}

fn validate_manifest_copy(value: &str, field: &str, max_chars: usize) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > max_chars || value.chars().any(char::is_control)
    {
        return Err(format!("pet.json 的 {field} 无效"));
    }
    Ok(())
}

fn validate_v2_atlas(bytes: &[u8]) -> Result<&'static str, String> {
    if bytes.is_empty() || bytes.len() as u64 > MAX_ATLAS_BYTES {
        return Err("动画图集为空或超过 50 MB".to_string());
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| "无法识别动画图集格式".to_string())?;
    let extension = match reader
        .format()
        .ok_or_else(|| "无法识别动画图集格式".to_string())?
    {
        ImageFormat::Png => "png",
        ImageFormat::WebP => "webp",
        _ => return Err("动画图集仅支持 PNG 或 WebP".to_string()),
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(types::DESKTOP_PET_V2_WIDTH);
    limits.max_image_height = Some(types::DESKTOP_PET_V2_HEIGHT);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| "无法解码动画图集".to_string())?
        .to_rgba8();
    if image.dimensions() != (types::DESKTOP_PET_V2_WIDTH, types::DESKTOP_PET_V2_HEIGHT) {
        return Err(format!(
            "v2 图集必须是 {}x{}",
            types::DESKTOP_PET_V2_WIDTH,
            types::DESKTOP_PET_V2_HEIGHT
        ));
    }
    for (row, used_columns) in types::DESKTOP_PET_V2_USED_COLUMNS.iter().enumerate() {
        for column in 0..types::DESKTOP_PET_V2_COLUMNS {
            let start_x = column * types::DESKTOP_PET_V2_CELL_WIDTH;
            let start_y = row as u32 * types::DESKTOP_PET_V2_CELL_HEIGHT;
            let populated = (start_y..start_y + types::DESKTOP_PET_V2_CELL_HEIGHT).any(|y| {
                (start_x..start_x + types::DESKTOP_PET_V2_CELL_WIDTH)
                    .any(|x| image.get_pixel(x, y)[3] != 0)
            });
            if column < *used_columns && !populated {
                return Err(format!("v2 图集第 {} 行第 {} 格为空", row + 1, column + 1));
            }
            if column >= *used_columns && populated {
                return Err(format!("v2 图集第 {} 行未使用格必须全透明", row + 1));
            }
        }
    }
    Ok(extension)
}

fn import_animated_pet_at(base: &Path, manifest_path: &Path) -> Result<DesktopPetStateDto, String> {
    let manifest_bytes =
        types::desktop_pet::read_limited_pet_file(manifest_path, MAX_MANIFEST_BYTES)
            .map_err(|error| format!("无法读取 pet.json：{error}"))?;
    let manifest: types::DesktopPetManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|error| format!("pet.json 格式无效：{error}"))?;
    let pet_id = validate_pet_id(&manifest.id)?.to_string();
    validate_manifest_copy(&manifest.display_name, "displayName", 80)?;
    validate_manifest_copy(&manifest.description, "description", 500)?;
    if manifest.sprite_version_number != types::DESKTOP_PET_V2_SPRITE_VERSION {
        return Err("仅支持 spriteVersionNumber: 2 的动画桌宠".to_string());
    }
    let manifest_parent = manifest_path
        .parent()
        .ok_or_else(|| "pet.json 缺少父目录".to_string())?
        .canonicalize()
        .map_err(|_| "pet.json 父目录不存在".to_string())?;
    let relative_sheet = Path::new(manifest.spritesheet_path.trim());
    if relative_sheet.as_os_str().is_empty() || relative_sheet.is_absolute() {
        return Err("spritesheetPath 必须是宠物包内的相对路径".to_string());
    }
    let source_sheet = manifest_parent
        .join(relative_sheet)
        .canonicalize()
        .map_err(|_| "找不到宠物动画图集".to_string())?;
    if !source_sheet.starts_with(&manifest_parent) || !source_sheet.is_file() {
        return Err("动画图集必须位于宠物包目录内".to_string());
    }
    let sheet_bytes = types::desktop_pet::read_limited_pet_file(&source_sheet, MAX_ATLAS_BYTES)
        .map_err(|error| format!("无法读取动画图集：{error}"))?;
    let extension = validate_v2_atlas(&sheet_bytes)?;

    let pets_root = pet_dir_at(base).join("pets");
    fs::create_dir_all(&pets_root).map_err(|error| format!("无法创建桌宠库：{error}"))?;
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let package_name = format!("{pet_id}-{}", &nonce[..8]);
    let temporary = pets_root.join(format!(".{package_name}.tmp"));
    let destination = pets_root.join(&package_name);
    fs::create_dir_all(&temporary).map_err(|error| format!("无法准备宠物包：{error}"))?;
    let copied_sheet_name = format!("spritesheet.{extension}");
    let copied_sheet = temporary.join(&copied_sheet_name);
    let managed_manifest = types::DesktopPetManifest {
        id: pet_id,
        display_name: manifest.display_name.trim().to_string(),
        description: manifest.description.trim().to_string(),
        spritesheet_path: copied_sheet_name,
        ..manifest
    };
    let import_result = (|| -> Result<(), String> {
        fs::write(&copied_sheet, &sheet_bytes)
            .map_err(|error| format!("无法复制动画图集：{error}"))?;
        fs::write(
            temporary.join("pet.json"),
            serde_json::to_vec_pretty(&managed_manifest).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("无法保存宠物清单：{error}"))?;
        fs::rename(&temporary, &destination).map_err(|error| format!("无法安装宠物包：{error}"))?;
        Ok(())
    })();
    if let Err(error) = import_result {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }

    let pet_path = destination.join(&managed_manifest.spritesheet_path);
    let updated = types::update_desktop_pet_state(base, |state| {
        state.pet_path = Some(pet_path.to_string_lossy().into_owned());
        state.source_path = None;
        state.follow_wallpaper = false;
        state.enabled = true;
        state.provider = None;
        state.model = None;
        state.sprite_version_number = Some(types::DESKTOP_PET_V2_SPRITE_VERSION);
        state.display_name = Some(managed_manifest.display_name.clone());
        state.description = Some(managed_manifest.description.clone());
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    });
    match updated {
        Ok(state) => Ok(state),
        Err(error) => {
            let _ = fs::remove_dir_all(&destination);
            Err(format!("无法应用动画桌宠：{error}"))
        }
    }
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
    tools::builtin::desktop_pet::normalize_pet_image(bytes).map_err(|error| error.to_string())
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
    // ASTRO_MEMORY_DIR may live outside $HOME. Grant only the configured UI asset domain,
    // never arbitrary source directories, so custom homes work with the asset protocol.
    app.asset_protocol_scope()
        .allow_directory(home::ui_dir(&home::default_memory_dir()), true)
        .map_err(|error| format!("无法授权桌宠素材目录：{error}"))?;
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

fn sync_window<R: Runtime>(app: &AppHandle<R>) -> Result<DesktopPetStateDto, String> {
    let _guard = PET_WINDOW_SYNC
        .lock()
        .map_err(|_| "桌宠窗口同步锁不可用".to_string())?;
    // Read after acquiring the lock: an older IPC must not reapply stale visibility/size.
    let latest = load_state()?;
    ensure_window(app, &latest)?;
    Ok(latest)
}

pub(super) fn present_committed_state(
    app: &AppHandle,
    committed: DesktopPetStateDto,
) -> Result<DesktopPetStateDto, String> {
    let latest = match sync_window(app) {
        Ok(state) => state,
        Err(error) => {
            emit_state(app, &committed);
            return Err(format!("桌宠设置已保存，但窗口同步失败：{error}"));
        }
    };
    emit_state(app, &latest);
    Ok(latest)
}

pub fn restore_window<R: Runtime>(app: &AppHandle<R>) {
    let state = match load_state() {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(%error, "restore desktop pet state failed");
            return;
        }
    };
    if let Err(error) = ensure_window(app, &state) {
        tracing::warn!(%error, "restore desktop pet window failed");
    }
}

fn refresh_from_disk<R: Runtime>(app: &AppHandle<R>) {
    let state = match sync_window(app) {
        Ok(state) => state,
        Err(error) => {
            tracing::warn!(%error, "refresh desktop pet state failed");
            return;
        }
    };
    emit_state(app, &state);
}

fn schedule_refresh(app: &AppHandle) {
    let app = app.clone();
    // Tauri dispatches platform operations internally. Creating a WebView from a
    // synchronous command/main-thread callback deadlocks on Windows WebView2.
    tauri::async_runtime::spawn_blocking(move || refresh_from_disk(&app));
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
        let mut previous = load_state().ok().map(|state| state.revision);
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            let Ok(state) = load_state() else {
                continue;
            };
            let current = Some(state.revision);
            if current == previous {
                continue;
            }
            previous = current;
            schedule_refresh(&watcher_app);
        }
    });
}

#[tauri::command]
pub fn get_desktop_pet_state() -> Result<DesktopPetStateDto, String> {
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
    let source_path = path.to_string_lossy().into_owned();
    let updated = types::update_desktop_pet_state(&base, |state| {
        state.source_path = Some(source_path);
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    });
    let state = match updated {
        Ok(state) => state,
        Err(error) => {
            let _ = fs::remove_file(path);
            return Err(format!("无法保存桌宠设置：{error}"));
        }
    };
    emit_state(&app, &state);
    Ok(state)
}

#[tauri::command]
pub async fn import_desktop_pet_package(
    app: AppHandle,
    manifest_path: String,
) -> Result<DesktopPetStateDto, String> {
    let state =
        import_animated_pet_at(&home::default_memory_dir(), Path::new(manifest_path.trim()))?;
    present_committed_state(&app, state)
}

pub(super) async fn generate_pet_identity(
    base: &Path,
    source_path: &str,
    details: &str,
) -> Result<types::pet_scene::PetIdentity, String> {
    let image = validate_source_path(source_path)?;
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
    let pet_path = store_asset(base, "pet", &transparent, "png")?;
    Ok(types::pet_scene::PetIdentity {
        pet_path: pet_path.to_string_lossy().into_owned(),
        source_path: Some(source_path.to_string()),
        provider: Some(generated.provider),
        model: Some(generated.model),
        sprite_version_number: None,
        display_name: None,
        description: Some(details.to_string()),
    })
}

#[tauri::command]
pub async fn set_desktop_pet_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<DesktopPetStateDto, String> {
    let base = home::default_memory_dir();
    let state = types::update_desktop_pet_state(&base, |state| {
        if enabled {
            anyhow::ensure!(
                state
                    .pet_path
                    .as_deref()
                    .is_some_and(|path| Path::new(path).is_file()),
                "请先生成或导入有效的桌宠图片"
            );
        }
        state.enabled = enabled;
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    })
    .map_err(|error| format!("无法保存桌宠设置：{error}"))?;
    present_committed_state(&app, state)
}

#[tauri::command]
pub async fn set_desktop_pet_scale(
    app: AppHandle,
    scale: f64,
) -> Result<DesktopPetStateDto, String> {
    if !scale.is_finite() {
        return Err("桌宠大小必须为有限数值".into());
    }
    let base = home::default_memory_dir();
    let state = types::update_desktop_pet_state(&base, |state| {
        state.scale = scale.clamp(0.65, 1.35);
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    })
    .map_err(|error| format!("无法保存桌宠设置：{error}"))?;
    present_committed_state(&app, state)
}

#[tauri::command]
pub async fn set_desktop_pet_always_on_top(
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
    present_committed_state(&app, state)
}

#[tauri::command]
pub fn open_desktop_pet_main(app: AppHandle, settings: Option<bool>) {
    crate::ui::tray::show_main_window(&app);
    if settings == Some(true) {
        let _ = app.emit_to("main", "desktop-pet-open-settings", ());
    }
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

    fn valid_v2_atlas(populate_unused_cell: bool) -> Vec<u8> {
        let mut image = RgbaImage::from_pixel(
            types::DESKTOP_PET_V2_WIDTH,
            types::DESKTOP_PET_V2_HEIGHT,
            Rgba([0, 0, 0, 0]),
        );
        for (row, used_columns) in types::DESKTOP_PET_V2_USED_COLUMNS.iter().enumerate() {
            for column in 0..*used_columns {
                image.put_pixel(
                    column * types::DESKTOP_PET_V2_CELL_WIDTH + 8,
                    row as u32 * types::DESKTOP_PET_V2_CELL_HEIGHT + 8,
                    Rgba([120, 65, 35, 255]),
                );
            }
        }
        if populate_unused_cell {
            image.put_pixel(
                7 * types::DESKTOP_PET_V2_CELL_WIDTH + 8,
                8,
                Rgba([120, 65, 35, 255]),
            );
        }
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
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
    fn corrupt_pet_state_is_reported_without_resetting_it() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(pet_dir_at(temp.path())).unwrap();
        let path = types::desktop_pet_state_path(temp.path());
        fs::write(&path, "corrupt-state").unwrap();
        assert!(load_state_at(temp.path()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "corrupt-state");
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
        let loaded = load_state_at(temp.path()).unwrap();
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

    #[test]
    fn imports_a_valid_codex_v2_pet_package() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("package");
        let astro = temp.path().join("astro");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("spritesheet.png"), valid_v2_atlas(false)).unwrap();
        fs::write(
            package.join("pet.json"),
            serde_json::to_vec_pretty(&types::DesktopPetManifest {
                id: "momo".into(),
                display_name: "Momo".into(),
                description: "A calm orange cat".into(),
                sprite_version_number: 2,
                spritesheet_path: "spritesheet.png".into(),
            })
            .unwrap(),
        )
        .unwrap();

        let state = import_animated_pet_at(&astro, &package.join("pet.json")).unwrap();
        assert!(state.enabled);
        assert_eq!(state.sprite_version_number, Some(2));
        assert_eq!(state.display_name.as_deref(), Some("Momo"));
        assert!(Path::new(state.pet_path.as_deref().unwrap()).is_file());
    }

    #[test]
    fn rejects_populated_unused_v2_cells() {
        let error = validate_v2_atlas(&valid_v2_atlas(true)).unwrap_err();
        assert!(error.contains("未使用格"), "{error}");
    }

    #[test]
    fn rejects_a_spritesheet_outside_the_selected_package() {
        let temp = tempfile::tempdir().unwrap();
        let package = temp.path().join("package");
        fs::create_dir_all(&package).unwrap();
        fs::write(temp.path().join("outside.png"), valid_v2_atlas(false)).unwrap();
        fs::write(
            package.join("pet.json"),
            serde_json::to_vec_pretty(&types::DesktopPetManifest {
                id: "escape".into(),
                display_name: "Escape".into(),
                description: "Must remain inside the package".into(),
                sprite_version_number: 2,
                spritesheet_path: "../outside.png".into(),
            })
            .unwrap(),
        )
        .unwrap();

        let error = import_animated_pet_at(&temp.path().join("astro"), &package.join("pet.json"))
            .unwrap_err();
        assert!(error.contains("必须位于宠物包目录内"), "{error}");
    }
}
