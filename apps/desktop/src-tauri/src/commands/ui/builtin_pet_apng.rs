//! All-or-nothing publication of reviewed builtin APNG packages.
use super::pet_placement::Screen;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};
use types::pet_scene::PetIdentity;

struct Package {
    id: &'static str,
    key: &'static str,
    directory: &'static str,
    manifest: &'static str,
    spec: &'static str,
    files: &'static [(&'static str, &'static [u8])],
}
const NAITANG: Package = Package {
    id: "builtin-naitang",
    key: "naitang",
    directory: "builtin-naitang-apng-natural-v1",
    manifest: include_str!("../../../../src/assets/pets/naitang/apng/pet.json"),
    spec: include_str!("../../../../src/assets/pets/naitang/apng/motion-clips.json"),
    files: &[
        (
            "idle.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/idle.apng"),
        ),
        (
            "running-right.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/running-right.apng"),
        ),
        (
            "running-left.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/running-left.apng"),
        ),
        (
            "waving.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/waving.apng"),
        ),
        (
            "jumping.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/jumping.apng"),
        ),
        (
            "failed.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/failed.apng"),
        ),
        (
            "waiting.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/waiting.apng"),
        ),
        (
            "running.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/running.apng"),
        ),
        (
            "review.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/review.apng"),
        ),
        (
            "look.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/look.apng"),
        ),
        (
            "kneading.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/kneading.apng"),
        ),
        (
            "grooming.apng",
            include_bytes!("../../../../src/assets/pets/naitang/apng/grooming.apng"),
        ),
    ],
};
const PUDDING: Package = Package {
    id: "builtin-pudding",
    key: "pudding",
    directory: "builtin-pudding-apng-natural-v1",
    manifest: include_str!("../../../../src/assets/pets/pudding/apng/pet.json"),
    spec: include_str!("../../../../src/assets/pets/pudding/apng/motion-clips.json"),
    files: &[
        (
            "idle.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/idle.apng"),
        ),
        (
            "running-right.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/running-right.apng"),
        ),
        (
            "running-left.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/running-left.apng"),
        ),
        (
            "waving.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/waving.apng"),
        ),
        (
            "jumping.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/jumping.apng"),
        ),
        (
            "failed.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/failed.apng"),
        ),
        (
            "waiting.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/waiting.apng"),
        ),
        (
            "running.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/running.apng"),
        ),
        (
            "review.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/review.apng"),
        ),
        (
            "look.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/look.apng"),
        ),
        (
            "tail-wag.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/tail-wag.apng"),
        ),
        (
            "head-tilt.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/head-tilt.apng"),
        ),
        (
            "stretch.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/stretch.apng"),
        ),
        (
            "nap.apng",
            include_bytes!("../../../../src/assets/pets/pudding/apng/nap.apng"),
        ),
    ],
};

