//! Offline, versioned companion assets shared by onboarding and Settings.
use sha2::{Digest, Sha256};
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

/// A catalog entry describes only shipped, validated assets, never placeholders.
struct BuiltinPetSpec {
    id: &'static str,
    directory: &'static str,
    legacy_directory: Option<&'static str>,
    name: &'static str,
    description: &'static str,
    atlas: &'static [u8],
    grooming: Option<&'static [u8]>,
    motion_spec: &'static str,
    assets: &'static [(&'static str, &'static [u8])],
}

const NAITANG: BuiltinPetSpec = BuiltinPetSpec {
    id: "builtin-naitang",
    directory: "builtin-naitang-v2",
    legacy_directory: Some("builtin-naitang-v1"),
    name: "奶糖",
    description: "内置橘白猫：眨眼、踩奶、舔脚脚与16个注视方向，无需模型生成",
    atlas: SPRITESHEET,
    grooming: Some(GROOMING),
    motion_spec: MOTION_SPEC,
    assets: &[
        ("spritesheet.webp", SPRITESHEET),
        ("grooming.webp", GROOMING),
        ("kneading-motion.webp", KNEADING_MOTION),
        ("grooming-motion.webp", GROOMING_MOTION),
    ],
};
const PUDDING_ATLAS: &[u8] = include_bytes!("../../../../src/assets/pets/pudding/spritesheet.webp");
const PUDDING_TAIL: &[u8] = include_bytes!("../../../../src/assets/pets/pudding/tail-wag.webp");
const PUDDING_HEAD: &[u8] = include_bytes!("../../../../src/assets/pets/pudding/head-tilt.webp");
const PUDDING_STRETCH: &[u8] = include_bytes!("../../../../src/assets/pets/pudding/stretch.webp");
const PUDDING_NAP: &[u8] = include_bytes!("../../../../src/assets/pets/pudding/nap.webp");
const PUDDING: BuiltinPetSpec = BuiltinPetSpec {
    id: "builtin-pudding",
    directory: "builtin-pudding-v1",
    legacy_directory: None,
    name: "布丁",
    description: "奶油色垂耳小狗：摇尾巴、歪头、伸懒腰与趴下打盹，温顺安静，无需模型生成",
    atlas: PUDDING_ATLAS,
    grooming: None,
    motion_spec: include_str!("../../../../src/assets/pets/pudding/motion-clips.json"),
    assets: &[
        ("spritesheet.webp", PUDDING_ATLAS),
        ("tail-wag-v2.webp", PUDDING_TAIL),
        ("head-tilt.webp", PUDDING_HEAD),
        ("stretch.webp", PUDDING_STRETCH),
        ("nap.webp", PUDDING_NAP),
    ],
};
const BUILTIN_PETS: &[BuiltinPetSpec] = &[NAITANG, PUDDING];

// Installed tail-v2 before its two alpha=1 edge pixels were trimmed. Verified
// against the shipped raster: only (45,1020) and (46,1020) differ, alpha 1→0.
// Recognize this exact file only, never use a fuzzy pixel/hash comparison.
const PUDDING_TAIL_PRE_TRIM_SHA256: &str =
    "7fefaa6520264a803d7c0b0b11a1a46252d17a82ffa8b63466da194260db9467";

/// Locate candidate legacy artwork without assuming a canonical library ID.
/// Bytes and clip metadata must still pass `is_original_identity` before use.
pub(super) fn package_at_original_path(base: &Path, path: &str) -> Option<&'static str> {
    let root = types::desktop_pet_root(base);
    BUILTIN_PETS
        .iter()
        .find(|spec| {
            std::iter::once(spec.directory)
                .chain(spec.legacy_directory)
                .any(|directory| Path::new(path) == root.join(directory).join("spritesheet.webp"))
        })
        .map(|spec| spec.id)
}

