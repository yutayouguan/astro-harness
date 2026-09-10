//! Desktop pet persistent state shared by Agent tools and the Tauri shell.

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

use serde::{Deserialize, Serialize};

pub const DESKTOP_PET_V2_SPRITE_VERSION: u32 = 2;
pub const DESKTOP_PET_V2_COLUMNS: u32 = 8;
pub const DESKTOP_PET_V2_ROWS: u32 = 11;
pub const DESKTOP_PET_V2_CELL_WIDTH: u32 = 192;
pub const DESKTOP_PET_V2_CELL_HEIGHT: u32 = 208;
pub const DESKTOP_PET_V2_WIDTH: u32 = DESKTOP_PET_V2_COLUMNS * DESKTOP_PET_V2_CELL_WIDTH;
pub const DESKTOP_PET_V2_HEIGHT: u32 = DESKTOP_PET_V2_ROWS * DESKTOP_PET_V2_CELL_HEIGHT;
pub const DESKTOP_PET_V2_USED_COLUMNS: [u32; 11] = [6, 8, 8, 4, 5, 8, 6, 6, 6, 8, 8];
pub const DESKTOP_PET_GROOMING_WIDTH: u32 = 6 * DESKTOP_PET_V2_CELL_WIDTH;
pub const DESKTOP_PET_MIN_SCALE: f64 = 0.30;
pub const DESKTOP_PET_MAX_SCALE: f64 = 0.60;
pub const DESKTOP_PET_DEFAULT_SCALE: f64 = 0.40;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPetManifest {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub sprite_version_number: u32,
    pub spritesheet_path: String,
    /// Optional Astro-only six-frame paw-grooming strip; base atlas remains Codex v2.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grooming_spritesheet_path: Option<String>,
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub motion_clips: crate::pet_motion::PetMotionClips,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_preferences: Option<crate::pet_preferences::PetScenePreferences>,
}

type DesktopPetChangeHandler = Arc<dyn Fn() + Send + Sync + 'static>;
static DESKTOP_PET_CHANGE_HANDLER: OnceLock<RwLock<Option<DesktopPetChangeHandler>>> =
    OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopPetState {
    /// Monotonic across all writers under state.lock; old state files start at zero.
    pub revision: u64,
    pub enabled: bool,
    pub source_path: Option<String>,
    pub pet_path: Option<String>,
    pub active_pet_id: Option<String>,
    pub active_scene_id: Option<String>,
    pub pets: Vec<crate::pet_library::PetRecord>,
    #[serde(default)]
    pub library_version: u32,
    pub grooming_path: Option<String>,
    pub motion_clips: crate::pet_motion::PetMotionClips,
    pub scale: f64,
    pub preferences: crate::pet_preferences::PetPreferences,
    pub always_on_top: bool,
    pub updated_at: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub sprite_version_number: Option<u32>,
    pub display_name: Option<String>,
    pub description: Option<String>,
    pub follow_wallpaper: bool,
    pub last_wallpaper_path: Option<String>,
    pub scenes: Vec<crate::pet_scene::PetScene>,
    pub favorite_scene_ids: Vec<String>,
    pub animation_paused: bool,
    /// Durable outbox: recovered before reads, so a crash cannot lose a scene wallpaper apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_scene_style: Option<crate::UiStyleManifest>,
}

impl Default for DesktopPetState {
    fn default() -> Self {
        Self {
            revision: 0,
            enabled: false,
            source_path: None,
            pet_path: None,
            active_pet_id: None,
            active_scene_id: None,
            pets: Vec::new(),
            library_version: 1,
            grooming_path: None,
            motion_clips: Default::default(),
            scale: DESKTOP_PET_DEFAULT_SCALE,
            preferences: Default::default(),
            always_on_top: true,
            updated_at: String::new(),
            provider: None,
            model: None,
            sprite_version_number: None,
            display_name: None,
            description: None,
            follow_wallpaper: false,
            last_wallpaper_path: None,
            scenes: Vec::new(),
            favorite_scene_ids: Vec::new(),
            animation_paused: false,
            pending_scene_style: None,
        }
    }
}

