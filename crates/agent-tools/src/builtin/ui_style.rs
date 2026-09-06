//! 生成式界面样式：将已生成壁纸与受控颜色 token 原子应用到 Astro Desktop。

use std::collections::BTreeMap;
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
use types::{
    active_ui_style_path, read_active_ui_style, ui_style_root, UiStyleIconMotion, UiStyleIcons,
    UiStyleManifest, UiStyleTokens, UiStyleWallpaper, UiStyleWallpaperFit, UI_STYLE_SCHEMA_VERSION,
};

const MAX_WALLPAPER_BYTES: u64 = 25 * 1024 * 1024;
const MAX_WALLPAPER_DIMENSION: u32 = 16_384;
const MAX_DECODE_ALLOC: u64 = 128 * 1024 * 1024;

static STYLE_WRITE_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

#[derive(Debug, Clone, Copy, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiStyleAction {
    Status,
    Apply,
    Rollback,
    Reset,
}

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct UiStyleArgs {
    /// status 查看当前样式；apply 应用；rollback 恢复上一版；reset 恢复系统样式。
    pub action: UiStyleAction,
    /// apply 必填，用户可读的主题名。
    #[serde(default)]
    pub name: Option<String>,
    /// 可选稳定 id；只允许小写字母、数字和连字符。
    #[serde(default)]
    pub id: Option<String>,
    /// image_gen 返回的工作区相对路径，或授权根内的绝对路径。
    #[serde(default)]
    pub wallpaper_path: Option<String>,
    #[serde(default)]
    pub fit: Option<UiStyleWallpaperFit>,
    #[serde(default)]
    pub shade: Option<u8>,
    #[serde(default)]
    pub blur: Option<u8>,
    #[serde(default)]
    pub adaptive_color: Option<bool>,
    #[serde(default)]
    pub light_tokens: BTreeMap<String, String>,
    #[serde(default)]
    pub dark_tokens: BTreeMap<String, String>,
    #[serde(default)]
    pub icon_motion: Option<UiStyleIconMotion>,
    #[serde(default)]
    pub icon_stroke_width: Option<f32>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "ui_style".to_string(),
        toolset: "ui_style".to_string(),
        description: "Apply, inspect, roll back, or reset Astro's persistent user UI style. Use image_gen first for a new wallpaper, then action=apply with wallpaperPath. Theme colors accept only the documented safe token allowlist. Styles are versioned under ~/.astro/ui/style and hot-reloaded by Desktop."
            .to_string(),
        schema: schema_for_args::<UiStyleArgs>(),
        check_fn: None,
        icon: "palette",
        ..ToolEntry::lifecycle_defaults().deferred().exclusive()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["ui_style"],
    sync_named: handle,
}

fn handle(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &serde_json::Value,
) -> anyhow::Result<String> {
    anyhow::ensure!(name == "ui_style", "未知界面样式工具: {name}");
    let parsed: UiStyleArgs = serde_json::from_value(args.clone())
        .map_err(|error| anyhow::anyhow!("ui_style 参数无效: {error}"))?;
    let _guard = STYLE_WRITE_LOCK
        .get_or_init(|| Mutex::new(()))
        .lock()
        .map_err(|_| anyhow::anyhow!("界面样式写锁不可用"))?;
    match parsed.action {
        UiStyleAction::Status => status(&ctx.memory_dir),
        UiStyleAction::Apply => apply(ctx, parsed),
        UiStyleAction::Rollback => rollback(&ctx.memory_dir),
        UiStyleAction::Reset => reset(&ctx.memory_dir),
    }
}

fn status(base: &Path) -> anyhow::Result<String> {
    match read_active_ui_style(base).map_err(anyhow::Error::msg)? {
        Some(manifest) => Ok(serde_json::to_string(&serde_json::json!({
            "active": true,
            "style": manifest,
            "rollbackAvailable": latest_history(base).is_some(),
        }))?),
        None => Ok(serde_json::to_string(&serde_json::json!({
            "active": false,
            "style": null,
            "rollbackAvailable": latest_history(base).is_some(),
        }))?),
    }
}

