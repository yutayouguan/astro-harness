//! Stable pet identity and defaults. Scene identity snapshots remain exportable,
//! but are refreshed from the library by id under the shared state transaction.
use crate::{pet_preferences::PetScenePreferences, pet_scene::PetIdentity, DesktopPetState};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PetRecord {
    pub id: String,
    pub identity: PetIdentity,
    pub defaults: PetScenePreferences,
    #[serde(default)]
    pub builtin: bool,
}

pub fn register_identity(
    state: &mut DesktopPetState,
    identity: &mut PetIdentity,
) -> anyhow::Result<()> {
    // Only legacy/path-only writers resolve by path. Once assigned, the id owns
    // the identity even when its files move or its animation is upgraded.
    if identity.pet_id.is_empty() {
        identity.pet_id = state
            .pets
            .iter()
            .find(|p| p.identity.pet_path == identity.pet_path)
            .map(|p| p.id.clone())
            .unwrap_or_else(|| format!("companion-{}", uuid::Uuid::new_v4().simple()));
    }
    if !state.pets.iter().any(|p| p.id == identity.pet_id) {
        let defaults = if state.pet_path.as_deref() == Some(identity.pet_path.as_str()) {
            PetScenePreferences::from_state(state)
        } else {
            PetScenePreferences::from_state(&DesktopPetState::default())
        };
        state.pets.push(PetRecord {
            id: identity.pet_id.clone(),
            identity: identity.clone(),
            defaults,
            builtin: false,
        });
    }
    Ok(())
}

pub fn reconcile(state: &mut DesktopPetState) -> anyhow::Result<()> {
    for i in 0..state.scenes.len() {
        let mut identity = state.scenes[i].pet.clone();
        register_identity(state, &mut identity)?;
        state.scenes[i].pet = identity;
    }
    if state.pet_path.is_some() {
        let mut identity = PetIdentity::from_state(state)?;
        // Legacy tool writers change the path without assigning an id. Never
        // overwrite the previously selected pet with that newly generated art.
        if state
            .pets
            .iter()
            .find(|p| p.id == identity.pet_id)
            .is_some_and(|p| p.identity.pet_path != identity.pet_path)
        {
            identity.pet_id.clear();
        }
        register_identity(state, &mut identity)?;
        state.active_pet_id = Some(identity.pet_id);
    } else {
        state.active_pet_id = None;
        state.active_scene_id = None;
    }
    for scene in &mut state.scenes {
        if let Some(record) = state.pets.iter().find(|p| p.id == scene.pet.pet_id) {
            scene.pet = record.identity.clone();
        }
    }
    if state.active_scene_id.as_ref().is_some_and(|id| {
        !state
            .scenes
            .iter()
            .any(|s| &s.id == id && Some(&s.pet.pet_id) == state.active_pet_id.as_ref())
    }) {
        state.active_scene_id = None;
    }
    state.library_version = 1;
    Ok(())
}

pub fn validate(state: &DesktopPetState) -> anyhow::Result<()> {
    anyhow::ensure!(state.library_version == 1, "不支持的宠物库版本");
    anyhow::ensure!(state.pets.len() <= 200, "宠物库已达 200 只上限");
    let mut ids = std::collections::HashSet::new();
    for pet in &state.pets {
        anyhow::ensure!(
            !pet.id.is_empty()
                && pet.id.len() <= 100
                && pet
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                && pet.identity.pet_id == pet.id
                && ids.insert(&pet.id),
            "宠物 id 无效或重复"
        );
        pet.defaults.validate()?;
        crate::pet_motion::validate_motion_clips(&pet.identity.motion_clips)?;
    }
    if state.library_version > 0 {
        anyhow::ensure!(
            state.scenes.iter().all(|s| ids.contains(&s.pet.pet_id)),
            "场景所属宠物不存在"
        );
        anyhow::ensure!(
            state
                .active_pet_id
                .as_ref()
                .is_none_or(|id| ids.contains(id)),
            "当前宠物不存在"
        );
    }
    Ok(())
}

