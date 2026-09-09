//! Saved pet identities paired with the existing UI-style manifest, never a second theme format.
use crate::{desktop_pet::read_limited_pet_file, DesktopPetState, UiStyleManifest};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PetIdentity {
    pub pet_path: String,
    #[serde(default)]
    pub grooming_path: Option<String>,
    pub source_path: Option<String>,
    pub sprite_version_number: Option<u32>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub provider: Option<String>,
    pub model: Option<String>,
}

impl PetIdentity {
    pub fn from_state(state: &DesktopPetState) -> anyhow::Result<Self> {
        Ok(Self {
            pet_path: state
                .pet_path
                .clone()
                .ok_or_else(|| anyhow::anyhow!("请先生成或导入桌宠"))?,
            source_path: state.source_path.clone(),
            grooming_path: state.grooming_path.clone(),
            sprite_version_number: state.sprite_version_number,
            display_name: state.display_name.clone(),
            description: state.description.clone(),
            provider: state.provider.clone(),
            model: state.model.clone(),
        })
    }

    fn apply(&self, state: &mut DesktopPetState) {
        state.pet_path = Some(self.pet_path.clone());
        state.grooming_path = self.grooming_path.clone();
        state.source_path = self.source_path.clone();
        state.sprite_version_number = self.sprite_version_number;
        state.display_name = self.display_name.clone();
        state.description = self.description.clone();
        state.provider = self.provider.clone();
        state.model = self.model.clone();
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PetScene {
    pub id: String,
    pub name: String,
    pub pet: PetIdentity,
    pub style: Option<UiStyleManifest>,
    /// Canonical managed wallpaper used before copying into the theme package.
    pub wallpaper_source_path: Option<String>,
}

impl PetScene {
    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.id.is_empty()
                && self.id.len() <= 80
                && self
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-'),
            "场景 id 无效"
        );
        anyhow::ensure!(
            !self.name.trim().is_empty()
                && self.name.chars().count() <= 80
                && !self.name.chars().any(char::is_control),
            "场景名称应为 1–80 个字符"
        );
        anyhow::ensure!(!self.pet.pet_path.is_empty(), "场景缺少桌宠");
        anyhow::ensure!(
            self.pet.grooming_path.is_none() || self.pet.sprite_version_number == Some(2),
            "舔爪扩展需要 v2 动画桌宠"
        );
        anyhow::ensure!(
            self.pet.sprite_version_number.is_none() || self.pet.sprite_version_number == Some(2),
            "不支持的桌宠版本"
        );
        if let Some(style) = &self.style {
            style.validate().map_err(anyhow::Error::msg)?;
            anyhow::ensure!(
                style.id == self.id && style.wallpaper.is_some(),
                "场景主题缺少匹配壁纸"
            );
        }
        Ok(())
    }

    pub fn wallpaper_path(&self, base: &Path) -> Option<PathBuf> {
        self.style
            .as_ref()?
            .wallpaper
            .as_ref()
            .map(|w| crate::ui_style_root(base).join(&w.path))
    }
}

pub fn managed_file(base: &Path, path: &Path) -> anyhow::Result<PathBuf> {
    let root = home::ui_dir(base).canonicalize()?;
    let path = path.canonicalize()?;
    anyhow::ensure!(
        path.starts_with(root) && path.is_file(),
        "场景素材不在受控 UI 目录内"
    );
    Ok(path)
}

pub fn save_scene(base: &Path, scene: PetScene) -> anyhow::Result<DesktopPetState> {
    scene.validate()?;
    managed_file(base, Path::new(&scene.pet.pet_path))?;
    if let Some(path) = &scene.pet.grooming_path {
        managed_file(base, Path::new(path))?;
    }
    if let Some(path) = scene.wallpaper_path(base) {
        managed_file(base, &path)?;
    }
    crate::update_desktop_pet_state(base, |state| {
        if let Some(existing) = state.scenes.iter_mut().find(|s| s.id == scene.id) {
            *existing = scene;
        } else {
            state.scenes.push(scene);
        }
        Ok(())
    })
}

