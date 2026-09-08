//! Desktop pet control for chat-driven generation workflows.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use image::{ImageFormat, ImageReader};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

const MAX_IMAGE_BYTES: u64 = 25 * 1024 * 1024;
const MAX_IMAGE_DIMENSION: u32 = 8192;
const MAX_DECODE_ALLOC: u64 = 128 * 1024 * 1024;
static PET_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DesktopPetAction {
    Status,
    Apply,
    Show,
    Hide,
    Configure,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPetArgs {
    /// status 查看；apply 应用 image_gen 结果；show/hide 显示或隐藏；configure 调整显示。
    pub action: DesktopPetAction,
    /// apply 必填：image_gen 返回的工作区相对路径或授权根内绝对路径。
    #[serde(default)]
    pub image_path: Option<String>,
    /// apply 可选：生成所依据的原始宠物照片路径，用于设置页保留来源预览。
    #[serde(default)]
    pub source_path: Option<String>,
    /// configure/apply 可选：窗口缩放，范围 0.65..=1.35。
    #[serde(default)]
    pub scale: Option<f64>,
    /// configure/apply 可选：是否保持置顶。
    #[serde(default)]
    pub always_on_top: Option<bool>,
    /// apply 可选：记录实际图片 Provider。
    #[serde(default)]
    pub provider: Option<String>,
    /// apply 可选：记录实际图片模型。
    #[serde(default)]
    pub model: Option<String>,
}

struct ValidatedImage {
    bytes: Vec<u8>,
    extension: &'static str,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "desktop_pet".to_string(),
        toolset: "desktop_pet".to_string(),
        description: "Apply an image_gen result as Astro's desktop pet, inspect its state, show or hide it, and adjust scale or always-on-top behavior. Use action=apply only with an existing authorized local image path; this tool does not generate images itself."
            .to_string(),
        schema: schema_for_args::<DesktopPetArgs>(),
        check_fn: None,
        icon: "dog",
        ..ToolEntry::lifecycle_defaults().deferred().exclusive()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["desktop_pet"],
    sync_named: handle,
}

fn handle(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    anyhow::ensure!(name == "desktop_pet", "未知桌面宠物工具: {name}");
    let parsed: DesktopPetArgs = serde_json::from_value(args.clone())
        .map_err(|error| anyhow::anyhow!("desktop_pet 参数无效: {error}"))?;
    let action = parsed.action;
    let _guard = PET_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("桌面宠物写锁不可用"))?;

    let state = match action {
        DesktopPetAction::Status => read_state(ctx)?,
        DesktopPetAction::Apply => apply(ctx, parsed)?,
        DesktopPetAction::Show => update_state(ctx, |state| {
            let path = state
                .pet_path
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("尚未生成桌面宠物"))?;
            anyhow::ensure!(Path::new(path).is_file(), "桌面宠物图片不存在");
            state.enabled = true;
            Ok(())
        })?,
        DesktopPetAction::Hide => update_state(ctx, |state| {
            state.enabled = false;
            Ok(())
        })?,
        DesktopPetAction::Configure => {
            anyhow::ensure!(
                parsed.scale.is_some() || parsed.always_on_top.is_some(),
                "configure 需要 scale 或 alwaysOnTop"
            );
            update_state(ctx, |state| {
                apply_display_options(state, parsed.scale, parsed.always_on_top);
                Ok(())
            })?
        }
    };

    Ok(serde_json::to_string(&serde_json::json!({
        "astro_desktop_pet": true,
        "action": action,
        "state": state,
    }))?)
}

fn read_state(ctx: &ToolContext<'_>) -> anyhow::Result<types::DesktopPetState> {
    types::read_desktop_pet_state(&ctx.memory_dir)
}