pub fn apply_pet(base: &Path, id: &str) -> anyhow::Result<DesktopPetState> {
    crate::update_desktop_pet_state(base, |state| {
        let pet = state
            .pets
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("宠物不存在"))?;
        crate::pet_scene::managed_file(base, Path::new(&pet.identity.pet_path))?;
        for clip in pet.identity.motion_clips.values() {
            crate::pet_scene::managed_file(base, Path::new(&clip.path))?;
        }
        if let Some(path) = &pet.identity.grooming_path {
            crate::pet_scene::managed_file(base, Path::new(path))?;
        }
        pet.identity.apply(state);
        pet.defaults.apply(state);
        state.active_scene_id = None;
        state.enabled = true;
        state.follow_wallpaper = false;
        Ok(())
    })
}

#[derive(Debug, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "snake_case",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum PetLibraryEdit {
    Rename {
        pet_id: String,
        name: String,
    },
    Delete {
        pet_id: String,
        confirm_scene_count: usize,
        confirm_active: bool,
    },
    SetDefaults {
        pet_id: String,
        defaults: PetScenePreferences,
    },
    AddScene {
        pet_id: String,
        name: String,
    },
}

fn name_valid(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !name.trim().is_empty()
            && name.chars().count() <= 80
            && !name.chars().any(char::is_control),
        "名称应为 1–80 个字符"
    );
    Ok(())
}

