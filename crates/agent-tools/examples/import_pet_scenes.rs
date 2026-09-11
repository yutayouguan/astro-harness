//! Import a validated local scene batch without switching the desktop.
//! Usage: cargo run -p tools --example import_pet_scenes -- HOME MANIFEST [--apply]
//! Dry-run by default. Uses the same locked state transaction as Desktop/tools.
use anyhow::{bail, ensure, Context, Result};
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs,
    io::Cursor,
    path::{Component, Path, PathBuf},
};
use types::{pet_scene::PetScene, DesktopPetState, UiStyleManifest, UiStyleWallpaper};

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Manifest {
    expected_scene_ids: Vec<String>,
    scenes: Vec<SceneInput>,
}

#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SceneInput {
    id: String,
    pet_id: String,
    name: String,
    wallpaper: String,
    recommended_theme: String,
}

struct PreparedImage {
    bytes: Vec<u8>,
    extension: &'static str,
}

fn prepare(manifest: &Manifest, directory: &Path) -> Result<Vec<PreparedImage>> {
    let directory = directory.canonicalize()?;
    ensure!(
        !manifest.scenes.is_empty() && manifest.scenes.len() <= 32,
        "批次应包含1–32个场景"
    );
    let mut ids = BTreeSet::new();
    let mut names = BTreeSet::new();
    manifest
        .scenes
        .iter()
        .map(|scene| {
            ensure!(ids.insert(&scene.id), "批次场景ID重复");
            ensure!(
                names.insert((&scene.pet_id, &scene.name)),
                "同一宠物的场景名称重复"
            );
            let relative = Path::new(&scene.wallpaper);
            ensure!(
                !relative.as_os_str().is_empty()
                    && relative
                        .components()
                        .all(|c| matches!(c, Component::Normal(_))),
                "壁纸必须使用包内安全相对路径"
            );
            let path = directory.join(relative).canonicalize()?;
            ensure!(path.starts_with(&directory), "壁纸不能越出素材包");
            let bytes = types::desktop_pet::read_limited_pet_file(&path, 25 * 1024 * 1024)?;
            let mut reader = image::ImageReader::new(Cursor::new(&bytes)).with_guessed_format()?;
            let extension = match reader.format() {
                Some(image::ImageFormat::Jpeg) => "jpg",
                Some(image::ImageFormat::Png) => "png",
                Some(image::ImageFormat::WebP) => "webp",
                _ => bail!("不支持的壁纸格式"),
            };
            let mut limits = image::Limits::default();
            limits.max_image_width = Some(16_384);
            limits.max_image_height = Some(16_384);
            limits.max_alloc = Some(128 * 1024 * 1024);
            reader.limits(limits);
            let decoded = reader.decode()?;
            ensure!(decoded.width() > 0 && decoded.height() > 0, "空壁纸");
            Ok(PreparedImage { bytes, extension })
        })
        .collect()
}

fn make_scenes(
    state: &DesktopPetState,
    manifest: &Manifest,
    images: &[PreparedImage],
    folder: &str,
) -> Result<Vec<PetScene>> {
    manifest
        .scenes
        .iter()
        .zip(images)
        .map(|(item, image)| {
            let pet = state
                .pets
                .iter()
                .find(|p| p.id == item.pet_id)
                .context("场景所属宠物不存在")?;
            let scene = PetScene {
                id: item.id.clone(),
                name: item.name.clone(),
                pet: pet.identity.clone(),
                style: Some(UiStyleManifest {
                    schema_version: types::UI_STYLE_SCHEMA_VERSION,
                    id: item.id.clone(),
                    name: item.name.clone(),
                    revision: uuid::Uuid::new_v4().to_string(),
                    updated_at: chrono::Utc::now().to_rfc3339(),
                    tokens: Default::default(),
                    icons: Default::default(),
                    wallpaper: Some(UiStyleWallpaper {
                        path: format!("{folder}/{}.{}", item.id, image.extension),
                        fit: Default::default(),
                        shade: 18,
                        blur: 0,
                        adaptive_color: true,
                        recommended_theme: Some(item.recommended_theme.clone()),
                        accent_color: None,
                        secondary_color: None,
                    }),
                }),
                wallpaper_source_path: None,
                preferences: None,
            };
            scene.validate()?;
            Ok(scene)
        })
        .collect()
}