pub fn desktop_pet_root(base: &Path) -> PathBuf {
    home::desktop_pet_dir(base)
}

pub fn desktop_pet_state_path(base: &Path) -> PathBuf {
    desktop_pet_root(base).join("state.json")
}

fn with_state_lock<T>(
    base: &Path,
    operation: impl FnOnce() -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let root = desktop_pet_root(base);
    fs::create_dir_all(&root)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("state.lock"))?;
    lock.lock()?;
    operation()
}

pub fn read_desktop_pet_state(base: &Path) -> anyhow::Result<DesktopPetState> {
    with_state_lock(base, || read_desktop_pet_state_unlocked(base))
}

pub fn write_desktop_pet_state(base: &Path, state: &DesktopPetState) -> anyhow::Result<()> {
    update_desktop_pet_state(base, |current| {
        let revision = current.revision;
        *current = state.clone();
        current.revision = revision;
        Ok(())
    })?;
    Ok(())
}

pub fn update_desktop_pet_state(
    base: &Path,
    update: impl FnOnce(&mut DesktopPetState) -> anyhow::Result<()>,
) -> anyhow::Result<DesktopPetState> {
    with_state_lock(base, || {
        let mut state = read_desktop_pet_state_unlocked(base)?;
        let revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("桌宠状态版本已耗尽"))?;
        update(&mut state)?;
        crate::pet_library::reconcile(&mut state)?;
        state.revision = revision;
        validate_state(&state)?;
        write_desktop_pet_state_unlocked(base, &state)?;
        Ok(state)
    })
}

fn read_desktop_pet_state_unlocked(base: &Path) -> anyhow::Result<DesktopPetState> {
    let path = desktop_pet_state_path(base);
    if !path.try_exists()? {
        return Ok(DesktopPetState::default());
    }
    let bytes = read_limited_pet_file(&path, 1024 * 1024)?;
    let mut state: DesktopPetState = serde_json::from_slice(&bytes)?;
    // Display-size policy changed: old default 1.0 becomes the new default;
    // other formerly valid oversized choices are capped, without rewriting on read.
    if state.scale == 1.0 {
        state.scale = DESKTOP_PET_DEFAULT_SCALE;
    } else if state.scale > DESKTOP_PET_MAX_SCALE && state.scale <= 1.35 {
        state.scale = DESKTOP_PET_MAX_SCALE;
    }
    if state.library_version == 0 {
        crate::pet_library::reconcile(&mut state)?;
        validate_state(&state)?;
        let backup = desktop_pet_root(base).join("state.before-pet-library.json");
        if !backup.try_exists()? {
            crate::pet_scene::atomic_write(&backup, &bytes)?;
        }
        state.library_version = 1;
        state.revision = state
            .revision
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("桌宠状态版本已耗尽"))?;
        write_desktop_pet_state_unlocked(base, &state)?;
    }
    validate_state(&state)?;
    if let Some(style) = state.pending_scene_style.as_ref() {
        crate::pet_scene::publish_style(base, style)?;
        state.pending_scene_style = None;
        write_desktop_pet_state_unlocked(base, &state)?;
    }
    Ok(state)
}