const PACKAGES: &[Package] = &[NAITANG, PUDDING];
const RELEASE: &str = include_str!("../../../../src/assets/pets/apng-release.json");
#[derive(serde::Deserialize)]
struct Release {
    approved: bool,
    visual_review: bool,
    native_review: bool,
    hashes: BTreeMap<String, String>,
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn validate_release(raw: &str) -> anyhow::Result<()> {
    let release: Release = serde_json::from_str(raw)?;
    anyhow::ensure!(
        release.approved && release.visual_review && release.native_review,
        "APNG release not approved"
    );
    let mut expected = BTreeMap::new();
    for package in PACKAGES {
        expected.insert(
            format!("{}/pet.json", package.key),
            digest(package.manifest.as_bytes()),
        );
        expected.insert(
            format!("{}/motion-clips.json", package.key),
            digest(package.spec.as_bytes()),
        );
        let manifest: types::DesktopPetManifest = serde_json::from_str(package.manifest)?;
        let spec: types::pet_motion::PetMotionClips = serde_json::from_str(package.spec)?;
        anyhow::ensure!(
            manifest.motion_clips == spec && manifest.sprite_version_number == 3,
            "APNG manifest mismatch"
        );
        super::pet_apng::validate_manifest(&manifest).map_err(anyhow::Error::msg)?;
        for name in ["running-left", "running-right"] {
            let clip = spec
                .get(name)
                .ok_or_else(|| anyhow::anyhow!("APNG walk missing"))?;
            super::pet_roaming_plan::validate_walk(clip).map_err(anyhow::Error::msg)?;
        }
        for (name, bytes) in package.files {
            expected.insert(format!("{}/{}", package.key, name), digest(bytes));
        }
    }
    anyhow::ensure!(
        release.hashes == expected,
        "APNG release hashes do not match reviewed resources"
    );
    Ok(())
}
pub(super) fn released() -> bool {
    static READY: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *READY.get_or_init(|| validate_release(RELEASE).is_ok())
}
pub(super) fn published_identity(base: &Path, id: &str) -> anyhow::Result<Option<PetIdentity>> {
    if !released() {
        return Ok(None);
    }
    let package = PACKAGES
        .iter()
        .find(|package| package.id == id)
        .ok_or_else(|| anyhow::anyhow!("Unknown builtin pet"))?;
    install(base, package).map(Some)
}

fn install(base: &Path, package: &Package) -> anyhow::Result<PetIdentity> {
    let manifest: types::DesktopPetManifest = serde_json::from_str(package.manifest)?;
    super::pet_apng::validate_manifest(&manifest).map_err(anyhow::Error::msg)?;
    types::pet_motion::validate_motion_clips(&manifest.motion_clips)?;
    for clip in manifest.motion_clips.values() {
        let bytes = package
            .files
            .iter()
            .find(|(name, _)| *name == clip.path)
            .ok_or_else(|| anyhow::anyhow!("APNG action missing"))?
            .1;
        super::pet_apng::validate(bytes, clip).map_err(anyhow::Error::msg)?;
    }
    let directory = types::desktop_pet_root(base).join(package.directory);
    fs::create_dir_all(&directory)?;
    let root = home::ui_dir(base).canonicalize()?;
    anyhow::ensure!(
        directory.canonicalize()?.starts_with(root),
        "APNG directory escapes managed UI root"
    );
    for (name, bytes) in package.files {
        let destination = directory.join(name);
        if destination.exists() {
            let existing = types::pet_scene::managed_file(base, &destination)?;
            anyhow::ensure!(
                types::desktop_pet::read_limited_pet_file(&existing, 16 * 1024 * 1024)?.as_slice()
                    == *bytes,
                "APNG destination has custom content; not overwritten"
            );
        } else {
            types::pet_scene::atomic_write(&destination, bytes)?;
        }
    }
    let mut clips = manifest.motion_clips;
    for clip in clips.values_mut() {
        clip.path = directory.join(&clip.path).to_string_lossy().into_owned();
    }
    Ok(PetIdentity {
        pet_id: package.id.into(),
        pet_path: clips["idle"].path.clone(),
        grooming_path: None,
        motion_clips: clips,
        sprite_version_number: Some(3),
        source_path: None,
        display_name: Some(manifest.display_name),
        description: Some(manifest.description),
        provider: None,
        model: None,
    })
}
fn visuals(target: &mut PetIdentity, source: &PetIdentity) {
    target.pet_path = source.pet_path.clone();
    target.grooming_path = None;
    target.motion_clips = source.motion_clips.clone();
    target.sprite_version_number = Some(3);
}
fn remap_position(
    position: &mut Option<types::pet_preferences::PetPosition>,
    scale: f64,
    screens: &[Screen],
) {
    let Some(old) = position.as_ref() else {
        return;
    };
    if !screens.iter().any(|screen| screen.name == old.monitor) {
        return;
    }
    let old_size = super::desktop_pet::window_size(scale);
    let new_size = (old_size.width * 256.0 / 192.0, old_size.height);
    let Some(point) =
        super::pet_placement::target(Some(old), screens, (old_size.width, old_size.height))
    else {
        return;
    };
    let screen = screens
        .iter()
        .filter(|screen| screen.name == old.monitor)
        .min_by_key(|screen| {
            i64::from(screen.x).abs_diff(i64::from(old.monitor_x))
                + i64::from(screen.y).abs_diff(i64::from(old.monitor_y))
        });
    let Some(screen) = screen else {
        return;
    };
    let x = f64::from(point.0) - (new_size.0 - old_size.width) * screen.scale / 2.0;
    *position = super::pet_placement::capture(
        (x.round() as i32, point.1),
        (
            (new_size.0 * screen.scale).round() as u32,
            (new_size.1 * screen.scale).round() as u32,
        ),
        screens,
        false,
    );
}
fn upgrade_unchecked(base: &Path, screens: &[Screen]) -> anyhow::Result<()> {
    let current = types::read_desktop_pet_state(base)?;
    let old_ids: Vec<_> = PACKAGES
        .iter()
        .filter(|package| {
            current
                .pets
                .iter()
                .any(|pet| pet.id == package.id && pet.identity.sprite_version_number != Some(3))
        })
        .map(|p| p.id)
        .collect();
    if old_ids.is_empty() {
        return Ok(());
    }
    types::update_desktop_pet_state(base, |state| {
        // Re-evaluate every old identity under the one shared writer transaction.
        for package in PACKAGES.iter().filter(|p| old_ids.contains(&p.id)) {
            let Some(pet) = state.pets.iter().find(|pet| pet.id == package.id) else {
                anyhow::bail!("Builtin changed during upgrade");
            };
            if pet.identity.sprite_version_number == Some(3) {
                continue;
            }
            anyhow::ensure!(
                pet.builtin
                    && super::builtin_pet::is_original_identity(base, &pet.identity, package.id),
                "Builtin contains customized art; migration stopped"
            );
            for scene in state
                .scenes
                .iter()
                .filter(|scene| scene.pet.pet_id == package.id)
            {
                anyhow::ensure!(
                    super::builtin_pet::is_original_identity(base, &scene.pet, package.id),
                    "Scene contains customized pet art; migration stopped"
                );
            }
        }
        let mut installed = Vec::new();
        for package in PACKAGES.iter().filter(|p| old_ids.contains(&p.id)) {
            if state
                .pets
                .iter()
                .any(|pet| pet.id == package.id && pet.identity.sprite_version_number != Some(3))
            {
                installed.push((package.id, install(base, package)?));
            }
        }
        if installed.is_empty() {
            return Ok(());
        }
        let backup = types::desktop_pet_root(base).join(format!(
            "state.before-natural-apng-r{}.json",
            state.revision
        ));
        let bytes = types::desktop_pet::read_limited_pet_file(
            &types::desktop_pet_state_path(base),
            1024 * 1024,
        )?;
        if backup.exists() {
            let existing = types::pet_scene::managed_file(base, &backup)?;
            anyhow::ensure!(
                types::desktop_pet::read_limited_pet_file(&existing, 1024 * 1024)? == bytes,
                "APNG backup already contains different state; migration stopped"
            );
        } else {
            types::pet_scene::atomic_write(&backup, &bytes)?;
        }
        for (id, identity) in installed {
            if state.active_pet_id.as_deref() == Some(id) {
                anyhow::ensure!(
                    super::builtin_pet::is_original_identity(
                        base,
                        &PetIdentity::from_state(state)?,
                        id
                    ),
                    "Current pet has customized art"
                );
                state.pet_path = Some(identity.pet_path.clone());
                state.grooming_path = None;
                state.motion_clips = identity.motion_clips.clone();
                state.sprite_version_number = Some(3);
                remap_position(&mut state.preferences.position, state.scale, screens);
            }
            let pet = state.pets.iter_mut().find(|pet| pet.id == id).unwrap();
            visuals(&mut pet.identity, &identity);
            remap_position(
                &mut pet.defaults.behavior.position,
                pet.defaults.scale,
                screens,
            );
            for scene in state
                .scenes
                .iter_mut()
                .filter(|scene| scene.pet.pet_id == id)
            {
                visuals(&mut scene.pet, &identity);
                if let Some(preferences) = &mut scene.preferences {
                    remap_position(
                        &mut preferences.behavior.position,
                        preferences.scale,
                        screens,
                    );
                }
            }
        }
        Ok(())
    })?;
    Ok(())
}
pub(super) fn upgrade(base: &Path, screens: &[Screen]) -> anyhow::Result<()> {
    if !released() {
        return Ok(());
    }
    upgrade_unchecked(base, screens)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unreviewed_release_cannot_publish() {
        assert!(validate_release(
            r#"{"approved":false,"visual_review":false,"native_review":false,"hashes":{}}"#
        )
        .is_err());
        assert!(validate_release(
            r#"{"approved":true,"visual_review":true,"native_review":true,"hashes":{}}"#
        )
        .is_err());
    }
    #[test]
    fn upgrade_preserves_preferences_identity_and_all_scene_bindings() {
        let root = tempfile::tempdir().unwrap();
        super::super::builtin_pet::ensure_library(root.path()).unwrap();
        types::pet_library::apply_pet(root.path(), "builtin-naitang").unwrap();
        types::pet_library::edit_library(
            root.path(),
            types::pet_library::PetLibraryEdit::AddScene {
                pet_id: "builtin-naitang".into(),
                name: "Home".into(),
            },
        )
        .unwrap();
        let before = types::read_desktop_pet_state(root.path()).unwrap();
        upgrade_unchecked(root.path(), &[]).unwrap();
        let after = types::read_desktop_pet_state(root.path()).unwrap();
        assert_eq!(after.sprite_version_number, Some(3));
        assert_eq!(after.preferences, before.preferences);
        assert_eq!(after.scale, before.scale);
        assert_eq!(after.active_pet_id, before.active_pet_id);
        assert_eq!(after.active_scene_id, before.active_scene_id);
        assert_eq!(after.scenes[0].id, before.scenes[0].id);
        assert!(after.scenes[0].pet.pet_path.ends_with(".apng"));
        assert!(after
            .pets
            .iter()
            .all(|pet| pet.identity.sprite_version_number == Some(3)));
        upgrade_unchecked(root.path(), &[]).unwrap();
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), after);
    }
    #[test]
    fn customized_builtin_prevents_partial_batch_upgrade() {
        let root = tempfile::tempdir().unwrap();
        super::super::builtin_pet::ensure_library(root.path()).unwrap();
        let before = types::read_desktop_pet_state(root.path()).unwrap();
        fs::write(&before.pets[1].identity.pet_path, b"customized").unwrap();
        assert!(upgrade_unchecked(root.path(), &[]).is_err());
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), before);
    }
    #[test]
    fn conflicting_backup_does_not_publish_or_overwrite() {
        let root = tempfile::tempdir().unwrap();
        super::super::builtin_pet::ensure_library(root.path()).unwrap();
        let before = types::read_desktop_pet_state(root.path()).unwrap();
        let backup = types::desktop_pet_root(root.path()).join(format!(
            "state.before-natural-apng-r{}.json",
            before.revision
        ));
        fs::write(&backup, b"earlier recovery state").unwrap();
        assert!(upgrade_unchecked(root.path(), &[]).is_err());
        assert_eq!(fs::read(backup).unwrap(), b"earlier recovery state");
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), before);
    }
}
