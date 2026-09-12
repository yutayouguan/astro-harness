//! Onboarding reuses real OS preferences and the shared pet transaction.
use serde::{Deserialize, Serialize};
use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::{NotificationExt, PermissionState};
static AUTOSTART_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const NOTIFICATIONS: &[&str] = &["desktop", "notifications"];
#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
struct NotificationPreferences {
    enabled: bool,
}

fn write_notifications_at(base: &Path, enabled: bool) -> Result<(), String> {
    home::settings::update(base, |doc| {
        if let Some(desktop) = doc.get("desktop") {
            anyhow::ensure!(desktop.is_table_like(), "desktop settings must be a table");
            if let Some(notifications) = desktop.get("notifications") {
                anyhow::ensure!(
                    notifications.is_table_like(),
                    "notifications settings must be a table"
                );
            }
        }
        doc["desktop"]["notifications"]["enabled"] = enabled.into();
        Ok(())
    })
    .map_err(|e| e.to_string())
}

pub(crate) fn notifications_enabled_at(base: &Path) -> Result<bool, String> {
    home::settings::read::<NotificationPreferences>(base, NOTIFICATIONS)
        .map(|value| value.unwrap_or_default().enabled)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_task_notifications_enabled() -> Result<bool, String> {
    notifications_enabled_at(&home::default_memory_dir())
}

#[tauri::command]
pub async fn set_task_notifications_enabled(app: AppHandle, enabled: bool) -> Result<bool, String> {
    // Permission prompts only follow an explicit toggle, never application startup.
    if enabled
        && app
            .notification()
            .permission_state()
            .map_err(|e| e.to_string())?
            != PermissionState::Granted
    {
        if app
            .notification()
            .request_permission()
            .map_err(|e| e.to_string())?
            != PermissionState::Granted
        {
            return Err("系统未允许通知，请在系统设置中授权后重试".into());
        }
    }
    tauri::async_runtime::spawn_blocking(move || {
        write_notifications_at(&home::default_memory_dir(), enabled)?;
        Ok(enabled)
    })
    .await
    .map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn get_desktop_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_desktop_autostart(app: AppHandle, enabled: bool) -> Result<bool, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let _guard = AUTOSTART_LOCK
            .lock()
            .map_err(|_| "Autostart settings lock unavailable")?;
        let manager = app.autolaunch();
        let previous = manager.is_enabled().map_err(|e| e.to_string())?;
        if previous == enabled {
            return Ok(previous);
        }
        let result = if enabled {
            manager.enable()
        } else {
            manager.disable()
        }
        .and_then(|_| manager.is_enabled());
        match result {
            Ok(actual) if actual == enabled => Ok(actual),
            _ => {
                let restored = if previous {
                    manager.enable()
                } else {
                    manager.disable()
                };
                Err(if restored.is_ok() {
                    "登录启动设置未生效，已恢复原状态"
                } else {
                    "登录启动设置未生效，请重新读取系统状态"
                }
                .into())
            }
        }
    })
    .await
    .map_err(|e| e.to_string())?
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingPetInfo {
    enabled: bool,
    has_asset: bool,
    preview_path: Option<String>,
    sprite_version_number: Option<u32>,
    display_name: Option<String>,
}

#[tauri::command]
pub fn get_onboarding_pet() -> Result<OnboardingPetInfo, String> {
    let state =
        types::read_desktop_pet_state(&home::default_memory_dir()).map_err(|e| e.to_string())?;
    let has_asset = state
        .pet_path
        .as_deref()
        .is_some_and(|path| Path::new(path).is_file());
    Ok(OnboardingPetInfo {
        enabled: state.enabled,
        has_asset,
        preview_path: state.pet_path,
        sprite_version_number: state.sprite_version_number,
        display_name: state.display_name,
    })
}