pub(super) fn is_original_identity(
    base: &Path,
    identity: &types::pet_scene::PetIdentity,
    id: &str,
) -> bool {
    let Some(spec) = BUILTIN_PETS.iter().find(|spec| spec.id == id) else {
        return false;
    };
    let root = types::desktop_pet_root(base);
    let directories = std::iter::once(spec.directory).chain(spec.legacy_directory);
    let same_file = |path: &Path, expected: &[u8]| {
        types::pet_scene::managed_file(base, path)
            .and_then(|path| types::desktop_pet::read_limited_pet_file(&path, 50 * 1024 * 1024))
            .is_ok_and(|bytes| {
                bytes == expected
                    || (spec.id == PUDDING.id
                        && expected == PUDDING_TAIL
                        && path
                            .file_name()
                            .is_some_and(|name| name == "tail-wag-v2.webp")
                        && format!("{:x}", Sha256::digest(&bytes)) == PUDDING_TAIL_PRE_TRIM_SHA256)
            })
    };
    for name in directories {
        let directory = root.join(name);
        if Path::new(&identity.pet_path) != directory.join("spritesheet.webp")
            || !same_file(Path::new(&identity.pet_path), spec.atlas)
        {
            continue;
        }
        if identity.sprite_version_number != Some(2) {
            return false;
        }
        match (identity.grooming_path.as_deref(), spec.grooming) {
            (Some(path), Some(bytes))
                if Path::new(path) == directory.join("grooming.webp")
                    && same_file(Path::new(path), bytes) => {}
            (None, None) => {}
            (None, Some(_)) if Some(name) == spec.legacy_directory => {}
            _ => return false,
        }
        if Some(name) == spec.legacy_directory && identity.motion_clips.is_empty() {
            return true;
        }
        let Ok(mut expected) =
            serde_json::from_str::<types::pet_motion::PetMotionClips>(spec.motion_spec)
        else {
            return false;
        };
        for clip in expected.values_mut() {
            clip.path = directory.join(&clip.path).to_string_lossy().into_owned();
        }
        let mut before_bookends = expected.clone();
        for clip in before_bookends.values_mut() {
            clip.neutral_bookends = false;
        }
        if identity.motion_clips != expected && identity.motion_clips != before_bookends {
            return false;
        }
        return identity.motion_clips.values().all(|clip| {
            let filename = Path::new(&clip.path)
                .file_name()
                .and_then(|name| name.to_str());
            spec.assets
                .iter()
                .find(|(name, _)| Some(*name) == filename)
                .is_some_and(|(_, bytes)| same_file(Path::new(&clip.path), bytes))
        });
    }
    false
}

fn existing_builtin<'a>(
    base: &Path,
    state: &'a types::DesktopPetState,
    spec: &BuiltinPetSpec,
) -> Option<&'a types::pet_library::PetRecord> {
    let root = types::desktop_pet_root(base);
    let current = root.join(spec.directory).join("spritesheet.webp");
    let legacy = spec
        .legacy_directory
        .map(|name| root.join(name).join("spritesheet.webp"));
    state.pets.iter().find(|p| {
        p.id == spec.id
            || Some(p.identity.pet_path.as_str()) == current.to_str()
            || legacy
                .as_ref()
                .is_some_and(|old| Some(p.identity.pet_path.as_str()) == old.to_str())
    })
}