pub fn edit_library(base: &Path, edit: PetLibraryEdit) -> anyhow::Result<DesktopPetState> {
    crate::update_desktop_pet_state(base, |state| {
        let id = match &edit {
            PetLibraryEdit::Rename { pet_id, .. }
            | PetLibraryEdit::Delete { pet_id, .. }
            | PetLibraryEdit::SetDefaults { pet_id, .. }
            | PetLibraryEdit::AddScene { pet_id, .. } => pet_id,
        };
        let index = state
            .pets
            .iter()
            .position(|p| &p.id == id)
            .ok_or_else(|| anyhow::anyhow!("宠物不存在"))?;
        match edit {
            PetLibraryEdit::Rename { pet_id, name } => {
                name_valid(&name)?;
                state.pets[index].identity.display_name = Some(name.trim().into());
                if state.active_pet_id.as_ref() == Some(&pet_id) {
                    state.display_name = Some(name.trim().into());
                }
            }
            PetLibraryEdit::SetDefaults { defaults, .. } => {
                defaults.validate()?;
                // Editing a library default does not change the running desktop.
                state.pets[index].defaults = defaults;
            }
            PetLibraryEdit::AddScene { name, .. } => {
                name_valid(&name)?;
                state.scenes.push(crate::pet_scene::PetScene {
                    id: format!("pet-{}", uuid::Uuid::new_v4().simple()),
                    name: name.trim().into(),
                    pet: state.pets[index].identity.clone(),
                    style: None,
                    wallpaper_source_path: None,
                    preferences: None,
                });
            }
            PetLibraryEdit::Delete {
                pet_id,
                confirm_scene_count,
                confirm_active,
            } => {
                anyhow::ensure!(!state.pets[index].builtin, "内置宠物不能删除");
                let count = state
                    .scenes
                    .iter()
                    .filter(|s| s.pet.pet_id == pet_id)
                    .count();
                anyhow::ensure!(
                    count == confirm_scene_count,
                    "关联场景已变化，请重新确认删除范围"
                );
                if state.active_pet_id.as_ref() == Some(&pet_id) {
                    anyhow::ensure!(confirm_active, "当前宠物正在使用，请确认隐藏后移除");
                    state.enabled = false;
                    state.pet_path = None;
                    state.active_pet_id = None;
                    state.active_scene_id = None;
                    state.grooming_path = None;
                    state.motion_clips.clear();
                    state.sprite_version_number = None;
                    state.display_name = None;
                    state.description = None;
                    state.provider = None;
                    state.model = None;
                    state.follow_wallpaper = false;
                }
                state.scenes.retain(|s| s.pet.pet_id != pet_id);
                state
                    .favorite_scene_ids
                    .retain(|id| state.scenes.iter().any(|s| &s.id == id));
                state.pets.remove(index);
                // Intentionally retain all files for recovery and shared wallpaper use.
            }
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (tempfile::TempDir, DesktopPetState) {
        let dir = tempfile::tempdir().unwrap();
        let path = crate::desktop_pet_root(dir.path()).join("cat.png");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"fixture").unwrap();
        let state = crate::update_desktop_pet_state(dir.path(), |s| {
            s.pet_path = Some(path.to_string_lossy().into());
            Ok(())
        })
        .unwrap();
        (dir, state)
    }
    #[test]
    fn library_keeps_identity_when_assets_change_and_scene_is_deleted() {
        let (dir, state) = fixture();
        let id = state.pets[0].id.clone();
        let saved = edit_library(
            dir.path(),
            PetLibraryEdit::AddScene {
                pet_id: id.clone(),
                name: "Home".into(),
            },
        )
        .unwrap();
        let scene_id = saved.scenes[0].id.clone();
        let changed = crate::update_desktop_pet_state(dir.path(), |s| {
            s.pets[0].identity.pet_path = "updated.png".into();
            s.pet_path = Some("updated.png".into());
            Ok(())
        })
        .unwrap();
        assert_eq!(changed.scenes[0].pet.pet_id, id);
        assert_eq!(changed.scenes[0].pet.pet_path, "updated.png");
        let deleted = crate::pet_scene::edit_scene(
            dir.path(),
            crate::pet_scene::PetSceneEdit::Delete {
                scene_id,
                confirm_active: true,
            },
        )
        .unwrap();
        assert_eq!(deleted.pets.len(), 1);
    }
    #[test]
    fn library_new_path_does_not_replace_old_pet_and_preferences_do_not_apply_on_edit() {
        let (dir, state) = fixture();
        let mut defaults = state.pets[0].defaults.clone();
        defaults.scale = 0.25;
        let saved = edit_library(
            dir.path(),
            PetLibraryEdit::SetDefaults {
                pet_id: state.pets[0].id.clone(),
                defaults,
            },
        )
        .unwrap();
        assert_eq!(saved.scale, state.scale);
        let new = crate::update_desktop_pet_state(dir.path(), |s| {
            s.pet_path = Some("new.png".into());
            Ok(())
        })
        .unwrap();
        assert_eq!(new.pets.len(), 2);
        assert_ne!(new.active_pet_id, state.active_pet_id);
    }

    #[test]
    fn saved_oversized_pet_and_scene_scales_load_at_the_new_ceiling() {
        let (dir, state) = fixture();
        let mut legacy = edit_library(
            dir.path(),
            PetLibraryEdit::AddScene {
                pet_id: state.pets[0].id.clone(),
                name: "Existing scene".into(),
            },
        )
        .unwrap();
        legacy.scale = 0.6;
        legacy.pets[0].defaults.scale = 0.4;
        legacy.scenes[0].preferences = Some(PetScenePreferences {
            scale: 0.5,
            behavior: legacy.preferences.clone(),
        });
        let path = crate::desktop_pet_state_path(dir.path());
        let bytes = serde_json::to_vec(&legacy).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let mut expected = legacy;
        expected.scale = 0.3;
        expected.pets[0].defaults.scale = 0.3;
        expected.scenes[0].preferences.as_mut().unwrap().scale = 0.3;
        assert_eq!(crate::read_desktop_pet_state(dir.path()).unwrap(), expected);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        let saved = crate::update_desktop_pet_state(dir.path(), |_| Ok(())).unwrap();
        expected.revision += 1;
        assert_eq!(saved, expected);
        assert_eq!(crate::read_desktop_pet_state(dir.path()).unwrap(), expected);
    }
    #[test]
    fn library_delete_checks_scene_count_and_keeps_files() {
        let (dir, state) = fixture();
        let id = state.pets[0].id.clone();
        edit_library(
            dir.path(),
            PetLibraryEdit::AddScene {
                pet_id: id.clone(),
                name: "Home".into(),
            },
        )
        .unwrap();
        assert!(edit_library(
            dir.path(),
            PetLibraryEdit::Delete {
                pet_id: id.clone(),
                confirm_scene_count: 0,
                confirm_active: true
            }
        )
        .is_err());
        let removed = edit_library(
            dir.path(),
            PetLibraryEdit::Delete {
                pet_id: id,
                confirm_scene_count: 1,
                confirm_active: true,
            },
        )
        .unwrap();
        assert!(removed.pets.is_empty() && removed.scenes.is_empty() && !removed.enabled);
        assert!(Path::new(state.pet_path.as_ref().unwrap()).exists());
    }

    #[test]
    fn legacy_migration_is_backed_up_once_and_ids_survive_reads() {
        let dir = tempfile::tempdir().unwrap();
        let path = crate::desktop_pet_state_path(dir.path());
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let legacy =
            br#"{"revision":8,"petPath":"legacy.png","scale":0.4,"displayName":"Keep name"}"#;
        std::fs::write(&path, legacy).unwrap();
        let first = crate::read_desktop_pet_state(dir.path()).unwrap();
        assert_eq!(first.library_version, 1);
        assert_eq!(first.pets.len(), 1);
        assert_eq!(first.revision, 9);
        assert_eq!(
            first.pets[0].identity.display_name.as_deref(),
            Some("Keep name")
        );
        assert_eq!(crate::read_desktop_pet_state(dir.path()).unwrap(), first);
        assert_eq!(
            std::fs::read(path.parent().unwrap().join("state.before-pet-library.json")).unwrap(),
            legacy
        );
    }

    #[test]
    fn inherited_scene_uses_latest_defaults_and_keeps_global_hiding() {
        let (dir, state) = fixture();
        let id = state.pets[0].id.clone();
        let saved = edit_library(
            dir.path(),
            PetLibraryEdit::AddScene {
                pet_id: id.clone(),
                name: "Home".into(),
            },
        )
        .unwrap();
        let scene_id = saved.scenes[0].id.clone();
        let mut defaults = saved.pets[0].defaults.clone();
        defaults.scale = 0.25;
        defaults.behavior.hide_in_fullscreen = false;
        edit_library(
            dir.path(),
            PetLibraryEdit::SetDefaults {
                pet_id: id,
                defaults,
            },
        )
        .unwrap();
        crate::update_desktop_pet_state(dir.path(), |s| {
            s.preferences.presentation_mode = true;
            Ok(())
        })
        .unwrap();
        let applied = crate::pet_scene::apply_scene(
            dir.path(),
            &scene_id,
            crate::pet_scene::SceneApplyMode::Pet,
        )
        .unwrap();
        assert_eq!(applied.scale, 0.25);
        assert!(applied.preferences.presentation_mode && applied.preferences.hide_in_fullscreen);
        assert_eq!(applied.active_scene_id.as_deref(), Some(scene_id.as_str()));
        let mut override_prefs = applied.pets[0].defaults.clone();
        override_prefs.scale = 0.3;
        crate::pet_scene::edit_scene(
            dir.path(),
            crate::pet_scene::PetSceneEdit::SetPreferences {
                scene_id: scene_id.clone(),
                preferences: Some(override_prefs),
            },
        )
        .unwrap();
        let applied = crate::pet_scene::apply_scene(
            dir.path(),
            &scene_id,
            crate::pet_scene::SceneApplyMode::Pet,
        )
        .unwrap();
        assert_eq!(applied.scale, 0.3);
        assert_eq!(applied.pets[0].defaults.scale, 0.25);
    }
}