fn apply(ctx: &ToolContext<'_>, args: UiStyleArgs) -> anyhow::Result<String> {
    let name = args.name.as_deref().map(str::trim).unwrap_or_default();
    anyhow::ensure!(!name.is_empty(), "apply 需要 name");
    anyhow::ensure!(name.chars().count() <= 80, "name 不能超过 80 个字符");
    let id = style_id(args.id.as_deref(), name)?;
    let revision = uuid::Uuid::new_v4().simple().to_string();
    let root = ui_style_root(&ctx.memory_dir);
    let theme_dir = root.join("themes").join(&id);

    let (wallpaper, wallpaper_file) = match args
        .wallpaper_path
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        Some(source) => {
            let source = resolve_authorized_source(ctx, source)?;
            let bytes = read_validated_image(&source)?;
            let format = image::guess_format(&bytes)?;
            let extension = match format {
                ImageFormat::Png => "png",
                ImageFormat::Jpeg => "jpg",
                ImageFormat::WebP => "webp",
                ImageFormat::Bmp => "bmp",
                _ => anyhow::bail!("仅支持 PNG、JPEG、WebP 和 BMP 壁纸"),
            };
            let filename = format!("wallpaper-{revision}.{extension}");
            (
                Some(UiStyleWallpaper {
                    path: format!("themes/{id}/{filename}"),
                    fit: args.fit.unwrap_or_default(),
                    shade: args.shade.unwrap_or(18),
                    blur: args.blur.unwrap_or(0),
                    adaptive_color: args.adaptive_color.unwrap_or(true),
                    recommended_theme: None,
                    accent_color: None,
                    secondary_color: None,
                }),
                Some((theme_dir.join(filename), bytes)),
            )
        }
        None => (None, None),
    };

    let manifest = UiStyleManifest {
        schema_version: UI_STYLE_SCHEMA_VERSION,
        id: id.clone(),
        name: name.to_string(),
        revision: revision.clone(),
        updated_at: chrono::Utc::now().to_rfc3339(),
        tokens: UiStyleTokens {
            light: args.light_tokens,
            dark: args.dark_tokens,
        },
        icons: UiStyleIcons {
            motion: args.icon_motion,
            stroke_width: args.icon_stroke_width,
        },
        wallpaper,
    };
    manifest.validate().map_err(anyhow::Error::msg)?;
    let json = serde_json::to_vec_pretty(&manifest)?;
    fs::create_dir_all(&theme_dir)?;
    if let Some((path, bytes)) = wallpaper_file {
        write_atomic(&path, &bytes)?;
    }
    write_atomic(&theme_dir.join("theme.json"), &json)?;
    activate_manifest(&ctx.memory_dir, &json)?;
    types::notify_ui_style_changed();

    Ok(serde_json::to_string(&serde_json::json!({
        "applied": true,
        "id": id,
        "name": name,
        "revision": revision,
        "activePath": active_ui_style_path(&ctx.memory_dir),
        "rollbackAvailable": latest_history(&ctx.memory_dir).is_some(),
    }))?)
}

fn reset(base: &Path) -> anyhow::Result<String> {
    let active = active_ui_style_path(base);
    if !active.is_file() {
        return Ok(serde_json::to_string(&serde_json::json!({
            "reset": true,
            "previouslyActive": false,
            "rollbackAvailable": latest_history(base).is_some(),
        }))?);
    }
    archive_active(base, &active)?;
    fs::remove_file(&active)?;
    types::notify_ui_style_changed();
    Ok(serde_json::to_string(&serde_json::json!({
        "reset": true,
        "previouslyActive": true,
        "rollbackAvailable": true,
    }))?)
}

fn rollback(base: &Path) -> anyhow::Result<String> {
    let previous = latest_history(base).ok_or_else(|| anyhow::anyhow!("没有可回滚的界面样式"))?;
    let bytes = fs::read(&previous)?;
    let manifest: UiStyleManifest = serde_json::from_slice(&bytes)?;
    manifest.validate().map_err(anyhow::Error::msg)?;

    let active = active_ui_style_path(base);
    let current_backup = archive_active(base, &active)?;
    if let Err(error) = write_atomic(&active, &bytes) {
        if let Some(current_backup) = current_backup {
            let _ = fs::copy(current_backup, &active);
        }
        return Err(error);
    }
    fs::remove_file(&previous)?;
    types::notify_ui_style_changed();
    Ok(serde_json::to_string(&serde_json::json!({
        "rolledBack": true,
        "id": manifest.id,
        "name": manifest.name,
        "revision": manifest.revision,
        "rollbackAvailable": latest_history(base).is_some(),
    }))?)
}