pub fn atomic_write(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let parent = path.parent().ok_or_else(|| anyhow::anyhow!("缺少父目录"))?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|error| error.error)?;
    Ok(())
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum PetSceneEdit {
    Rename {
        scene_id: String,
        name: String,
    },
    RenamePet {
        scene_id: String,
        name: String,
    },
    Favorite {
        scene_id: String,
        favorite: bool,
    },
    Delete {
        scene_id: String,
        confirm_active: bool,
    },
    Duplicate {
        scene_id: String,
        name: String,
    },
    Pause {
        paused: bool,
    },
}

fn validate_name(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.trim().is_empty()
            && name.chars().count() <= 80
            && !name.chars().any(char::is_control),
        "名称应为 1–80 个字符"
    );
    Ok(())
}

pub fn scene_is_in_use(base: &Path, state: &DesktopPetState, scene: &PetScene) -> bool {
    state.pet_path.as_ref() == Some(&scene.pet.pet_path)
        || state.last_wallpaper_path.as_ref().is_some_and(|current| {
            scene
                .wallpaper_path(base)
                .is_some_and(|p| wallpaper_key(&p) == *current)
                || scene
                    .wallpaper_source_path
                    .as_ref()
                    .is_some_and(|p| wallpaper_key(Path::new(p)) == *current)
        })
}

/// Library deletion is non-destructive: active visuals and files remain usable.
pub fn edit_scene(base: &Path, edit: PetSceneEdit) -> anyhow::Result<DesktopPetState> {
    let state = crate::update_desktop_pet_state(base, |state| {
        let id = match &edit {
            PetSceneEdit::Rename { scene_id, .. }
            | PetSceneEdit::RenamePet { scene_id, .. }
            | PetSceneEdit::Favorite { scene_id, .. }
            | PetSceneEdit::Delete { scene_id, .. }
            | PetSceneEdit::Duplicate { scene_id, .. } => Some(scene_id),
            PetSceneEdit::Pause { .. } => None,
        };
        let index = id
            .map(|id| {
                state
                    .scenes
                    .iter()
                    .position(|s| &s.id == id)
                    .ok_or_else(|| anyhow::anyhow!("场景不存在"))
            })
            .transpose()?;
        match edit {
            PetSceneEdit::Pause { paused } => state.animation_paused = paused,
            PetSceneEdit::Rename { name, .. } => {
                validate_name(&name)?;
                let scene = &mut state.scenes[index.unwrap()];
                scene.name = name.trim().into();
                if let Some(style) = &mut scene.style {
                    style.name = scene.name.clone();
                }
            }
            PetSceneEdit::RenamePet { name, .. } => {
                validate_name(&name)?;
                let path = state.scenes[index.unwrap()].pet.pet_path.clone();
                for scene in &mut state.scenes {
                    if scene.pet.pet_path == path {
                        scene.pet.display_name = Some(name.trim().into());
                    }
                }
                if state.pet_path.as_ref() == Some(&path) {
                    state.display_name = Some(name.trim().into());
                }
            }
            PetSceneEdit::Favorite { scene_id, favorite } => {
                state.favorite_scene_ids.retain(|id| id != &scene_id);
                if favorite {
                    state.favorite_scene_ids.push(scene_id);
                }
            }
            PetSceneEdit::Delete {
                scene_id,
                confirm_active,
            } => {
                let index = index.unwrap();
                anyhow::ensure!(
                    confirm_active || !scene_is_in_use(base, state, &state.scenes[index]),
                    "此场景的宠物或壁纸正在使用，请确认移出收藏；当前显示和素材文件会保留"
                );
                state.scenes.remove(index);
                state.favorite_scene_ids.retain(|id| id != &scene_id);
            }
            PetSceneEdit::Duplicate { name, .. } => {
                validate_name(&name)?;
                let pet = state.scenes[index.unwrap()].pet.clone();
                state.scenes.push(PetScene {
                    id: format!("pet-{}", uuid::Uuid::new_v4().simple()),
                    name: name.trim().into(),
                    pet,
                    style: None,
                    wallpaper_source_path: None,
                });
            }
        }
        Ok(())
    })?;
    crate::notify_desktop_pet_changed();
    Ok(state)
}

