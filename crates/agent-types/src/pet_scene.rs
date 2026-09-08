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
            sprite_version_number: state.sprite_version_number,
            display_name: state.display_name.clone(),
            description: state.description.clone(),
            provider: state.provider.clone(),
            model: state.model.clone(),
        })
    }

    fn apply(&self, state: &mut DesktopPetState) {
        state.pet_path = Some(self.pet_path.clone());
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
    let root = base.join("ui").canonicalize()?;
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
}