fn latest_history(base: &Path) -> Option<PathBuf> {
    let history = ui_style_root(base).join("history");
    fs::read_dir(&history)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "json"))
        .max_by(|left, right| {
            let modified = |entry: &fs::DirEntry| {
                entry
                    .metadata()
                    .and_then(|metadata| metadata.modified())
                    .unwrap_or(std::time::UNIX_EPOCH)
            };
            modified(left)
                .cmp(&modified(right))
                .then_with(|| left.file_name().cmp(&right.file_name()))
        })
        .map(|entry| entry.path())
}

fn style_id(requested: Option<&str>, name: &str) -> anyhow::Result<String> {
    if let Some(requested) = requested.map(str::trim).filter(|value| !value.is_empty()) {
        anyhow::ensure!(
            requested.len() <= 80
                && requested
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'),
            "id 只能包含小写字母、数字和连字符"
        );
        return Ok(requested.to_string());
    }
    let slug = name
        .chars()
        .filter_map(|character| {
            let character = character.to_ascii_lowercase();
            (character.is_ascii_alphanumeric() || character == '-').then_some(character)
        })
        .take(48)
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    let suffix = &uuid::Uuid::new_v4().simple().to_string()[..8];
    Ok(if slug.is_empty() {
        format!("theme-{suffix}")
    } else {
        format!("{slug}-{suffix}")
    })
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
        .map_err(|_| anyhow::anyhow!("壁纸文件不存在: {}", candidate.display()))?;
    anyhow::ensure!(canonical.is_file(), "壁纸路径不是文件");

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
    anyhow::ensure!(authorized, "壁纸必须位于当前工作区或 Astro 数据目录内");
    Ok(canonical)
}

fn read_validated_image(path: &Path) -> anyhow::Result<Vec<u8>> {
    let metadata = fs::metadata(path)?;
    anyhow::ensure!(metadata.len() <= MAX_WALLPAPER_BYTES, "壁纸不能超过 25 MB");
    let bytes = fs::read(path)?;
    let mut reader = ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
    let format = reader
        .format()
        .filter(|format| {
            matches!(
                format,
                ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Bmp
            )
        })
        .ok_or_else(|| anyhow::anyhow!("仅支持 PNG、JPEG、WebP 和 BMP 壁纸"))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_WALLPAPER_DIMENSION);
    limits.max_image_height = Some(MAX_WALLPAPER_DIMENSION);
    limits.max_alloc = Some(MAX_DECODE_ALLOC);
    reader.limits(limits);
    reader
        .decode()
        .map_err(|_| anyhow::anyhow!("壁纸已损坏或尺寸过大"))?;
    anyhow::ensure!(
        matches!(
            format,
            ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP | ImageFormat::Bmp
        ),
        "不支持的壁纸格式"
    );
    Ok(bytes)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("目标路径缺少父目录"))?;
    fs::create_dir_all(parent)?;
    let temporary = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, bytes)?;
    if path.exists() {
        fs::remove_file(path)?;
    }
    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
    Ok(())
}

fn archive_active(base: &Path, active: &Path) -> anyhow::Result<Option<PathBuf>> {
    if !active.is_file() {
        return Ok(None);
    }
    let history = ui_style_root(base).join("history");
    fs::create_dir_all(&history)?;
    let destination = history.join(format!(
        "{}-{}.json",
        chrono::Utc::now().format("%Y%m%dT%H%M%S"),
        uuid::Uuid::new_v4().simple()
    ));
    fs::copy(active, &destination)?;
    Ok(Some(destination))
}