/// Export derived assets only, never the user's source photo or credentials.
pub fn export_scene(base: &Path, id: &str, destination: &Path) -> anyhow::Result<PathBuf> {
    anyhow::ensure!(destination.is_dir(), "请选择导出文件夹");
    let state = crate::read_desktop_pet_state(base)?;
    let mut scene = state
        .scenes
        .into_iter()
        .find(|s| s.id == id)
        .ok_or_else(|| anyhow::anyhow!("场景不存在"))?;
    let pet_path = managed_file(base, Path::new(&scene.pet.pet_path))?;
    let pet = read_limited_pet_file(&pet_path, 50 * 1024 * 1024)?;
    let wallpaper = scene
        .wallpaper_path(base)
        .map(|p| -> anyhow::Result<_> {
            let p = managed_file(base, &p)?;
            let bytes = read_limited_pet_file(&p, 25 * 1024 * 1024)?;
            Ok((p, bytes))
        })
        .transpose()?;
    let package = destination.join(format!("{}-{}", scene.id, uuid::Uuid::new_v4().simple()));
    let temporary = tempfile::Builder::new()
        .prefix(".pet-export-")
        .tempdir_in(destination)?;
    let pet_name = format!(
        "pet.{}",
        pet_path
            .extension()
            .and_then(|x| x.to_str())
            .unwrap_or("png")
    );
    fs::write(temporary.path().join(&pet_name), pet)?;
    scene.pet.pet_path = pet_name.clone();
    scene.pet.source_path = None;
    scene.wallpaper_source_path = None;
    if let Some(path) = &scene.pet.grooming_path {
        let path = managed_file(base, Path::new(path))?;
        let bytes = read_limited_pet_file(&path, 8 * 1024 * 1024)?;
        let name = format!(
            "grooming.{}",
            path.extension().and_then(|s| s.to_str()).unwrap_or("png")
        );
        fs::write(temporary.path().join(&name), bytes)?;
        scene.pet.grooming_path = Some(name);
    }
    if scene.pet.sprite_version_number == Some(2) {
        let manifest = crate::DesktopPetManifest {
            id: scene.id.clone(),
            display_name: scene
                .pet
                .display_name
                .clone()
                .unwrap_or_else(|| scene.name.clone()),
            description: scene
                .pet
                .description
                .clone()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "Exported Astro companion".into()),
            sprite_version_number: 2,
            spritesheet_path: pet_name,
            grooming_spritesheet_path: scene.pet.grooming_path.clone(),
        };
        fs::write(
            temporary.path().join("pet.json"),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
    }
    if let Some((path, bytes)) = wallpaper {
        let name = format!(
            "wallpaper.{}",
            path.extension().and_then(|x| x.to_str()).unwrap_or("png")
        );
        fs::write(temporary.path().join(&name), bytes)?;
        scene
            .style
            .as_mut()
            .unwrap()
            .wallpaper
            .as_mut()
            .unwrap()
            .path = name;
    }
    fs::write(
        temporary.path().join("scene.json"),
        serde_json::to_vec_pretty(&scene)?,
    )?;
    fs::rename(temporary.path(), &package)?;
    Ok(package)
}

