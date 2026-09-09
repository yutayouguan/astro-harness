//! Offline, versioned companion assets shared by onboarding and Settings.
use std::{fs, io::Write, path::Path};

pub(super) const SPRITESHEET: &[u8] =
    include_bytes!("../../../../src/assets/pets/naitang/spritesheet.webp");
pub(super) const GROOMING: &[u8] =
    include_bytes!("../../../../src/assets/pets/naitang/grooming.webp");

/// Caller holds the shared pet-state transaction. Publish state only after both
/// assets are durable; a failed write never switches the active companion.
pub(super) fn install_into_state(
    base: &Path,
    state: &mut types::DesktopPetState,
) -> anyhow::Result<()> {
    let directory = types::desktop_pet_root(base).join("builtin-naitang-v1");
    fs::create_dir_all(&directory)?;
    for (name, bytes) in [
        ("spritesheet.webp", SPRITESHEET),
        ("grooming.webp", GROOMING),
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
    state.display_name = Some("奶糖".into());
    state.description = Some("内置橘白猫：眨眼、踩奶、舔脚脚与16个注视方向，无需模型生成".into());
    state.source_path = None;
    state.provider = None;
    state.model = None;
    state.follow_wallpaper = false;
    state.animation_paused = false;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_installs_both_assets_and_repairs_missing_files() {
        let root = tempfile::tempdir().unwrap();
        let mut state = types::DesktopPetState::default();
        state.scale = 0.8;
        install_into_state(root.path(), &mut state).unwrap();
        let original = state.pet_path.clone();
        assert_eq!(state.sprite_version_number, Some(2));
        assert_eq!(state.scale, 0.8);
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
            types::desktop_pet_root(root.path()).join("builtin-naitang-v1/grooming.webp"),
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
}
