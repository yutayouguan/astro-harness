//! Offline, versioned companion assets shared by onboarding and Settings.
use std::{fs, io::Write, path::Path};

pub(super) const SPRITESHEET: &[u8] =
    include_bytes!("../../../../src/assets/pets/naitang/spritesheet.webp");
pub(super) const GROOMING: &[u8] =
    include_bytes!("../../../../src/assets/pets/naitang/grooming.webp");
const KNEADING_MOTION: &[u8] =
    include_bytes!("../../../../src/assets/pets/naitang/kneading-motion.webp");
const GROOMING_MOTION: &[u8] =
    include_bytes!("../../../../src/assets/pets/naitang/grooming-motion.webp");
const MOTION_SPEC: &str = include_str!("../../../../src/assets/pets/naitang/motion-clips.json");

/// Caller holds the shared pet-state transaction. Publish state only after both
/// assets are durable; a failed write never switches the active companion.
pub(super) fn install_into_state(
    base: &Path,
    state: &mut types::DesktopPetState,
) -> anyhow::Result<()> {
    let mut motion_clips: types::pet_motion::PetMotionClips = serde_json::from_str(MOTION_SPEC)?;
    types::pet_motion::validate_motion_clips(&motion_clips)?;
    for (name, bytes) in [("kneading", KNEADING_MOTION), ("grooming", GROOMING_MOTION)] {
        super::desktop_pet::validate_motion_image(bytes, &motion_clips[name])
            .map_err(anyhow::Error::msg)?;
    }
    let directory = types::desktop_pet_root(base).join("builtin-naitang-v2");
    fs::create_dir_all(&directory)?;
    for (name, bytes) in [
        ("spritesheet.webp", SPRITESHEET),
        ("grooming.webp", GROOMING),
        ("kneading-motion.webp", KNEADING_MOTION),
        ("grooming-motion.webp", GROOMING_MOTION),
    ] {
        let path = directory.join(name);
        if fs::read(&path).is_ok_and(|existing| existing == bytes) {
            continue;
        }
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        file.persist(&path)?;
    }
    state.pet_path = Some(
        directory
            .join("spritesheet.webp")
            .to_string_lossy()
            .into_owned(),
    );
    state.grooming_path = Some(
        directory
            .join("grooming.webp")
            .to_string_lossy()
            .into_owned(),
    );
    state.sprite_version_number = Some(types::DESKTOP_PET_V2_SPRITE_VERSION);
    for clip in motion_clips.values_mut() {
        clip.path = directory.join(&clip.path).to_string_lossy().into_owned();
    }
    state.motion_clips = motion_clips;
    state.display_name = Some("奶糖".into());
    state.description = Some("内置橘白猫：眨眼、踩奶、舔脚脚与16个注视方向，无需模型生成".into());
    state.source_path = None;
    state.provider = None;
    state.model = None;
    state.follow_wallpaper = false;
    state.animation_paused = false;
    Ok(())
}

/// Only upgrade our untouched, versioned built-in; never user-imported art.
pub(super) fn upgrade_at(base: &Path) -> anyhow::Result<()> {
    let old = types::desktop_pet_root(base).join("builtin-naitang-v1/spritesheet.webp");
    let current = types::read_desktop_pet_state(base)?;
    if current.pet_path.as_deref() != old.to_str()
        || !current.motion_clips.is_empty()
        || fs::read(&old).ok().as_deref() != Some(SPRITESHEET)
    {
        return Ok(());
    }
    types::update_desktop_pet_state(base, |state| {
        if state.pet_path.as_deref() != old.to_str()
            || !state.motion_clips.is_empty()
            || fs::read(&old).ok().as_deref() != Some(SPRITESHEET)
        {
            return Ok(());
        }
        let paused = state.animation_paused;
        let follow = state.follow_wallpaper;
        let previous = state.clone();
        install_into_state(base, state)?;
        state.animation_paused = paused;
        state.follow_wallpaper = follow;
        state.display_name = previous.display_name;
        state.description = previous.description;
        state.source_path = previous.source_path;
        state.provider = previous.provider;
        state.model = previous.model;
        for scene in &mut state.scenes {
            if scene.pet.pet_path == old.to_string_lossy() && scene.pet.motion_clips.is_empty() {
                scene.pet.pet_path = state.pet_path.clone().unwrap();
                scene.pet.grooming_path = state.grooming_path.clone();
                scene.pet.motion_clips = state.motion_clips.clone();
            }
        }
        Ok(())
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_installs_both_assets_and_repairs_missing_files() {
        let root = tempfile::tempdir().unwrap();
        let mut state = types::DesktopPetState::default();
        state.scale = 0.5;
        install_into_state(root.path(), &mut state).unwrap();
        let original = state.pet_path.clone();
        assert_eq!(state.sprite_version_number, Some(2));
        assert_eq!(state.scale, 0.5);
        assert_eq!(state.motion_clips["kneading"].durations_ms.len(), 16);
        assert_eq!(state.motion_clips["grooming"].durations_ms.len(), 17);
        assert_eq!(
            fs::read(state.pet_path.as_ref().unwrap()).unwrap(),
            SPRITESHEET
        );
        fs::remove_file(state.grooming_path.as_ref().unwrap()).unwrap();
        install_into_state(root.path(), &mut state).unwrap();
        assert_eq!(state.pet_path, original);
        assert_eq!(
            fs::read(state.grooming_path.as_ref().unwrap()).unwrap(),
            GROOMING
        );
    }

    #[test]
    fn failed_asset_install_does_not_replace_active_pet_state() {
        let root = tempfile::tempdir().unwrap();
        let before = types::update_desktop_pet_state(root.path(), |state| {
            state.display_name = Some("Keep me".into());
            Ok(())
        })
        .unwrap();
        fs::create_dir_all(
            types::desktop_pet_root(root.path()).join("builtin-naitang-v2/grooming.webp"),
        )
        .unwrap();
        assert!(types::update_desktop_pet_state(root.path(), |state| {
            install_into_state(root.path(), state)
        })
        .is_err());
        let after = types::read_desktop_pet_state(root.path()).unwrap();
        assert_eq!(after.revision, before.revision);
        assert_eq!(after.display_name, before.display_name);
        assert_eq!(after.pet_path, before.pet_path);
    }

    #[test]
    fn builtin_upgrade_preserves_user_visibility_scale_and_pause() {
        let root = tempfile::tempdir().unwrap();
        let old = types::desktop_pet_root(root.path()).join("builtin-naitang-v1/spritesheet.webp");
        fs::create_dir_all(old.parent().unwrap()).unwrap();
        fs::write(&old, SPRITESHEET).unwrap();
        types::update_desktop_pet_state(root.path(), |state| {
            state.pet_path = Some(old.to_string_lossy().into_owned());
            state.scale = 0.3;
            state.enabled = false;
            state.animation_paused = true;
            state.display_name = Some("My kitten".into());
            state.source_path = Some("pending-photo.png".into());
            Ok(())
        })
        .unwrap();
        upgrade_at(root.path()).unwrap();
        let state = types::read_desktop_pet_state(root.path()).unwrap();
        assert_eq!(state.scale, 0.3);
        assert!(!state.enabled);
        assert!(state.animation_paused);
        assert_eq!(state.display_name.as_deref(), Some("My kitten"));
        assert_eq!(state.source_path.as_deref(), Some("pending-photo.png"));
        assert_eq!(state.motion_clips.len(), 2);
        upgrade_at(root.path()).unwrap();
        assert_eq!(
            types::read_desktop_pet_state(root.path()).unwrap().revision,
            state.revision
        );
    }
}