pub(crate) fn publish_style(base: &Path, style: &UiStyleManifest) -> anyhow::Result<()> {
    style.validate().map_err(anyhow::Error::msg)?;
    if let Some(w) = &style.wallpaper {
        managed_file(base, &crate::ui_style_root(base).join(&w.path))?;
    }
    let active = crate::active_ui_style_path(base);
    if active.is_file() {
        let old = read_limited_pet_file(&active, crate::ui_style::UI_STYLE_MAX_MANIFEST_BYTES)?;
        if serde_json::from_slice::<UiStyleManifest>(&old)
            .ok()
            .is_some_and(|old| old.revision == style.revision)
        {
            return Ok(());
        }
        atomic_write(
            &crate::ui_style_root(base)
                .join("history")
                .join(format!("scene-{}.json", uuid::Uuid::new_v4())),
            &old,
        )?;
    }
    atomic_write(&active, &serde_json::to_vec_pretty(style)?)
}

#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SceneApplyMode {
    All,
    Pet,
    Wallpaper,
    Linked,
}

pub fn apply_scene(base: &Path, id: &str, mode: SceneApplyMode) -> anyhow::Result<DesktopPetState> {
    let state = crate::update_desktop_pet_state(base, |state| {
        let scene = state
            .scenes
            .iter()
            .find(|s| s.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("场景不存在"))?;
        let apply_pet = mode == SceneApplyMode::All
            || mode == SceneApplyMode::Pet
            || (mode == SceneApplyMode::Linked && state.follow_wallpaper);
        if apply_pet {
            managed_file(base, Path::new(&scene.pet.pet_path))?;
            if let Some(path) = &scene.pet.grooming_path {
                managed_file(base, Path::new(path))?;
            }
        }
        if mode != SceneApplyMode::Pet {
            let mut style = scene
                .style
                .clone()
                .ok_or_else(|| anyhow::anyhow!("此场景尚无壁纸"))?;
            let path = managed_file(base, &scene.wallpaper_path(base).unwrap())?;
            style.revision = uuid::Uuid::new_v4().to_string();
            state.pending_scene_style = Some(style);
            state.last_wallpaper_path = Some(path.to_string_lossy().into_owned());
        }
        if apply_pet {
            scene.pet.apply(state);
        }
        if mode == SceneApplyMode::All {
            state.follow_wallpaper = true;
            state.enabled = true;
        }
        if mode == SceneApplyMode::Pet {
            state.follow_wallpaper = false;
            state.enabled = true;
        }
        Ok(())
    })?;
    // The durable outbox is replayed on startup/read if publication is interrupted.
    let recovered = crate::read_desktop_pet_state(base)
        .map_err(|error| anyhow::anyhow!("场景选择已保存，壁纸应用待恢复：{error}"))?;
    if state.pending_scene_style.is_some() {
        crate::notify_ui_style_changed();
    }
    crate::notify_desktop_pet_changed();
    Ok(recovered)
}