/// Caller holds the shared pet transaction. Validate the complete package before
/// publishing any library entry. File failure cannot switch the active companion.
fn install_record(
    base: &Path,
    spec: &BuiltinPetSpec,
    id: String,
) -> anyhow::Result<types::pet_library::PetRecord> {
    super::desktop_pet::validate_v2_atlas(spec.atlas).map_err(anyhow::Error::msg)?;
    if let Some(grooming) = spec.grooming {
        super::desktop_pet::validate_grooming_strip(grooming).map_err(anyhow::Error::msg)?;
    }
    let mut motion_clips: types::pet_motion::PetMotionClips =
        serde_json::from_str(spec.motion_spec)?;
    types::pet_motion::validate_motion_clips(&motion_clips)?;
    for clip in motion_clips.values() {
        let bytes = spec
            .assets
            .iter()
            .find(|(name, _)| *name == clip.path)
            .map(|(_, bytes)| *bytes)
            .ok_or_else(|| anyhow::anyhow!("内置动作缺少素材：{}", clip.path))?;
        super::desktop_pet::validate_motion_image(bytes, clip).map_err(anyhow::Error::msg)?;
    }
    let directory = types::desktop_pet_root(base).join(spec.directory);
    fs::create_dir_all(&directory)?;
    for (name, bytes) in spec.assets {
        let path = directory.join(name);
        if fs::read(&path).is_ok_and(|existing| existing == *bytes) {
            continue;
        }
        let mut file = tempfile::NamedTempFile::new_in(&directory)?;
        file.write_all(bytes)?;
        file.as_file().sync_all()?;
        file.persist(&path)?;
    }
    for clip in motion_clips.values_mut() {
        clip.path = directory.join(&clip.path).to_string_lossy().into_owned();
    }
    Ok(types::pet_library::PetRecord {
        id: id.clone(),
        builtin: true,
        defaults: types::pet_preferences::PetScenePreferences::from_state(
            &types::DesktopPetState::default(),
        ),
        identity: types::pet_scene::PetIdentity {
            pet_id: id,
            pet_path: directory
                .join("spritesheet.webp")
                .to_string_lossy()
                .into_owned(),
            grooming_path: spec.grooming.map(|_| {
                directory
                    .join("grooming.webp")
                    .to_string_lossy()
                    .into_owned()
            }),
            motion_clips,
            sprite_version_number: Some(types::DESKTOP_PET_V2_SPRITE_VERSION),
            display_name: Some(spec.name.into()),
            description: Some(spec.description.into()),
            source_path: None,
            provider: None,
            model: None,
        },
    })
}

/// Existing onboarding/default command still explicitly chooses Naitang.
pub(super) fn install_into_state(
    base: &Path,
    state: &mut types::DesktopPetState,
) -> anyhow::Result<()> {
    install_into_state_with_format(base, state, true)
}
fn install_into_state_with_format(
    base: &Path,
    state: &mut types::DesktopPetState,
    use_apng: bool,
) -> anyhow::Result<()> {
    let previous = existing_builtin(base, state, &NAITANG).cloned();
    let id = previous
        .as_ref()
        .map(|p| p.id.clone())
        .unwrap_or_else(|| NAITANG.id.into());
    let mut record = install_record(base, &NAITANG, id.clone())?;
    if use_apng {
        if let Some(mut identity) = super::builtin_pet_apng::published_identity(base, NAITANG.id)? {
            identity.pet_id = id.clone();
            record.identity = identity;
        }
    }
    if let Some(previous) = previous {
        record.defaults = previous.defaults;
    }
    let identity = &record.identity;
    state.active_pet_id = Some(id.clone());
    state.active_scene_id = None;
    state.pet_path = Some(identity.pet_path.clone());
    state.grooming_path = identity.grooming_path.clone();
    state.motion_clips = identity.motion_clips.clone();
    state.sprite_version_number = identity.sprite_version_number;
    state.display_name = identity.display_name.clone();
    state.description = identity.description.clone();
    state.source_path = None;
    state.provider = None;
    state.model = None;
    if let Some(existing) = state.pets.iter_mut().find(|p| p.id == id) {
        *existing = record;
    } else {
        state.pets.push(record);
    }
    state.follow_wallpaper = false;
    state.animation_paused = false;
    Ok(())
}