fn validate_state(state: &DesktopPetState) -> anyhow::Result<()> {
    crate::pet_library::validate(state)?;
    state.preferences.validate()?;
    crate::pet_motion::validate_motion_clips(&state.motion_clips)?;
    anyhow::ensure!(
        state.grooming_path.is_none() || state.sprite_version_number == Some(2),
        "舔爪扩展需要 v2 动画桌宠"
    );
    anyhow::ensure!(state.scenes.len() <= 100, "场景收藏已达 100 个上限");
    let mut ids = std::collections::HashSet::new();
    for scene in &state.scenes {
        scene.validate()?;
        anyhow::ensure!(ids.insert(&scene.id), "场景 id 重复");
    }
    anyhow::ensure!(
        state.favorite_scene_ids.len() <= 100
            && state.favorite_scene_ids.iter().all(|id| ids.contains(id)),
        "收藏场景不存在"
    );
    if let Some(style) = &state.pending_scene_style {
        style.validate().map_err(anyhow::Error::msg)?;
    }
    anyhow::ensure!(
        state.scale.is_finite()
            && (DESKTOP_PET_MIN_SCALE..=DESKTOP_PET_MAX_SCALE).contains(&state.scale),
        "桌宠大小必须在 0.30..=0.60 之间（界面 75%–150%）"
    );
    anyhow::ensure!(
        state.sprite_version_number.is_none() || state.sprite_version_number == Some(2),
        "不支持的桌宠动画版本"
    );
    Ok(())
}

/// Check the open file, then cap the read as well so growth cannot bypass the limit.
pub fn read_limited_pet_file(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    let file = fs::File::open(path)?;
    let metadata = file.metadata()?;
    anyhow::ensure!(metadata.is_file(), "请选择普通文件");
    anyhow::ensure!(metadata.len() <= limit, "文件超过 {} 字节限制", limit);
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1)).read_to_end(&mut bytes)?;
    anyhow::ensure!(bytes.len() as u64 <= limit, "文件超过 {} 字节限制", limit);
    Ok(bytes)
}

fn write_desktop_pet_state_unlocked(base: &Path, state: &DesktopPetState) -> anyhow::Result<()> {
    let root = desktop_pet_root(base);
    let path = desktop_pet_state_path(base);
    let mut temporary = tempfile::NamedTempFile::new_in(&root)?;
    let bytes = serde_json::to_vec_pretty(state)?;
    anyhow::ensure!(bytes.len() <= 1024 * 1024, "桌宠场景配置过大");
    temporary.write_all(&bytes)?;
    temporary.as_file().sync_all()?;
    // persist replaces atomically on Windows too, without deleting the old state first.
    temporary.persist(&path).map_err(|error| error.error)?;
    Ok(())
}

pub fn set_desktop_pet_change_handler(handler: DesktopPetChangeHandler) {
    if let Ok(mut slot) = DESKTOP_PET_CHANGE_HANDLER
        .get_or_init(|| RwLock::new(None))
        .write()
    {
        *slot = Some(handler);
    }
}