/// Return true only when the entire exact batch already exists; never overwrite
/// a rebound scene or restore a deliberately deleted part of a previous batch.
fn check_scope(
    base: &Path,
    state: &DesktopPetState,
    manifest: &Manifest,
    images: &[PreparedImage],
) -> Result<bool> {
    let present: Vec<_> = manifest
        .scenes
        .iter()
        .map(|item| state.scenes.iter().find(|s| s.id == item.id))
        .collect();
    if present.iter().all(|s| s.is_some()) {
        for ((scene, item), image) in present.iter().zip(&manifest.scenes).zip(images) {
            let scene = scene.unwrap();
            ensure!(
                scene.pet.pet_id == item.pet_id,
                "既有场景关联到不同宠物，未覆盖"
            );
            let path = scene
                .wallpaper_path(base)
                .context("既有场景缺少壁纸，未覆盖")?;
            let path = types::pet_scene::managed_file(base, &path)?;
            ensure!(
                types::desktop_pet::read_limited_pet_file(&path, 25 * 1024 * 1024)? == image.bytes,
                "既有场景壁纸已变化，未覆盖"
            );
        }
        return Ok(true);
    }
    ensure!(
        present.iter().all(|s| s.is_none()),
        "场景ID冲突或已有部分批次，未覆盖"
    );
    let pets: BTreeSet<_> = manifest.scenes.iter().map(|s| s.pet_id.as_str()).collect();
    let current: BTreeSet<_> = state
        .scenes
        .iter()
        .filter(|s| pets.contains(s.pet.pet_id.as_str()))
        .map(|s| s.id.as_str())
        .collect();
    let expected: BTreeSet<_> = manifest
        .expected_scene_ids
        .iter()
        .map(String::as_str)
        .collect();
    ensure!(
        current == expected,
        "生成期间宠物场景库已变化，请重新核对范围"
    );
    ensure!(
        state.scenes.len() + manifest.scenes.len() <= 100,
        "场景数量超过上限"
    );
    Ok(false)
}