fn apply(ctx: &ToolContext<'_>, args: DesktopPetArgs) -> anyhow::Result<types::DesktopPetState> {
    let image_path = args
        .image_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("apply 需要 imagePath"))?;
    let image = read_authorized_image(ctx, image_path)?;
    let source_image = args
        .source_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|path| read_authorized_image(ctx, path))
        .transpose()?;
    let pet_bytes = normalize_pet_image(&image.bytes)?;
    let pet_path = store_asset(&ctx.memory_dir, "pet", &pet_bytes, "png")?;
    let source_path = source_image
        .map(|image| store_asset(&ctx.memory_dir, "source", &image.bytes, image.extension))
        .transpose();
    let source_path = match source_path {
        Ok(path) => path,
        Err(error) => {
            let _ = fs::remove_file(&pet_path);
            return Err(error);
        }
    };

    let pet_path_value = pet_path.to_string_lossy().into_owned();
    let source_path_value = source_path
        .as_ref()
        .map(|path| path.to_string_lossy().into_owned());
    let provider = clean_optional(args.provider);
    let model = clean_optional(args.model);
    let updated = types::update_desktop_pet_state(&ctx.memory_dir, |state| {
        state.pet_path = Some(pet_path_value);
        state.sprite_version_number = None;
        state.display_name = None;
        state.description = None;
        if let Some(source_path) = source_path_value {
            state.source_path = Some(source_path);
        }
        state.enabled = true;
        if let Some(provider) = provider {
            state.provider = Some(provider);
        }
        if let Some(model) = model {
            state.model = Some(model);
        }
        apply_display_options(state, args.scale, args.always_on_top);
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    });
    let state = match updated {
        Ok(state) => state,
        Err(error) => {
            let _ = fs::remove_file(&pet_path);
            if let Some(source_path) = source_path {
                let _ = fs::remove_file(source_path);
            }
            return Err(error);
        }
    };
    types::notify_desktop_pet_changed();
    Ok(state)
}

fn update_state(
    ctx: &ToolContext<'_>,
    update: impl FnOnce(&mut types::DesktopPetState) -> anyhow::Result<()>,
) -> anyhow::Result<types::DesktopPetState> {
    let state = types::update_desktop_pet_state(&ctx.memory_dir, |state| {
        update(state)?;
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    })?;
    types::notify_desktop_pet_changed();
    Ok(state)
}

fn apply_display_options(
    state: &mut types::DesktopPetState,
    scale: Option<f64>,
    always_on_top: Option<bool>,
) {
    if let Some(scale) = scale.filter(|scale| scale.is_finite()) {
        state.scale = scale.clamp(0.65, 1.35);
    }
    if let Some(always_on_top) = always_on_top {
        state.always_on_top = always_on_top;
    }
}

fn clean_optional(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn resolve_authorized_source(ctx: &ToolContext<'_>, input: &str) -> anyhow::Result<PathBuf> {
    let requested = PathBuf::from(input);
    let candidate = if requested.is_absolute() {
        requested
    } else {
        ctx.workspace_dir.join(requested)
    };
    let canonical = candidate
        .canonicalize()
        .map_err(|_| anyhow::anyhow!("桌面宠物图片不存在: {}", candidate.display()))?;
    anyhow::ensure!(canonical.is_file(), "桌面宠物路径不是文件");

    let mut roots = vec![ctx.workspace_dir.clone(), ctx.memory_dir.clone()];
    if let Some(project_root) = &ctx.project_root {
        roots.push(project_root.clone());
    }
    roots.extend(ctx.workspace_roots.iter().cloned());
    let authorized = roots.into_iter().any(|root| {
        root.canonicalize()
            .map(|root| canonical.starts_with(root))
            .unwrap_or(false)
    });
    anyhow::ensure!(authorized, "桌面宠物图片必须位于工作区或 Astro 数据目录内");
    Ok(canonical)
}

fn read_authorized_image(ctx: &ToolContext<'_>, input: &str) -> anyhow::Result<ValidatedImage> {
    let path = resolve_authorized_source(ctx, input)?;
    let bytes = types::desktop_pet::read_limited_pet_file(&path, MAX_IMAGE_BYTES)?;
    let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
    let format = reader
        .format()
        .ok_or_else(|| anyhow::anyhow!("无法识别桌面宠物图片格式"))?;
    let extension = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpeg => "jpg",
        ImageFormat::WebP => "webp",
        _ => anyhow::bail!("桌面宠物仅支持 PNG、JPEG 和 WebP"),
    };
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| anyhow::anyhow!("桌面宠物图片已损坏或尺寸过大"))?;
    Ok(ValidatedImage { bytes, extension })
}