pub fn notify_desktop_pet_changed() {
    let handler = DESKTOP_PET_CHANGE_HANDLER
        .get()
        .and_then(|slot| slot.read().ok())
        .and_then(|slot| slot.clone());
    if let Some(handler) = handler {
        handler();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_with_camel_case_contract() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = DesktopPetState {
            enabled: true,
            pet_path: Some("/tmp/pet.png".into()),
            scale: 0.5,
            always_on_top: false,
            updated_at: "now".into(),
            ..DesktopPetState::default()
        };
        crate::pet_library::reconcile(&mut state).unwrap();
        write_desktop_pet_state(temp.path(), &state).unwrap();
        let loaded = read_desktop_pet_state(temp.path()).unwrap();
        assert_eq!(loaded.revision, 1);
        assert_eq!(
            loaded,
            DesktopPetState {
                revision: 1,
                ..state
            }
        );
        let raw = fs::read_to_string(desktop_pet_state_path(temp.path())).unwrap();
        assert!(raw.contains("\"petPath\""));
        assert!(raw.contains("\"alwaysOnTop\""));
        assert!(raw.contains("\"spriteVersionNumber\""));
    }

    #[test]
    fn transactional_update_preserves_unrelated_fields() {
        let temp = tempfile::tempdir().unwrap();
        let initial = DesktopPetState {
            pet_path: Some("/tmp/pet.png".into()),
            provider: Some("provider".into()),
            ..DesktopPetState::default()
        };
        write_desktop_pet_state(temp.path(), &initial).unwrap();
        let updated = update_desktop_pet_state(temp.path(), |state| {
            state.scale = 0.55;
            Ok(())
        })
        .unwrap();
        assert_eq!(updated.scale, 0.55);
        assert_eq!(updated.pet_path, initial.pet_path);
        assert_eq!(updated.provider, initial.provider);
        assert_eq!(updated.revision, 2);
    }

    #[test]
    fn rejected_update_preserves_the_previous_file() {
        let temp = tempfile::tempdir().unwrap();
        write_desktop_pet_state(temp.path(), &DesktopPetState::default()).unwrap();
        let before = fs::read(desktop_pet_state_path(temp.path())).unwrap();
        assert!(update_desktop_pet_state(temp.path(), |state| {
            state.scale = f64::NAN;
            Ok(())
        })
        .is_err());
        assert_eq!(
            fs::read(desktop_pet_state_path(temp.path())).unwrap(),
            before
        );
    }

    #[test]
    fn desktop_pet_small_scale_round_trips_without_clamping() {
        let temp = tempfile::tempdir().unwrap();
        for scale in [
            DESKTOP_PET_MIN_SCALE,
            0.325,
            0.5,
            0.55,
            DESKTOP_PET_MAX_SCALE,
        ] {
            update_desktop_pet_state(temp.path(), |state| {
                state.scale = scale;
                Ok(())
            })
            .unwrap();
            assert_eq!(read_desktop_pet_state(temp.path()).unwrap().scale, scale);
        }
        for scale in [0.29, 0.61, 1.36, f64::INFINITY] {
            assert!(update_desktop_pet_state(temp.path(), |state| {
                state.scale = scale;
                Ok(())
            })
            .is_err());
        }
        assert_eq!(
            read_desktop_pet_state(temp.path()).unwrap().scale,
            DESKTOP_PET_MAX_SCALE
        );
    }

    #[test]
    fn legacy_state_migrates_once_and_corruption_is_not_overwritten() {
        let temp = tempfile::tempdir().unwrap();
        fs::create_dir_all(desktop_pet_root(temp.path())).unwrap();
        let path = desktop_pet_state_path(temp.path());
        fs::write(&path, br#"{"scale":1.0}"#).unwrap();
        assert_eq!(read_desktop_pet_state(temp.path()).unwrap().scale, 0.4);
        assert_eq!(read_desktop_pet_state(temp.path()).unwrap().revision, 1);
        fs::write(&path, br#"{"scale":0.65}"#).unwrap();
        assert_eq!(read_desktop_pet_state(temp.path()).unwrap().scale, 0.6);
        fs::write(&path, br#"{"scale":0.4}"#).unwrap();
        assert_eq!(read_desktop_pet_state(temp.path()).unwrap().scale, 0.4);
        fs::write(&path, "broken").unwrap();
        assert!(write_desktop_pet_state(temp.path(), &DesktopPetState::default()).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "broken");
    }

    #[test]
    fn bounded_read_rejects_oversized_and_non_regular_files() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("large");
        fs::File::create(&path)
            .unwrap()
            .set_len(51 * 1024 * 1024)
            .unwrap();
        assert!(read_limited_pet_file(&path, 50 * 1024 * 1024).is_err());
        assert!(read_limited_pet_file(temp.path(), 50 * 1024 * 1024).is_err());
    }

    #[test]
    fn concurrent_writers_publish_unique_monotonic_revisions() {
        let temp = tempfile::tempdir().unwrap();
        let workers = (0..8)
            .map(|_| {
                let base = temp.path().to_path_buf();
                std::thread::spawn(move || {
                    update_desktop_pet_state(&base, |_| Ok(()))
                        .unwrap()
                        .revision
                })
            })
            .collect::<Vec<_>>();
        let mut revisions = workers
            .into_iter()
            .map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>();
        revisions.sort_unstable();
        assert_eq!(revisions, (1..=8).collect::<Vec<_>>());
        assert_eq!(read_desktop_pet_state(temp.path()).unwrap().revision, 8);
    }
}