fn import(
    base: &Path,
    manifest: &Manifest,
    directory: &Path,
    apply: bool,
) -> Result<(bool, DesktopPetState)> {
    ensure!(
        types::desktop_pet_state_path(base).is_file(),
        "目标不是已有宠物库"
    );
    let images = prepare(manifest, directory)?;
    let current = types::read_desktop_pet_state(base)?;
    make_scenes(&current, manifest, &images, "scene-pack-preview")?;
    if check_scope(base, &current, manifest, &images)? || !apply {
        return Ok((false, current));
    }
    let root = types::ui_style_root(base);
    fs::create_dir_all(&root)?;
    // All image files exist before publishing a single scene. RAII discards only
    // this owned temporary folder on errors; the caller's generated originals stay.
    let staging = tempfile::Builder::new()
        .prefix("scene-pack-")
        .tempdir_in(&root)?;
    let folder = staging
        .path()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    for (scene, image) in manifest.scenes.iter().zip(&images) {
        types::pet_scene::atomic_write(
            &staging
                .path()
                .join(format!("{}.{}", scene.id, image.extension)),
            &image.bytes,
        )?;
    }
    let state = types::update_desktop_pet_state(base, |state| {
        ensure!(
            !check_scope(base, state, manifest, &images)?,
            "批次已被另一个写入者导入"
        );
        let scenes = make_scenes(state, manifest, &images, &folder)?;
        for scene in &scenes {
            types::pet_scene::managed_file(base, Path::new(&scene.pet.pet_path))?;
            types::pet_scene::managed_file(base, &scene.wallpaper_path(base).unwrap())?;
        }
        state.scenes.extend(scenes);
        Ok(())
    })?;
    let _ = staging.keep();
    types::notify_desktop_pet_changed();
    Ok((true, state))
}

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    ensure!(
        args.len() == 2 || (args.len() == 3 && args[2] == "--apply"),
        "usage: import_pet_scenes HOME MANIFEST [--apply]"
    );
    let base = PathBuf::from(&args[0]).canonicalize()?;
    let file = PathBuf::from(&args[1]).canonicalize()?;
    let bytes = types::desktop_pet::read_limited_pet_file(&file, 64 * 1024)?;
    let manifest: Manifest = serde_json::from_slice(&bytes)?;
    let (applied, state) = import(&base, &manifest, file.parent().unwrap(), args.len() == 3)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&serde_json::json!({
            "applied":applied,"dryRun":args.len()==2,"revision":state.revision,
            "activePetId":state.active_pet_id,"activeSceneId":state.active_scene_id,
            "sceneIds":manifest.scenes.iter().map(|s|&s.id).collect::<Vec<_>>()
        }))?
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, tempfile::TempDir, Manifest) {
        let base = tempfile::tempdir().unwrap();
        let input = tempfile::tempdir().unwrap();
        let image = image::DynamicImage::new_rgb8(16, 12);
        image.save(input.path().join("wall.png")).unwrap();
        let path = types::desktop_pet_root(base.path()).join("pet.png");
        types::pet_scene::atomic_write(&path, b"validated existing pet").unwrap();
        let state = types::update_desktop_pet_state(base.path(), |state| {
            state.pet_path = Some(path.to_string_lossy().into_owned());
            state.enabled = true;
            state.scale = 0.35;
            state.preferences.quiet_mode = true;
            Ok(())
        })
        .unwrap();
        let id = state.active_pet_id.unwrap();
        let manifest = Manifest {
            expected_scene_ids: vec![],
            scenes: (0..3)
                .map(|i| SceneInput {
                    id: format!("pet-fixture-{i}"),
                    pet_id: id.clone(),
                    name: format!("Scene {i}"),
                    wallpaper: "wall.png".into(),
                    recommended_theme: "light".into(),
                })
                .collect(),
        };
        (base, input, manifest)
    }
    #[test]
    fn batch_is_preview_only_atomic_idempotent_and_dry_by_default() {
        let (base, input, manifest) = fixture();
        let before = types::read_desktop_pet_state(base.path()).unwrap();
        let (applied, dry) = import(base.path(), &manifest, input.path(), false).unwrap();
        assert!(!applied);
        assert_eq!(dry, before);
        let (_, after) = import(base.path(), &manifest, input.path(), true).unwrap();
        assert_eq!(after.scenes.len(), 3);
        assert!(after.scenes.iter().all(|s| s.preferences.is_none()));
        let mut unchanged = after.clone();
        unchanged.scenes.clear();
        unchanged.revision = before.revision;
        assert_eq!(unchanged, before);
        assert!(!types::active_ui_style_path(base.path()).exists());
        let (applied, repeated) = import(base.path(), &manifest, input.path(), true).unwrap();
        assert!(!applied);
        assert_eq!(after, repeated);
    }
    #[test]
    fn rejects_bad_images_and_concurrent_scene_changes_without_partial_import() {
        let (base, input, mut manifest) = fixture();
        let before = types::read_desktop_pet_state(base.path()).unwrap();
        manifest.scenes[2].wallpaper = "missing.png".into();
        assert!(import(base.path(), &manifest, input.path(), true).is_err());
        assert_eq!(types::read_desktop_pet_state(base.path()).unwrap(), before);
        manifest.scenes[2].wallpaper = "wall.png".into();
        let changed = types::pet_library::edit_library(
            base.path(),
            types::pet_library::PetLibraryEdit::AddScene {
                pet_id: manifest.scenes[0].pet_id.clone(),
                name: "User added".into(),
            },
        )
        .unwrap();
        assert!(import(base.path(), &manifest, input.path(), true).is_err());
        assert_eq!(types::read_desktop_pet_state(base.path()).unwrap(), changed);
    }

    #[test]
    fn imports_three_scenes_for_each_pet_without_changing_the_active_pet() {
        let (base, input, mut manifest) = fixture();
        let before = types::update_desktop_pet_state(base.path(), |state| {
            let mut other = state.pets[0].clone();
            other.id = "second-pet".into();
            other.identity.pet_id = other.id.clone();
            other.identity.display_name = Some("Second".into());
            state.pets.push(other);
            Ok(())
        })
        .unwrap();
        let more = manifest
            .scenes
            .iter()
            .enumerate()
            .map(|(i, item)| SceneInput {
                id: format!("pet-second-{i}"),
                pet_id: "second-pet".into(),
                ..item.clone()
            })
            .collect::<Vec<_>>();
        manifest.scenes.extend(more);
        let (_, after) = import(base.path(), &manifest, input.path(), true).unwrap();
        assert_eq!(after.scenes.len(), 6);
        assert_eq!(
            after
                .scenes
                .iter()
                .filter(|s| s.pet.pet_id == "second-pet")
                .count(),
            3
        );
        assert_eq!(after.active_pet_id, before.active_pet_id);
        assert_eq!(after.preferences, before.preferences);
        assert_eq!(after.scale, before.scale);
    }

    #[test]
    fn rejects_path_escape_missing_pet_and_partial_reimport() {
        let (base, input, manifest) = fixture();
        let before = types::read_desktop_pet_state(base.path()).unwrap();
        let mut bad = manifest.clone();
        bad.scenes[0].wallpaper = "../wall.png".into();
        assert!(import(base.path(), &bad, input.path(), true).is_err());
        bad = manifest.clone();
        bad.scenes[0].pet_id = "missing-pet".into();
        assert!(import(base.path(), &bad, input.path(), true).is_err());
        assert_eq!(types::read_desktop_pet_state(base.path()).unwrap(), before);
        let (_, saved) = import(base.path(), &manifest, input.path(), true).unwrap();
        let changed = types::pet_scene::edit_scene(
            base.path(),
            types::pet_scene::PetSceneEdit::Delete {
                scene_id: saved.scenes[0].id.clone(),
                confirm_active: true,
            },
        )
        .unwrap();
        assert!(import(base.path(), &manifest, input.path(), true).is_err());
        assert_eq!(types::read_desktop_pet_state(base.path()).unwrap(), changed);
    }
}