/// Only called after model/workspace validation. Never replace an existing pet.
pub(super) fn apply_pet_choice_at(base: &Path, enabled: Option<bool>) -> Result<(), String> {
    let Some(enabled) = enabled else {
        return Ok(());
    };
    let current = types::read_desktop_pet_state(base).map_err(|e| e.to_string())?;
    if current.enabled == enabled
        && (!enabled
            || current
                .pet_path
                .as_deref()
                .is_some_and(|p| Path::new(p).is_file()))
    {
        return Ok(());
    }
    types::update_desktop_pet_state(base, |state| {
        if enabled {
            if let Some(path) = state.pet_path.as_deref() {
                anyhow::ensure!(
                    Path::new(path).is_file(),
                    "原有桌宠资源不可用，请在设置中重新选择"
                );
            } else {
                super::builtin_pet::install_into_state(base, state)?;
            }
        }
        state.enabled = enabled;
        state.updated_at = chrono::Utc::now().to_rfc3339();
        Ok(())
    })
    .map(|_| ())
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn preferences_default_off_and_reading_does_not_create_config() {
        let root = tempfile::tempdir().unwrap();
        assert!(!notifications_enabled_at(root.path()).unwrap());
        assert!(!home::config_path(root.path()).exists());
        apply_pet_choice_at(root.path(), None).unwrap();
        assert!(!types::desktop_pet_state_path(root.path()).exists());
        write_notifications_at(root.path(), true).unwrap();
        assert!(notifications_enabled_at(root.path()).unwrap());
    }
    #[test]
    fn builtin_pet_is_installed_only_when_chosen_and_is_reused() {
        let root = tempfile::tempdir().unwrap();
        apply_pet_choice_at(root.path(), Some(false)).unwrap();
        assert!(types::read_desktop_pet_state(root.path())
            .unwrap()
            .pet_path
            .is_none());
        apply_pet_choice_at(root.path(), Some(true)).unwrap();
        let first = types::read_desktop_pet_state(root.path()).unwrap();
        assert!(first.enabled);
        assert_eq!(
            fs::read(first.pet_path.as_ref().unwrap()).unwrap(),
            super::super::builtin_pet::SPRITESHEET
        );
        assert_eq!(first.sprite_version_number, Some(2));
        assert_eq!(
            fs::read(first.grooming_path.as_ref().unwrap()).unwrap(),
            super::super::builtin_pet::GROOMING
        );
        apply_pet_choice_at(root.path(), Some(true)).unwrap();
        assert_eq!(
            types::read_desktop_pet_state(root.path()).unwrap().revision,
            first.revision
        );
        apply_pet_choice_at(root.path(), Some(false)).unwrap();
        assert_eq!(
            types::read_desktop_pet_state(root.path()).unwrap().pet_path,
            first.pet_path
        );
    }
    #[test]
    fn notification_writes_preserve_other_settings() {
        let root = tempfile::tempdir().unwrap();
        fs::write(
            home::config_path(root.path()),
            "# keep\n[desktop.tools]\nexec_command = false\n[desktop.notifications]\ncustom_date = 2026-09-09 # preserve date\n",
        )
        .unwrap();
        write_notifications_at(root.path(), true).unwrap();
        assert!(notifications_enabled_at(root.path()).unwrap());
        assert!(fs::read_to_string(home::config_path(root.path()))
            .unwrap()
            .contains("# keep"));
        assert!(fs::read_to_string(home::config_path(root.path()))
            .unwrap()
            .contains("custom_date = 2026-09-09 # preserve date"));
        assert_eq!(
            home::settings::read::<bool>(root.path(), &["desktop", "tools", "exec_command"])
                .unwrap(),
            Some(false)
        );
    }

    #[test]
    fn existing_pet_and_preferences_are_never_replaced_by_builtin() {
        let root = tempfile::tempdir().unwrap();
        let asset = root.path().join("my-pet.png");
        fs::write(&asset, b"existing user asset").unwrap();
        types::update_desktop_pet_state(root.path(), |state| {
            state.pet_path = Some(asset.to_string_lossy().into_owned());
            state.display_name = Some("My pet".into());
            state.scale = 0.25;
            Ok(())
        })
        .unwrap();
        apply_pet_choice_at(root.path(), Some(true)).unwrap();
        let pet = types::read_desktop_pet_state(root.path()).unwrap();
        assert_eq!(pet.pet_path.as_deref(), asset.to_str());
        assert_eq!(pet.display_name.as_deref(), Some("My pet"));
        assert_eq!(pet.scale, 0.25);
        assert_eq!(fs::read(asset).unwrap(), b"existing user asset");
        apply_pet_choice_at(root.path(), None).unwrap();
        assert!(types::read_desktop_pet_state(root.path()).unwrap().enabled);
    }
}