fn ensure_catalog(base: &Path, catalog: &[BuiltinPetSpec]) -> anyhow::Result<()> {
    let needs_update = |state: &types::DesktopPetState| {
        catalog
            .iter()
            .any(|spec| existing_builtin(base, state, spec).is_none_or(|p| !p.builtin))
    };
    if !needs_update(&types::read_desktop_pet_state(base)?) {
        return Ok(());
    }
    types::update_desktop_pet_state(base, |state| {
        // Check each identity again inside the transaction, never "any builtin".
        for spec in catalog {
            if let Some(existing) = existing_builtin(base, state, spec) {
                let id = existing.id.clone();
                state.pets.iter_mut().find(|p| p.id == id).unwrap().builtin = true;
            } else {
                state.pets.push(install_record(base, spec, spec.id.into())?);
            }
        }
        Ok(())
    })?;
    Ok(())
}

pub(super) fn ensure_library(base: &Path) -> anyhow::Result<()> {
    ensure_catalog(base, BUILTIN_PETS)
}

const OLD_PUDDING_TAIL_SHA256: &str =
    "a8fa62bc14c2b09cf31e6bc3c4aa5f4584d6ab3010203f0ac410b43ffdc70155";

fn old_pudding_tail(base: &Path) -> types::pet_motion::PetMotionClip {
    let mut durations_ms = vec![90; 39];
    durations_ms[0] = 180;
    durations_ms[38] = 220;
    types::pet_motion::PetMotionClip {
        path: types::desktop_pet_root(base)
            .join("builtin-pudding-v1/tail-wag.webp")
            .to_string_lossy()
            .into_owned(),
        frame_width: 192,
        frame_height: 208,
        columns: 4,
        durations_ms,
        loop_start: 4,
        loop_end: 34,
        loop_repeats: 1,
        neutral_bookends: true,
        locomotion: None,
    }
}

/// Only upgrade the recognized shipped tail and timings. Keep the old file for
/// recovery and change the URL so decoded WebView image caches cannot reuse it.
fn upgrade_pudding_tail_with_hash(base: &Path, legacy_hash: &str) -> anyhow::Result<()> {
    let legacy = old_pudding_tail(base);
    let main = types::desktop_pet_root(base).join("builtin-pudding-v1/spritesheet.webp");
    let eligible = |state: &types::DesktopPetState| {
        state.pets.iter().any(|pet| {
            pet.id == "builtin-pudding"
                && pet.builtin
                && Some(pet.identity.pet_path.as_str()) == main.to_str()
                && pet.identity.motion_clips.get("tail-wag") == Some(&legacy)
        })
    };
    let original_files = || -> anyhow::Result<bool> {
        let old_path = types::pet_scene::managed_file(base, Path::new(&legacy.path))?;
        let bytes = types::desktop_pet::read_limited_pet_file(&old_path, 16 * 1024 * 1024)?;
        let original_main = types::pet_scene::managed_file(base, &main)
            .and_then(|path| types::desktop_pet::read_limited_pet_file(&path, 50 * 1024 * 1024))
            .ok();
        Ok(format!("{:x}", Sha256::digest(&bytes)) == legacy_hash
            && original_main.as_deref() == Some(PUDDING_ATLAS))
    };
    if !eligible(&types::read_desktop_pet_state(base)?) || !original_files()? {
        return Ok(());
    }
    types::update_desktop_pet_state(base, |state| {
        if !eligible(state) || !original_files()? {
            return Ok(());
        }
        let clips: types::pet_motion::PetMotionClips = serde_json::from_str(PUDDING.motion_spec)?;
        let mut updated = clips["tail-wag"].clone();
        super::desktop_pet::validate_motion_image(PUDDING_TAIL, &updated)
            .map_err(anyhow::Error::msg)?;
        let destination = types::desktop_pet_root(base)
            .join(PUDDING.directory)
            .join(&updated.path);
        if destination.exists()
            && types::desktop_pet::read_limited_pet_file(&destination, 16 * 1024 * 1024)?.as_slice()
                != PUDDING_TAIL
        {
            anyhow::bail!("新的布丁尾巴路径已有自定义素材，未覆盖");
        }
        types::pet_scene::atomic_write(&destination, PUDDING_TAIL)?;
        updated.path = destination.to_string_lossy().into_owned();
        state
            .pets
            .iter_mut()
            .find(|p| p.id == "builtin-pudding")
            .unwrap()
            .identity
            .motion_clips
            .insert("tail-wag".into(), updated.clone());
        if state.active_pet_id.as_deref() == Some("builtin-pudding")
            && state.motion_clips.get("tail-wag") == Some(&legacy)
        {
            state
                .motion_clips
                .insert("tail-wag".into(), updated.clone());
        }
        for scene in &mut state.scenes {
            if scene.pet.pet_id == "builtin-pudding"
                && scene.pet.motion_clips.get("tail-wag") == Some(&legacy)
            {
                scene
                    .pet
                    .motion_clips
                    .insert("tail-wag".into(), updated.clone());
            }
        }
        Ok(())
    })?;
    Ok(())
}