fn activate_manifest(base: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let active = active_ui_style_path(base);
    let root = ui_style_root(base);
    fs::create_dir_all(&root)?;
    let temporary = root.join(format!(".active-{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, bytes)?;
    let backup = archive_active(base, &active)?;
    if active.exists() {
        fs::remove_file(&active)?;
    }
    if let Err(error) = fs::rename(&temporary, &active) {
        let _ = fs::remove_file(&temporary);
        if let Some(backup) = backup {
            let _ = fs::copy(backup, &active);
        }
        return Err(error.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{DynamicImage, Rgba, RgbaImage};
    use std::sync::RwLock;

    fn tiny_png(path: &Path) {
        let image = DynamicImage::ImageRgba8(RgbaImage::from_pixel(4, 3, Rgba([1, 2, 3, 255])));
        image.save_with_format(path, ImageFormat::Png).unwrap();
    }

    #[test]
    fn writes_versioned_bundle_and_archives_previous_active_style() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        let image = workspace.join("generated.png");
        tiny_png(&image);

        let first = UiStyleManifest {
            schema_version: UI_STYLE_SCHEMA_VERSION,
            id: "first".into(),
            name: "First".into(),
            revision: "r1".into(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            tokens: UiStyleTokens {
                light: BTreeMap::from([("--color-accent".into(), "#112233".into())]),
                dark: BTreeMap::new(),
            },
            icons: UiStyleIcons::default(),
            wallpaper: None,
        };
        activate_manifest(temp.path(), &serde_json::to_vec(&first).unwrap()).unwrap();

        let second = UiStyleManifest {
            id: "second".into(),
            name: "Second".into(),
            revision: "r2".into(),
            ..first
        };
        activate_manifest(temp.path(), &serde_json::to_vec(&second).unwrap()).unwrap();

        assert_eq!(
            read_active_ui_style(temp.path()).unwrap().unwrap().id,
            "second"
        );
        assert_eq!(
            fs::read_dir(ui_style_root(temp.path()).join("history"))
                .unwrap()
                .count(),
            1
        );
    }

    #[test]
    fn validates_image_bytes_before_copying() {
        let temp = tempfile::tempdir().unwrap();
        let bad = temp.path().join("bad.png");
        fs::write(&bad, b"not an image").unwrap();
        assert!(read_validated_image(&bad).is_err());
    }

    #[test]
    fn rollback_restores_the_previous_manifest() {
        let temp = tempfile::tempdir().unwrap();
        let first = UiStyleManifest {
            schema_version: UI_STYLE_SCHEMA_VERSION,
            id: "first".into(),
            name: "First".into(),
            revision: "r1".into(),
            updated_at: chrono::Utc::now().to_rfc3339(),
            tokens: UiStyleTokens {
                light: BTreeMap::from([("--color-accent".into(), "#112233".into())]),
                dark: BTreeMap::new(),
            },
            icons: UiStyleIcons::default(),
            wallpaper: None,
        };
        let second = UiStyleManifest {
            id: "second".into(),
            name: "Second".into(),
            revision: "r2".into(),
            ..first.clone()
        };
        activate_manifest(temp.path(), &serde_json::to_vec(&first).unwrap()).unwrap();
        activate_manifest(temp.path(), &serde_json::to_vec(&second).unwrap()).unwrap();

        rollback(temp.path()).unwrap();

        assert_eq!(
            read_active_ui_style(temp.path()).unwrap().unwrap().id,
            "first"
        );
    }

    #[tokio::test]
    async fn dispatch_apply_writes_the_active_manifest_from_workspace_image() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        fs::create_dir_all(&workspace).unwrap();
        tiny_png(&workspace.join("generated.png"));
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
            session_id: "ui-style-test".into(),
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
            "ui_style",
            &serde_json::json!({
                "action": "apply",
                "name": "Ocean",
                "wallpaperPath": "generated.png",
                "darkTokens": { "--color-accent": "#38BDF8" },
                "iconMotion": "smooth",
                "iconStrokeWidth": 2
            }),
        )
        .unwrap();

        assert!(output.contains("\"applied\":true"));
        let manifest = read_active_ui_style(temp.path()).unwrap().unwrap();
        assert_eq!(manifest.name, "Ocean");
        assert!(manifest.wallpaper.is_some());
        assert_eq!(
            manifest.tokens.dark.get("--color-accent"),
            Some(&"#38BDF8".to_string())
        );
    }

    #[test]
    fn rejects_untrusted_source_path() {
        let temp = tempfile::tempdir().unwrap();
        let workspace = temp.path().join("workspace");
        let outside = temp.path().join("outside");
        fs::create_dir_all(&workspace).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let image = outside.join("wallpaper.png");
        tiny_png(&image);
        let canonical = image.canonicalize().unwrap();
        let allowed = [workspace]
            .into_iter()
            .filter_map(|root| root.canonicalize().ok())
            .any(|root| canonical.starts_with(root));
        assert!(!allowed);
    }
}