/// Shared by the settings generator and Agent tool. Always encodes a real PNG.
pub fn normalize_pet_image(bytes: &[u8]) -> anyhow::Result<Vec<u8>> {
    anyhow::ensure!(
        !bytes.is_empty() && bytes.len() as u64 <= MAX_IMAGE_BYTES,
        "桌面宠物图片为空或超过 25 MB"
    );
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    anyhow::ensure!(
        matches!(
            reader.format(),
            Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
        ),
        "桌面宠物仅支持 PNG、JPEG 和 WebP"
    );
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_IMAGE_DIMENSION);
    limits.max_image_height = Some(MAX_IMAGE_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    let decoded = reader.decode()?;
    anyhow::ensure!(
        u64::from(decoded.width()) * u64::from(decoded.height()) <= MAX_DECODE_ALLOC / 4,
        "桌面宠物图片尺寸过大"
    );
    let mut image = decoded.to_rgba8();
    let (width, height) = image.dimensions();
    anyhow::ensure!(width > 0 && height > 0, "桌面宠物图片尺寸无效");
    if image.pixels().all(|pixel| pixel[3] >= 250) {
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
        if corners.iter().all(|pixel| distance(pixel) <= 36.0) {
            // Only remove the edge-connected background, never similarly colored
            // enclosed fur/eyes/body details. Do not recolor pre-existing alpha art.
            let mut visited = vec![false; (width as usize) * (height as usize)];
            let mut pending = std::collections::VecDeque::new();
            for x in 0..width {
                pending.push_back((x, 0));
                pending.push_back((x, height - 1));
            }
            for y in 0..height {
                pending.push_back((0, y));
                pending.push_back((width - 1, y));
            }
            while let Some((x, y)) = pending.pop_front() {
                let index = y as usize * width as usize + x as usize;
                if visited[index] {
                    continue;
                }
                visited[index] = true;
                let pixel = image.get_pixel_mut(x, y);
                let difference = distance(pixel);
                if difference >= 66.0 {
                    continue;
                }
                pixel[3] = (((difference - 22.0) / 44.0).clamp(0.0, 1.0) * 255.0).round() as u8;
                if x > 0 {
                    pending.push_back((x - 1, y));
                }
                if x + 1 < width {
                    pending.push_back((x + 1, y));
                }
                if y > 0 {
                    pending.push_back((x, y - 1));
                }
                if y + 1 < height {
                    pending.push_back((x, y + 1));
                }
            }
        }
    }
    anyhow::ensure!(
        image.pixels().any(|pixel| pixel[3] > 0),
        "未能从图片中提取桌宠主体"
    );
    let mut encoded = Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image).write_to(&mut encoded, ImageFormat::Png)?;
    Ok(encoded.into_inner())
}

