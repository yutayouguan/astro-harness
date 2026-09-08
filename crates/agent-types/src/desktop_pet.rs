//! Desktop pet persistent state shared by Agent tools and the Tauri shell.

use std::fs;
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DesktopPetManifest {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub sprite_version_number: u32,
    pub spritesheet_path: String,
}

type DesktopPetChangeHandler = Arc<dyn Fn() + Send + Sync + 'static>;
static DESKTOP_PET_CHANGE_HANDLER: OnceLock<RwLock<Option<DesktopPetChangeHandler>>> =
    OnceLock::new();

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct DesktopPetState {
    pub enabled: bool,
    pub source_path: Option<String>,
    pub pet_path: Option<String>,
    pub scale: f64,
    pub always_on_top: bool,
    pub updated_at: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub sprite_version_number: Option<u32>,
    pub display_name: Option<String>,
    pub description: Option<String>,
}

impl Default for DesktopPetState {
    fn default() -> Self {
        Self {
            enabled: false,
            source_path: None,
            pet_path: None,
            scale: 1.0,
            always_on_top: true,
            updated_at: String::new(),
            provider: None,
            model: None,
            sprite_version_number: None,
            display_name: None,
            description: None,
        }
    }
}

pub fn desktop_pet_root(base: &Path) -> PathBuf {
    base.join("ui").join("desktop-pet")
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
    with_state_lock(base, || write_desktop_pet_state_unlocked(base, state))
}

pub fn update_desktop_pet_state(
    base: &Path,
    update: impl FnOnce(&mut DesktopPetState) -> anyhow::Result<()>,
) -> anyhow::Result<DesktopPetState> {
    with_state_lock(base, || {
        let mut state = read_desktop_pet_state_unlocked(base)?;
        update(&mut state)?;
        write_desktop_pet_state_unlocked(base, &state)?;
        Ok(state)
    })
}

fn read_desktop_pet_state_unlocked(base: &Path) -> anyhow::Result<DesktopPetState> {
    let path = desktop_pet_state_path(base);
    if !path.is_file() {
        return Ok(DesktopPetState::default());
    }
    let bytes = fs::read(path)?;
    Ok(serde_json::from_slice(&bytes)?)
}

fn write_desktop_pet_state_unlocked(base: &Path, state: &DesktopPetState) -> anyhow::Result<()> {
    let root = desktop_pet_root(base);
    let path = desktop_pet_state_path(base);
    let temporary = root.join(format!(".state-{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
    #[cfg(target_os = "windows")]
    if path.exists() {
        fs::remove_file(&path)?;
    }
    if let Err(error) = fs::rename(&temporary, &path) {
        let _ = fs::remove_file(&temporary);
        return Err(error.into());
    }
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
        let state = DesktopPetState {
            enabled: true,
            pet_path: Some("/tmp/pet.png".into()),
            scale: 1.2,
            always_on_top: false,
            updated_at: "now".into(),
            ..DesktopPetState::default()
        };
        write_desktop_pet_state(temp.path(), &state).unwrap();
        assert_eq!(read_desktop_pet_state(temp.path()).unwrap(), state);
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
            state.scale = 1.25;
            Ok(())
        })
        .unwrap();
        assert_eq!(updated.scale, 1.25);
        assert_eq!(updated.pet_path, initial.pet_path);
        assert_eq!(updated.provider, initial.provider);
    }
}
