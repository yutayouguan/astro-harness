//! One-step appearance changes and guarded undo. Library records are never restored wholesale.
use crate::{
    pet_scene::{PetIdentity, SceneApplyMode},
    DesktopPetState, UiStyleManifest,
};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum AmbienceChange {
    Scene {
        scene_id: String,
        expected_pet_id: Option<String>,
    },
    Wallpaper {
        path: Option<String>,
    },
    /// Keep the background, non-palette tokens and icons; save only an active-style variant.
    Palette {
        adaptive_color: bool,
        colors: Option<[String; 2]>,
        dark_colors: Option<[String; 2]>,
    },
}

#[derive(Clone, PartialEq)]
struct Appearance {
    pet: Option<PetIdentity>,
    scene: Option<String>,
    follow: bool,
    last_wallpaper: Option<String>,
    enabled: bool,
    scale: f64,
    preferences: crate::pet_preferences::PetPreferences,
    style: Option<UiStyleManifest>,
}

impl Appearance {
    fn capture(base: &Path, state: &DesktopPetState) -> anyhow::Result<Self> {
        Ok(Self {
            pet: state
                .pet_path
                .as_ref()
                .map(|_| PetIdentity::from_state(state))
                .transpose()?,
            scene: state.active_scene_id.clone(),
            follow: state.follow_wallpaper,
            last_wallpaper: state.last_wallpaper_path.clone(),
            enabled: state.enabled,
            scale: state.scale,
            preferences: state.preferences.clone(),
            style: crate::read_active_ui_style(base).map_err(anyhow::Error::msg)?,
        })
    }
    fn restore(&self, state: &mut DesktopPetState) {
        if let Some(pet) = &self.pet {
            pet.apply(state);
        } else {
            state.pet_path = None;
            state.active_pet_id = None;
            state.grooming_path = None;
            state.motion_clips.clear();
            state.source_path = None;
            state.sprite_version_number = None;
            state.display_name = None;
            state.description = None;
            state.provider = None;
            state.model = None;
        }
        state.active_scene_id = self.scene.clone();
        state.follow_wallpaper = self.follow;
        state.last_wallpaper_path = self.last_wallpaper.clone();
        state.enabled = self.enabled;
        state.scale = self.scale;
        state.preferences = self.preferences.clone();
        state.pending_scene_style = self.style.clone();
        state.pending_style_reset = self.style.is_none();
    }
}

pub struct AmbienceUndo {
    before: Appearance,
    after: Appearance,
}

pub(crate) fn publish_style_reset(base: &Path) -> anyhow::Result<()> {
    let active = crate::active_ui_style_path(base);
    if !active.try_exists()? {
        return Ok(());
    }
    let bytes = crate::desktop_pet::read_limited_pet_file(
        &active,
        crate::ui_style::UI_STYLE_MAX_MANIFEST_BYTES,
    )?;
    crate::pet_scene::atomic_write(
        &crate::ui_style_root(base)
            .join("history")
            .join(format!("ambience-{}.json", uuid::Uuid::new_v4())),
        &bytes,
    )?;
    std::fs::remove_file(active)?;
    Ok(())
}

fn finish(base: &Path) -> anyhow::Result<DesktopPetState> {
    let state = crate::read_desktop_pet_state(base)?;
    crate::notify_ui_style_changed();
    crate::notify_desktop_pet_changed();
    Ok(state)
}

pub fn change(
    base: &Path,
    request: AmbienceChange,
) -> anyhow::Result<(DesktopPetState, AmbienceUndo)> {
    let mut before = None;
    let mut after = None;
    crate::update_desktop_pet_state(base, |state| {
        before = Some(Appearance::capture(base, state)?);
        match request {
            AmbienceChange::Scene {
                scene_id,
                expected_pet_id,
            } => {
                if let Some(pet_id) = expected_pet_id {
                    anyhow::ensure!(
                        state.active_pet_id.as_deref() == Some(pet_id.as_str()),
                        "当前宠物已切换，请重新选择换景范围"
                    );
                    anyhow::ensure!(
                        state
                            .scenes
                            .iter()
                            .any(|scene| scene.id == scene_id && scene.pet.pet_id == pet_id),
                        "该场景不属于当前宠物"
                    );
                }
                crate::pet_scene::prepare_scene(base, state, &scene_id, SceneApplyMode::All)?
            }
            AmbienceChange::Wallpaper { path } => {
                let path = path
                    .map(|p| crate::pet_scene::managed_file(base, Path::new(&p)))
                    .transpose()?;
                state.last_wallpaper_path = path.map(|p| p.to_string_lossy().into_owned());
                state.active_scene_id = None;
                state.follow_wallpaper = false;
                state.pending_scene_style = None;
                state.pending_style_reset = true;
            }
            AmbienceChange::Palette {
                adaptive_color,
                colors,
                dark_colors,
            } => {
                for colors in [&colors, &dark_colors].into_iter().flatten() {
                    anyhow::ensure!(
                        colors.iter().all(|c| c.len() == 7
                            && c.starts_with('#')
                            && c[1..].bytes().all(|b| b.is_ascii_hexdigit())),
                        "配色必须是六位十六进制颜色"
                    );
                }
                if let Some(mut style) =
                    crate::read_active_ui_style(base).map_err(anyhow::Error::msg)?
                {
                    style.id = format!("ambience-{}", uuid::Uuid::new_v4().simple());
                    style.revision = uuid::Uuid::new_v4().to_string();
                    if let Some(wallpaper) = &mut style.wallpaper {
                        wallpaper.adaptive_color = adaptive_color;
                    }
                    for (tokens, colors) in [
                        (&mut style.tokens.light, colors.as_ref()),
                        (
                            &mut style.tokens.dark,
                            dark_colors.as_ref().or(colors.as_ref()),
                        ),
                    ] {
                        for (index, key) in ["--color-accent", "--color-accent-secondary"]
                            .iter()
                            .enumerate()
                        {
                            if let Some(colors) = colors {
                                tokens.insert(key.to_string(), colors[index].clone());
                            } else if adaptive_color {
                                tokens.remove(*key);
                            }
                        }
                    }
                    state.pending_scene_style = Some(style);
                }
            }
        }
        let mut expected = Appearance::capture(base, state)?;
        if state.pending_style_reset {
            expected.style = None;
        } else if let Some(style) = &state.pending_scene_style {
            expected.style = Some(style.clone());
        }
        after = Some(expected);
        Ok(())
    })?;
    let state = finish(base)?;
    Ok((
        state,
        AmbienceUndo {
            before: before.expect("committed change"),
            after: after.expect("committed change"),
        },
    ))
}