fn store_asset(
    base: &Path,
    prefix: &str,
    bytes: &[u8],
    extension: &str,
) -> anyhow::Result<PathBuf> {
    let root = types::desktop_pet_root(base);
    fs::create_dir_all(&root)?;
    let id = uuid::Uuid::new_v4().simple();
    let path = root.join(format!("{prefix}-{id}.{extension}"));
    let temporary = root.join(format!(".{prefix}-{id}.tmp"));
    fs::write(&temporary, bytes)?;
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};
    use std::sync::RwLock;

    #[test]
    fn matte_preserves_enclosed_details_matching_the_background() {
        let mut image = RgbaImage::from_pixel(12, 12, Rgba([255, 255, 255, 255]));
        for y in 3..9 {
            for x in 3..9 {
                image.put_pixel(x, y, Rgba([80, 40, 20, 255]));
            }
        }
        image.put_pixel(6, 6, Rgba([255, 255, 255, 255]));
        let mut bytes = Cursor::new(Vec::new());
        DynamicImage::ImageRgba8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        let result = image::load_from_memory(&normalize_pet_image(bytes.get_ref()).unwrap())
            .unwrap()
            .to_rgba8();
        assert_eq!(result.get_pixel(0, 0)[3], 0);
        assert_eq!(result.get_pixel(6, 6)[3], 255);
    }

    #[test]
    fn normalization_always_returns_png_and_preserves_existing_alpha() {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, Rgba([40, 80, 120, 128])));
        let mut bytes = Cursor::new(Vec::new());
        image.write_to(&mut bytes, ImageFormat::WebP).unwrap();
        let normalized = normalize_pet_image(bytes.get_ref()).unwrap();
        assert_eq!(image::guess_format(&normalized).unwrap(), ImageFormat::Png);
        let normalized = image::load_from_memory(&normalized).unwrap().to_rgba8();
        assert!(normalized.pixels().all(|pixel| pixel[3] == 128));
    }

    fn tiny_png(path: &Path) {
        let mut image = RgbaImage::from_pixel(12, 12, Rgba([220, 250, 235, 255]));
        for y in 4..8 {
            for x in 4..8 {
                image.put_pixel(x, y, Rgba([120, 65, 35, 255]));
            }
        }
        DynamicImage::ImageRgba8(image)
            .save_with_format(path, ImageFormat::Png)
            .unwrap();
    }

    #[tokio::test]
    async fn apply_copies_workspace_image_and_enables_pet() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        tiny_png(&workspace.join("pet.png"));
        let memory = RwLock::new(memory::MemoryManager::new(temp.path().to_path_buf()).unwrap());
        let sessions = session::SessionStore::open_sessions_dir(&temp.path().join("data"))
            .await
            .unwrap();
        let targets = crate::ImageGenTargets::default();
        let credentials = crate::ModelCredentials::default();
        let mut ctx = ToolContext {
            memory: &memory,
            sessions: &sessions,
            memory_dir: temp.path().to_path_buf(),
            workspace_dir: workspace,
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: &targets,
            session_id: "desktop-pet-test".into(),
            turn_id: None,
            credentials: &credentials,
            service_tier: None,
            model_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        };

        let output = handle(
            &mut ctx,
            "desktop_pet",
            &serde_json::json!({
                "action": "apply",
                "imagePath": "pet.png",
                "scale": 1.1,
                "alwaysOnTop": true,
                "provider": "test",
                "model": "test-image"
            }),
        )
        .unwrap();

        assert!(output.contains("\"astro_desktop_pet\":true"));
        let state = types::read_desktop_pet_state(temp.path()).unwrap();
        assert!(state.enabled);
        assert_eq!(state.scale, 1.1);
        assert_eq!(state.provider.as_deref(), Some("test"));
        let pet_path = Path::new(state.pet_path.as_deref().unwrap());
        assert!(pet_path.is_file());
        let applied = image::open(pet_path).unwrap().to_rgba8();
        assert_eq!(applied.get_pixel(0, 0)[3], 0);
        assert_eq!(applied.get_pixel(6, 6)[3], 255);
    }

    #[tokio::test]
    async fn rejects_image_outside_authorized_roots() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&outside).unwrap();
        tiny_png(&outside.join("pet.png"));
        let memory = RwLock::new(memory::MemoryManager::new(temp.path().to_path_buf()).unwrap());
        let sessions = session::SessionStore::open_sessions_dir(&temp.path().join("data"))
            .await
            .unwrap();
        let targets = crate::ImageGenTargets::default();
        let credentials = crate::ModelCredentials::default();
        let ctx = ToolContext {
            memory: &memory,
            sessions: &sessions,
            memory_dir: temp.path().join("astro-home"),
            workspace_dir: workspace,
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: &targets,
            session_id: "desktop-pet-test".into(),
            turn_id: None,
            credentials: &credentials,
            service_tier: None,
            model_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        };

        let error = resolve_authorized_source(&ctx, outside.join("pet.png").to_str().unwrap())
            .unwrap_err()
            .to_string();
        assert!(error.contains("工作区或 Astro 数据目录"), "{error}");
    }
}