fn can_upgrade(base: &Path, state: &types::DesktopPetState) -> bool {
    let root = types::desktop_pet_root(base);
    let v1 = root.join("builtin-naitang-v1/spritesheet.webp");
    if state.pet_path.as_deref() == v1.to_str() {
        return state.motion_clips.is_empty() && fs::read(&v1).ok().as_deref() == Some(SPRITESHEET);
    }
    let v2 = root.join("builtin-naitang-v2");
    if state.pet_path.as_deref() != v2.join("spritesheet.webp").to_str() {
        return false;
    }
    let Ok(mut legacy) = serde_json::from_str::<types::pet_motion::PetMotionClips>(MOTION_SPEC)
    else {
        return false;
    };
    for clip in legacy.values_mut() {
        clip.path = v2.join(&clip.path).to_string_lossy().into_owned();
        clip.neutral_bookends = false;
    }
    state.motion_clips == legacy
        && fs::read(v2.join("spritesheet.webp")).ok().as_deref() == Some(SPRITESHEET)
        && fs::read(v2.join("kneading-motion.webp")).ok().as_deref() == Some(KNEADING_MOTION)
        && fs::read(v2.join("grooming-motion.webp")).ok().as_deref() == Some(GROOMING_MOTION)
}

/// Upgrade only untouched built-in art and timing; preserve all user preferences.
pub(super) fn upgrade_at(base: &Path) -> anyhow::Result<()> {
    upgrade_pudding_tail_with_hash(base, OLD_PUDDING_TAIL_SHA256)?;
    let current = types::read_desktop_pet_state(base)?;
    if !can_upgrade(base, &current) {
        return Ok(());
    }
    types::update_desktop_pet_state(base, |state| {
        if !can_upgrade(base, state) {
            return Ok(());
        }
        let previous = state.clone();
        install_into_state_with_format(base, state, false)?;
        state.animation_paused = previous.animation_paused;
        state.follow_wallpaper = previous.follow_wallpaper;
        state.display_name = previous.display_name;
        state.description = previous.description;
        state.source_path = previous.source_path;
        state.provider = previous.provider;
        state.model = previous.model;
        let identity = types::pet_scene::PetIdentity::from_state(state)?;
        if let Some(record) = state.pets.iter_mut().find(|p| p.id == identity.pet_id) {
            record.identity = identity;
        }
        for scene in &mut state.scenes {
            if Some(&scene.pet.pet_path) == previous.pet_path.as_ref()
                && (scene.pet.motion_clips.is_empty()
                    || scene.pet.motion_clips == previous.motion_clips)
            {
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

    fn legacy_pudding_fixture() -> (tempfile::TempDir, types::DesktopPetState, String) {
        let root = tempfile::tempdir().unwrap();
        ensure_library(root.path()).unwrap();
        let legacy = old_pudding_tail(root.path());
        // Only the expected hash is injected for this synthetic migration fixture.
        // Production uses the fixed fingerprint of the previously shipped asset.
        let bytes = b"previous shipped tail fixture";
        fs::write(&legacy.path, bytes).unwrap();
        let hash = format!("{:x}", Sha256::digest(bytes));
        types::update_desktop_pet_state(root.path(), |state| {
            let pet = state
                .pets
                .iter_mut()
                .find(|p| p.id == "builtin-pudding")
                .unwrap();
            pet.identity
                .motion_clips
                .insert("tail-wag".into(), legacy.clone());
            pet.identity.display_name = Some("My Pudding".into());
            pet.defaults.scale = 0.175;
            Ok(())
        })
        .unwrap();
        types::pet_library::apply_pet(root.path(), "builtin-pudding").unwrap();
        let saved = types::pet_library::edit_library(
            root.path(),
            types::pet_library::PetLibraryEdit::AddScene {
                pet_id: "builtin-pudding".into(),
                name: "My home".into(),
            },
        )
        .unwrap();
        let scene_id = saved.scenes[0].id.clone();
        let state = types::update_desktop_pet_state(root.path(), |state| {
            state.scale = 0.25;
            state.active_scene_id = Some(scene_id);
            state.preferences.presentation_mode = true;
            state.preferences.position_locked = true;
            state.animation_paused = true;
            state.follow_wallpaper = true;
            Ok(())
        })
        .unwrap();
        (root, state, hash)
    }

    #[test]
    fn pudding_tail_upgrade_changes_only_the_recognized_tail_and_is_idempotent() {
        let (root, before, hash) = legacy_pudding_fixture();
        upgrade_pudding_tail_with_hash(root.path(), &hash).unwrap();
        let after = types::read_desktop_pet_state(root.path()).unwrap();
        let tail = after.motion_clips["tail-wag"].clone();
        assert!(tail.path.ends_with("tail-wag-v2.webp"));
        assert_eq!(tail.durations_ms.len(), 49);
        assert_eq!(fs::read(&tail.path).unwrap(), PUDDING_TAIL);
        let mut expected = before.clone();
        expected.revision += 1;
        expected
            .motion_clips
            .insert("tail-wag".into(), tail.clone());
        expected
            .pets
            .iter_mut()
            .find(|p| p.id == "builtin-pudding")
            .unwrap()
            .identity
            .motion_clips
            .insert("tail-wag".into(), tail.clone());
        expected.scenes[0]
            .pet
            .motion_clips
            .insert("tail-wag".into(), tail);
        assert_eq!(after, expected);
        assert!(Path::new(&old_pudding_tail(root.path()).path).is_file());
        upgrade_pudding_tail_with_hash(root.path(), &hash).unwrap();
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), after);
    }

    #[test]
    fn pudding_tail_upgrade_preserves_user_modified_assets_and_timings() {
        let (root, before, hash) = legacy_pudding_fixture();
        fs::write(&old_pudding_tail(root.path()).path, b"user modified tail").unwrap();
        upgrade_pudding_tail_with_hash(root.path(), &hash).unwrap();
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), before);
        let (root, _, hash) = legacy_pudding_fixture();
        let changed = types::update_desktop_pet_state(root.path(), |state| {
            state
                .pets
                .iter_mut()
                .find(|p| p.id == "builtin-pudding")
                .unwrap()
                .identity
                .motion_clips
                .get_mut("tail-wag")
                .unwrap()
                .durations_ms[1] = 100;
            Ok(())
        })
        .unwrap();
        upgrade_pudding_tail_with_hash(root.path(), &hash).unwrap();
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), changed);
        let (root, before, hash) = legacy_pudding_fixture();
        fs::write(
            types::desktop_pet_root(root.path()).join("builtin-pudding-v1/spritesheet.webp"),
            b"user changed body",
        )
        .unwrap();
        upgrade_pudding_tail_with_hash(root.path(), &hash).unwrap();
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), before);
    }

    #[test]
    fn pudding_tail_upgrade_does_not_overwrite_a_conflicting_destination() {
        let (root, before, hash) = legacy_pudding_fixture();
        let target =
            types::desktop_pet_root(root.path()).join("builtin-pudding-v1/tail-wag-v2.webp");
        fs::write(&target, b"user created destination").unwrap();
        assert!(upgrade_pudding_tail_with_hash(root.path(), &hash).is_err());
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), before);
        assert_eq!(fs::read(&target).unwrap(), b"user created destination");
    }

    #[test]
    fn pudding_tail_frames_keep_body_and_paws_still() {
        let atlas = image::load_from_memory(PUDDING_ATLAS).unwrap().to_rgba8();
        let tail = image::load_from_memory(PUDDING_TAIL).unwrap().to_rgba8();
        for i in 0..49 {
            for y in 0..208 {
                for x in 0..192 {
                    if x >= 72 || !(125..188).contains(&y) {
                        assert_eq!(
                            tail.get_pixel((i % 4) * 192 + x, (i / 4) * 208 + y),
                            atlas.get_pixel(x, y),
                            "frame {i}, pixel {x},{y}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn pudding_and_naitang_keep_independent_scenes_and_global_visibility_policy() {
        let root = tempfile::tempdir().unwrap();
        ensure_library(root.path()).unwrap();
        let before = types::read_desktop_pet_state(root.path()).unwrap();
        let dog = before
            .pets
            .iter()
            .find(|p| p.id == "builtin-pudding")
            .unwrap();
        assert_eq!(dog.identity.display_name.as_deref(), Some("布丁"));
        assert!(dog.identity.grooming_path.is_none());
        assert_eq!(
            dog.identity
                .motion_clips
                .keys()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            vec!["head-tilt", "nap", "stretch", "tail-wag"]
        );
        assert_ne!(PUDDING_ATLAS, SPRITESHEET);
        for (id, name) in [
            ("builtin-naitang", "Cat home"),
            ("builtin-pudding", "Dog home"),
        ] {
            types::pet_library::edit_library(
                root.path(),
                types::pet_library::PetLibraryEdit::AddScene {
                    pet_id: id.into(),
                    name: name.into(),
                },
            )
            .unwrap();
        }
        let state = types::update_desktop_pet_state(root.path(), |state| {
            state.preferences.presentation_mode = true;
            Ok(())
        })
        .unwrap();
        let dog_scene = state
            .scenes
            .iter()
            .find(|s| s.pet.pet_id == "builtin-pudding")
            .unwrap();
        let selected = types::pet_scene::apply_scene(
            root.path(),
            &dog_scene.id,
            types::pet_scene::SceneApplyMode::Pet,
        )
        .unwrap();
        assert_eq!(selected.active_pet_id.as_deref(), Some("builtin-pudding"));
        assert_eq!(
            selected.scale,
            types::desktop_pet::DESKTOP_PET_DEFAULT_SCALE
        );
        assert!(selected.preferences.presentation_mode);
        assert_eq!(selected.motion_clips.len(), 4);
        let cat = types::pet_library::apply_pet(root.path(), "builtin-naitang").unwrap();
        assert_eq!(cat.motion_clips.len(), 2);
        assert!(cat.motion_clips.contains_key("kneading"));
        assert!(cat
            .scenes
            .iter()
            .find(|s| s.id == dog_scene.id)
            .unwrap()
            .pet
            .motion_clips
            .contains_key("tail-wag"));
    }

    #[test]
    fn catalog_registers_each_missing_identity_without_switching_the_desktop() {
        let root = tempfile::tempdir().unwrap();
        ensure_library(root.path()).unwrap();
        let before = types::update_desktop_pet_state(root.path(), |state| {
            state.scale = 0.3;
            state.preferences.presentation_mode = true;
            Ok(())
        })
        .unwrap();
        // Test-only catalog entry reuses validated bytes; it is not Pudding art.
        let second = BuiltinPetSpec {
            id: "builtin-test-companion",
            directory: "test-companion-v1",
            legacy_directory: None,
            name: "Catalog fixture",
            ..NAITANG
        };
        let catalog = [NAITANG, second];
        ensure_catalog(root.path(), &catalog).unwrap();
        let state = types::read_desktop_pet_state(root.path()).unwrap();
        assert_eq!(state.pets.len(), 3);
        assert_eq!(state.pet_path, before.pet_path);
        assert_eq!(state.enabled, before.enabled);
        assert_eq!(state.scale, 0.3);
        assert!(state.preferences.presentation_mode);
        let added = state
            .pets
            .iter()
            .find(|p| p.id == "builtin-test-companion")
            .unwrap();
        assert_eq!(
            added.defaults.scale,
            types::desktop_pet::DESKTOP_PET_DEFAULT_SCALE
        );
        assert_eq!(added.defaults.behavior.activity_interval_secs, 45);
        ensure_catalog(root.path(), &catalog).unwrap();
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), state);
    }

    #[test]
    fn incomplete_catalog_assets_never_publish_a_partial_pet() {
        let root = tempfile::tempdir().unwrap();
        ensure_library(root.path()).unwrap();
        let before = types::read_desktop_pet_state(root.path()).unwrap();
        let broken = BuiltinPetSpec {
            id: "builtin-broken",
            directory: "broken-v1",
            legacy_directory: None,
            assets: &[("spritesheet.webp", SPRITESHEET)],
            ..NAITANG
        };
        assert!(ensure_catalog(root.path(), &[NAITANG, broken]).is_err());
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), before);
    }

    #[test]
    fn builtin_library_is_available_without_applying_and_seeds_once() {
        let root = tempfile::tempdir().unwrap();
        ensure_library(root.path()).unwrap();
        let state = types::read_desktop_pet_state(root.path()).unwrap();
        assert!(!state.enabled && state.pet_path.is_none());
        assert_eq!(state.pets.len(), 2);
        assert!(state.pets[0].builtin);
        assert!(Path::new(&state.pets[0].identity.pet_path).is_file());
        ensure_library(root.path()).unwrap();
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), state);
    }

    #[test]
    fn builtin_v2_neutral_upgrade_preserves_preferences_and_rejects_modified_art() {
        let root = tempfile::tempdir().unwrap();
        let mut state = types::DesktopPetState::default();
        install_into_state(root.path(), &mut state).unwrap();
        state.preferences.quiet_mode = true;
        for clip in state.motion_clips.values_mut() {
            clip.neutral_bookends = false;
        }
        types::write_desktop_pet_state(root.path(), &state).unwrap();
        upgrade_at(root.path()).unwrap();
        let upgraded = types::read_desktop_pet_state(root.path()).unwrap();
        assert!(upgraded
            .motion_clips
            .values()
            .all(|clip| clip.neutral_bookends));
        assert!(upgraded.preferences.quiet_mode);
        let path = &upgraded.motion_clips["grooming"].path;
        fs::write(path, b"user edited motion").unwrap();
        let edited = types::update_desktop_pet_state(root.path(), |state| {
            for clip in state.motion_clips.values_mut() {
                clip.neutral_bookends = false;
            }
            Ok(())
        })
        .unwrap();
        upgrade_at(root.path()).unwrap();
        assert_eq!(types::read_desktop_pet_state(root.path()).unwrap(), edited);
    }

    #[test]
    fn builtin_installs_both_assets_and_repairs_missing_files() {
        let root = tempfile::tempdir().unwrap();
        let mut state = types::DesktopPetState {
            scale: 0.25,
            ..Default::default()
        };
        install_into_state(root.path(), &mut state).unwrap();
        let original = state.pet_path.clone();
        assert_eq!(state.sprite_version_number, Some(2));
        assert_eq!(state.scale, 0.25);
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