/// Unbound wallpapers preserve the current pet; never regenerate during switching.
fn wallpaper_key(path: &Path) -> String {
    path.canonicalize()
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

pub fn sync_wallpaper(base: &Path, path: Option<&str>) -> anyhow::Result<DesktopPetState> {
    // Resolve the active theme again under the pet lock below. A delayed UI analysis
    // response must not switch the pet back to a wallpaper that is no longer active.
    let effective = || -> anyhow::Result<Option<String>> {
        let active = crate::read_active_ui_style(base).map_err(anyhow::Error::msg)?;
        Ok(active
            .and_then(|s| {
                s.wallpaper
                    .map(|w| wallpaper_key(&crate::ui_style_root(base).join(w.path)))
            })
            .or_else(|| path.map(|p| wallpaper_key(Path::new(p)))))
    };
    let current = crate::read_desktop_pet_state(base)?;
    let path = effective()?;
    if current.last_wallpaper_path == path {
        return Ok(current);
    }
    let state = crate::update_desktop_pet_state(base, |state| {
        let path = effective()?;
        if state.last_wallpaper_path == path {
            return Ok(());
        }
        if state.follow_wallpaper {
            if let Some(scene) = state
                .scenes
                .iter()
                .rev()
                .find(|scene| {
                    path.as_ref().is_some_and(|path| {
                        scene
                            .wallpaper_source_path
                            .as_ref()
                            .is_some_and(|source| wallpaper_key(Path::new(source)) == *path)
                            || scene
                                .wallpaper_path(base)
                                .as_ref()
                                .is_some_and(|p| wallpaper_key(p) == *path)
                    })
                })
                .cloned()
            {
                managed_file(base, Path::new(&scene.pet.pet_path))?;
                if let Some(path) = &scene.pet.grooming_path {
                    managed_file(base, Path::new(path))?;
                }
                scene.pet.apply(state);
            }
        }
        state.last_wallpaper_path = path;
        Ok(())
    })?;
    crate::notify_desktop_pet_changed();
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn pet_scene_grooming_survives_apply_and_export() {
        let (dir, mut scene) = fixture();
        let path = dir.path().join("ui/grooming.png");
        fs::write(&path, b"validated grooming strip").unwrap();
        scene.pet.sprite_version_number = Some(2);
        scene.pet.grooming_path = Some(path.to_string_lossy().into_owned());
        save_scene(dir.path(), scene.clone()).unwrap();
        let applied = apply_scene(dir.path(), &scene.id, SceneApplyMode::All).unwrap();
        assert_eq!(applied.grooming_path, scene.pet.grooming_path);
        let output = tempfile::tempdir().unwrap();
        let package = export_scene(dir.path(), &scene.id, output.path()).unwrap();
        let manifest: crate::DesktopPetManifest =
            serde_json::from_slice(&fs::read(package.join("pet.json")).unwrap()).unwrap();
        assert!(package
            .join(manifest.grooming_spritesheet_path.unwrap())
            .is_file());
    }
    fn fixture() -> (tempfile::TempDir, PetScene) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("ui");
        fs::create_dir_all(root.join("style/themes/pet-test")).unwrap();
        fs::write(root.join("pet.png"), b"prepared pet asset").unwrap();
        fs::write(
            root.join("style/themes/pet-test/wallpaper.png"),
            b"prepared wallpaper",
        )
        .unwrap();
        let pet = PetIdentity {
            pet_path: root.join("pet.png").to_string_lossy().into_owned(),
            source_path: None,
            sprite_version_number: None,
            grooming_path: None,
            display_name: Some("Mochi".into()),
            description: None,
            provider: None,
            model: None,
        };
        let scene = PetScene {
            id: "pet-test".into(),
            name: "Forest".into(),
            pet,
            style: Some(UiStyleManifest {
                schema_version: 1,
                id: "pet-test".into(),
                name: "Forest".into(),
                revision: "r1".into(),
                updated_at: "now".into(),
                tokens: Default::default(),
                icons: Default::default(),
                wallpaper: Some(crate::UiStyleWallpaper {
                    path: "themes/pet-test/wallpaper.png".into(),
                    fit: Default::default(),
                    shade: 18,
                    blur: 0,
                    adaptive_color: true,
                    recommended_theme: None,
                    accent_color: None,
                    secondary_color: None,
                }),
            }),
            wallpaper_source_path: Some("/original/wallpaper.png".into()),
        };
        (dir, scene)
    }

    #[test]
    fn pet_scene_save_is_preview_only_and_round_trips() {
        let (dir, scene) = fixture();
        let saved = save_scene(dir.path(), scene.clone()).unwrap();
        assert!(!saved.enabled);
        assert!(saved.pet_path.is_none());
        assert!(!crate::active_ui_style_path(dir.path()).exists());
        assert_eq!(
            crate::read_desktop_pet_state(dir.path()).unwrap().scenes,
            vec![scene]
        );
    }

    #[test]
    fn pet_scene_all_commits_pair_and_enables_linking() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        let state = apply_scene(dir.path(), &scene.id, SceneApplyMode::All).unwrap();
        assert!(state.enabled && state.follow_wallpaper);
        assert_eq!(state.pet_path, Some(scene.pet.pet_path));
        assert!(state.pending_scene_style.is_none());
        assert_eq!(
            crate::read_active_ui_style(dir.path()).unwrap().unwrap().id,
            scene.id
        );
    }

    #[test]
    fn pet_scene_wallpaper_only_does_not_switch_even_when_linked() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        crate::update_desktop_pet_state(dir.path(), |s| {
            s.follow_wallpaper = true;
            s.pet_path = Some("old-pet".into());
            Ok(())
        })
        .unwrap();
        let state = apply_scene(dir.path(), &scene.id, SceneApplyMode::Wallpaper).unwrap();
        assert_eq!(state.pet_path.as_deref(), Some("old-pet"));
        let state = sync_wallpaper(dir.path(), state.last_wallpaper_path.as_deref()).unwrap();
        assert_eq!(state.pet_path.as_deref(), Some("old-pet"));
    }

    #[test]
    fn pet_scene_pet_only_preserves_wallpaper_and_disables_linking() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        apply_scene(dir.path(), &scene.id, SceneApplyMode::All).unwrap();
        let style = fs::read(crate::active_ui_style_path(dir.path())).unwrap();
        let state = apply_scene(dir.path(), &scene.id, SceneApplyMode::Pet).unwrap();
        assert!(!state.follow_wallpaper);
        assert_eq!(
            fs::read(crate::active_ui_style_path(dir.path())).unwrap(),
            style
        );
    }

    #[test]
    fn pet_scene_unbound_wallpaper_keeps_pet_and_bound_switch_keeps_hidden() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        crate::update_desktop_pet_state(dir.path(), |s| {
            s.follow_wallpaper = true;
            s.pet_path = Some("old".into());
            Ok(())
        })
        .unwrap();
        let state = sync_wallpaper(dir.path(), Some("unbound")).unwrap();
        assert_eq!(state.pet_path.as_deref(), Some("old"));
        let state = sync_wallpaper(dir.path(), scene.wallpaper_source_path.as_deref()).unwrap();
        assert_eq!(state.pet_path, Some(scene.pet.pet_path));
        assert!(!state.enabled);
    }

    #[test]
    fn pet_scene_missing_asset_rejects_before_committing() {
        let (dir, scene) = fixture();
        let before = save_scene(dir.path(), scene.clone()).unwrap();
        fs::remove_file(scene.wallpaper_path(dir.path()).unwrap()).unwrap();
        assert!(apply_scene(dir.path(), &scene.id, SceneApplyMode::All).is_err());
        assert_eq!(crate::read_desktop_pet_state(dir.path()).unwrap(), before);
        assert!(!crate::active_ui_style_path(dir.path()).exists());
    }

    #[test]
    fn pet_scene_pending_style_is_recovered_after_interruption() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        crate::update_desktop_pet_state(dir.path(), |s| {
            scene.pet.apply(s);
            s.pending_scene_style = scene.style.clone();
            Ok(())
        })
        .unwrap();
        assert!(!crate::active_ui_style_path(dir.path()).exists());
        let recovered = crate::read_desktop_pet_state(dir.path()).unwrap();
        assert!(recovered.pending_scene_style.is_none());
        assert_eq!(
            crate::read_active_ui_style(dir.path()).unwrap().unwrap(),
            scene.style.unwrap()
        );
    }

    #[test]
    fn pet_scene_rejects_escaping_style_and_unmanaged_pet() {
        let (dir, mut scene) = fixture();
        scene
            .style
            .as_mut()
            .unwrap()
            .wallpaper
            .as_mut()
            .unwrap()
            .path = "../outside.png".into();
        assert!(save_scene(dir.path(), scene).is_err());
        let (_, mut scene) = fixture();
        scene.pet.pet_path = dir
            .path()
            .join("outside.png")
            .to_string_lossy()
            .into_owned();
        fs::write(&scene.pet.pet_path, b"outside").unwrap();
        assert!(save_scene(dir.path(), scene).is_err());
    }

    #[test]
    fn pet_scene_late_wallpaper_response_cannot_override_active_pair() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        let applied = apply_scene(dir.path(), &scene.id, SceneApplyMode::All).unwrap();
        let state = sync_wallpaper(dir.path(), Some("late-previous-wallpaper")).unwrap();
        assert_eq!(state.pet_path, applied.pet_path);
        assert_eq!(state.last_wallpaper_path, applied.last_wallpaper_path);
    }

    #[test]
    fn pet_scene_management_reuses_identity_and_renames_all_homes() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        let duplicate = edit_scene(
            dir.path(),
            PetSceneEdit::Duplicate {
                scene_id: scene.id.clone(),
                name: "Beach".into(),
            },
        )
        .unwrap();
        assert_eq!(duplicate.scenes.len(), 2);
        assert_eq!(duplicate.scenes[1].pet, scene.pet);
        assert!(duplicate.scenes[1].style.is_none());
        let renamed = edit_scene(
            dir.path(),
            PetSceneEdit::RenamePet {
                scene_id: scene.id.clone(),
                name: "Mochi".into(),
            },
        )
        .unwrap();
        assert!(renamed
            .scenes
            .iter()
            .all(|s| s.pet.display_name.as_deref() == Some("Mochi")));
        let renamed_scene = edit_scene(
            dir.path(),
            PetSceneEdit::Rename {
                scene_id: scene.id.clone(),
                name: "Garden".into(),
            },
        )
        .unwrap();
        assert_eq!(renamed_scene.scenes[0].name, "Garden");
        assert_eq!(renamed_scene.scenes[1].name, "Beach");
    }

    #[test]
    fn pet_scene_active_delete_requires_confirmation_and_keeps_files() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        apply_scene(dir.path(), &scene.id, SceneApplyMode::All).unwrap();
        edit_scene(
            dir.path(),
            PetSceneEdit::Favorite {
                scene_id: scene.id.clone(),
                favorite: true,
            },
        )
        .unwrap();
        assert!(edit_scene(
            dir.path(),
            PetSceneEdit::Delete {
                scene_id: scene.id.clone(),
                confirm_active: false
            }
        )
        .is_err());
        let state = edit_scene(
            dir.path(),
            PetSceneEdit::Delete {
                scene_id: scene.id.clone(),
                confirm_active: true,
            },
        )
        .unwrap();
        assert!(state.scenes.is_empty() && state.favorite_scene_ids.is_empty());
        assert_eq!(state.pet_path.as_deref(), Some(scene.pet.pet_path.as_str()));
        assert!(Path::new(&scene.pet.pet_path).exists());
        assert!(scene.wallpaper_path(dir.path()).unwrap().exists());
    }

    #[test]
    fn pet_scene_export_is_portable_and_excludes_original_photo() {
        let (dir, mut scene) = fixture();
        scene.pet.source_path = Some("private-original-photo.jpg".into());
        save_scene(dir.path(), scene.clone()).unwrap();
        let output = tempfile::tempdir().unwrap();
        let package = export_scene(dir.path(), &scene.id, output.path()).unwrap();
        let data = fs::read_to_string(package.join("scene.json")).unwrap();
        assert!(!data.contains("private-original"));
        let exported: PetScene = serde_json::from_str(&data).unwrap();
        assert!(exported.pet.source_path.is_none());
        assert!(package.join(exported.pet.pet_path).is_file());
        assert!(package
            .join(exported.style.unwrap().wallpaper.unwrap().path)
            .is_file());
        assert_ne!(
            export_scene(dir.path(), &scene.id, output.path()).unwrap(),
            package
        );
    }

    #[test]
    fn pet_scene_pause_survives_scene_switch_and_restart() {
        let (dir, scene) = fixture();
        save_scene(dir.path(), scene.clone()).unwrap();
        edit_scene(dir.path(), PetSceneEdit::Pause { paused: true }).unwrap();
        apply_scene(dir.path(), &scene.id, SceneApplyMode::All).unwrap();
        assert!(
            crate::read_desktop_pet_state(dir.path())
                .unwrap()
                .animation_paused
        );
    }
}