pub fn undo(base: &Path, record: &AmbienceUndo) -> anyhow::Result<DesktopPetState> {
    crate::update_desktop_pet_state(base, |state| {
        anyhow::ensure!(
            Appearance::capture(base, state)? == record.after,
            "外观或桌宠已在其他位置修改，无法撤销这一步"
        );
        if let Some(id) = &record.before.scene {
            anyhow::ensure!(
                state.scenes.iter().any(|s| &s.id == id),
                "原场景已删除，无法撤销"
            );
        }
        if let Some(pet) = &record.before.pet {
            anyhow::ensure!(
                state.pets.iter().any(|p| p.id == pet.pet_id),
                "原宠物已删除，无法撤销"
            );
            crate::pet_scene::managed_file(base, Path::new(&pet.pet_path))?;
            if let Some(path) = &pet.grooming_path {
                crate::pet_scene::managed_file(base, Path::new(path))?;
            }
            for clip in pet.motion_clips.values() {
                crate::pet_scene::managed_file(base, Path::new(&clip.path))?;
            }
        }
        if let Some(style) = &record.before.style {
            if let Some(wallpaper) = &style.wallpaper {
                crate::pet_scene::managed_file(
                    base,
                    &crate::ui_style_root(base).join(&wallpaper.path),
                )?;
            }
        }
        record.before.restore(state);
        Ok(())
    })?;
    finish(base)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pet_scene::{atomic_write, save_scene, PetScene};
    fn setup(base: &Path) {
        let pet = base.join("ui/pet.png");
        atomic_write(&pet, b"previously validated pet").unwrap();
        atomic_write(
            &crate::ui_style_root(base).join("room.png"),
            b"previously validated wallpaper",
        )
        .unwrap();
        let scene = PetScene {
            id: "pet-room".into(),
            name: "Room".into(),
            pet: PetIdentity {
                pet_id: "pet-one".into(),
                pet_path: pet.to_string_lossy().into_owned(),
                grooming_path: None,
                motion_clips: Default::default(),
                source_path: None,
                sprite_version_number: None,
                display_name: Some("One".into()),
                description: None,
                provider: None,
                model: None,
            },
            style: Some(UiStyleManifest {
                schema_version: 1,
                id: "pet-room".into(),
                name: "Room".into(),
                revision: "r1".into(),
                updated_at: "now".into(),
                tokens: Default::default(),
                icons: Default::default(),
                wallpaper: Some(crate::UiStyleWallpaper {
                    path: "room.png".into(),
                    fit: Default::default(),
                    shade: 18,
                    blur: 0,
                    adaptive_color: true,
                    recommended_theme: None,
                    accent_color: None,
                    secondary_color: None,
                }),
            }),
            wallpaper_source_path: None,
            preferences: None,
        };
        save_scene(base, scene).unwrap();
    }
    #[test]
    fn scene_undo_restores_appearance_but_keeps_library_edits() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        setup(base);
        let before = crate::read_desktop_pet_state(base).unwrap();
        let (applied, record) = change(
            base,
            AmbienceChange::Scene {
                scene_id: "pet-room".into(),
                expected_pet_id: None,
            },
        )
        .unwrap();
        assert!(applied.enabled);
        assert!(crate::read_active_ui_style(base).unwrap().is_some());
        crate::update_desktop_pet_state(base, |s| {
            s.scenes[0].name = "New name".into();
            s.always_on_top = false;
            Ok(())
        })
        .unwrap();
        let restored = undo(base, &record).unwrap();
        assert_eq!(restored.pet_path, before.pet_path);
        assert_eq!(restored.enabled, before.enabled);
        assert_eq!(restored.scenes[0].name, "New name");
        assert!(!restored.always_on_top);
        assert!(crate::read_active_ui_style(base).unwrap().is_none());
    }
    #[test]
    fn wallpaper_only_keeps_pet_and_undo_restores_pair() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        setup(base);
        let (before, _) = change(
            base,
            AmbienceChange::Scene {
                scene_id: "pet-room".into(),
                expected_pet_id: None,
            },
        )
        .unwrap();
        let (after, record) = change(
            base,
            AmbienceChange::Wallpaper {
                path: Some(
                    crate::ui_style_root(base)
                        .join("room.png")
                        .to_string_lossy()
                        .into_owned(),
                ),
            },
        )
        .unwrap();
        assert_eq!(after.pet_path, before.pet_path);
        assert!(!after.follow_wallpaper);
        assert!(after.active_scene_id.is_none());
        assert!(crate::read_active_ui_style(base).unwrap().is_none());
        let restored = undo(base, &record).unwrap();
        assert_eq!(restored.active_scene_id, before.active_scene_id);
        assert!(restored.follow_wallpaper);
        assert!(crate::read_active_ui_style(base).unwrap().is_some());
    }
    #[test]
    fn palette_keeps_scene_and_conflicting_undo_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        setup(base);
        let (before, _) = change(
            base,
            AmbienceChange::Scene {
                scene_id: "pet-room".into(),
                expected_pet_id: None,
            },
        )
        .unwrap();
        let (after, record) = change(
            base,
            AmbienceChange::Palette {
                adaptive_color: false,
                colors: Some(["#123456".into(), "#789abc".into()]),
                dark_colors: Some(["#aabbcc".into(), "#ddeeff".into()]),
            },
        )
        .unwrap();
        assert_eq!(after.active_scene_id, before.active_scene_id);
        let variant = crate::read_active_ui_style(base).unwrap().unwrap();
        assert!(variant.id.starts_with("ambience-"));
        assert_eq!(variant.tokens.light["--color-accent"], "#123456");
        assert_eq!(variant.tokens.dark["--color-accent"], "#aabbcc");
        assert_eq!(variant.wallpaper.unwrap().path, "room.png");
        assert_eq!(after.last_wallpaper_path, before.last_wallpaper_path);
        crate::update_desktop_pet_state(base, |s| {
            s.preferences.quiet_mode = true;
            Ok(())
        })
        .unwrap();
        assert!(undo(base, &record)
            .unwrap_err()
            .to_string()
            .contains("其他位置"));
        assert!(
            crate::read_desktop_pet_state(base)
                .unwrap()
                .preferences
                .quiet_mode
        );
    }
    #[test]
    fn reset_outbox_recovers_and_invalid_targets_do_not_commit() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        setup(base);
        change(
            base,
            AmbienceChange::Scene {
                scene_id: "pet-room".into(),
                expected_pet_id: None,
            },
        )
        .unwrap();
        crate::update_desktop_pet_state(base, |s| {
            s.pending_style_reset = true;
            Ok(())
        })
        .unwrap();
        assert!(
            !crate::read_desktop_pet_state(base)
                .unwrap()
                .pending_style_reset
        );
        assert!(crate::read_active_ui_style(base).unwrap().is_none());
        let before = crate::read_desktop_pet_state(base).unwrap();
        assert!(change(
            base,
            AmbienceChange::Wallpaper {
                path: Some("/etc/passwd".into())
            }
        )
        .is_err());
        assert_eq!(crate::read_desktop_pet_state(base).unwrap(), before);
    }

    #[test]
    fn scoped_shuffle_rejects_stale_pet_and_cross_pet_scene_without_changes() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        setup(base);
        let mut other = crate::read_desktop_pet_state(base).unwrap().scenes[0].clone();
        other.id = "pet-other".into();
        other.style.as_mut().unwrap().id = other.id.clone();
        other.pet.pet_id = "pet-two".into();
        let other_path = base.join("ui/other.png");
        atomic_write(&other_path, b"validated other pet").unwrap();
        other.pet.pet_path = other_path.to_string_lossy().into_owned();
        save_scene(base, other).unwrap();
        let (before, _) = change(
            base,
            AmbienceChange::Scene {
                scene_id: "pet-room".into(),
                expected_pet_id: None,
            },
        )
        .unwrap();
        let pet_id = before.active_pet_id.clone().unwrap();
        for (scene_id, expected_pet_id) in
            [("pet-room", "stale-pet"), ("pet-other", pet_id.as_str())]
        {
            assert!(change(
                base,
                AmbienceChange::Scene {
                    scene_id: scene_id.into(),
                    expected_pet_id: Some(expected_pet_id.into())
                }
            )
            .is_err());
            assert_eq!(crate::read_desktop_pet_state(base).unwrap(), before);
        }
        assert!(change(
            base,
            AmbienceChange::Scene {
                scene_id: "pet-room".into(),
                expected_pet_id: Some(pet_id)
            }
        )
        .is_ok());
    }
}
