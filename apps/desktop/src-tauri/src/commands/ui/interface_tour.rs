//! Interface tour progress is independent from verified first-run setup.
use serde::{Deserialize, Serialize};
use std::path::Path;

const VERSION: u32 = 1;
const SETTINGS: &[&str] = &["desktop", "interface_tour"];

#[derive(Default, Deserialize, Serialize)]
#[serde(default)]
pub struct InterfaceTourState {
    resolved_version: u32,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TourOutcome {
    Completed,
    Skipped,
}

fn read_at(base: &Path) -> Result<InterfaceTourState, String> {
    home::settings::read(base, SETTINGS)
        .map(|value| value.unwrap_or_default())
        .map_err(|error| error.to_string())
}

fn resolve_at(base: &Path, outcome: TourOutcome) -> Result<(), String> {
    home::settings::update(base, |doc| {
        if let Some(desktop) = doc.get("desktop") {
            anyhow::ensure!(desktop.is_table_like(), "desktop settings must be a table");
            if let Some(tour) = desktop.get("interface_tour") {
                anyhow::ensure!(tour.is_table_like(), "interface_tour must be a table");
            }
        }
        let tour = &mut doc["desktop"]["interface_tour"];
        let existing = tour
            .get("resolved_version")
            .and_then(|v| v.as_integer())
            .unwrap_or(0);
        // An older client must not downgrade a newer tour's progress.
        if existing > i64::from(VERSION) {
            return Ok(());
        }
        tour["resolved_version"] = i64::from(VERSION).into();
        // Replaying then skipping does not erase a completed tour.
        if tour.get("outcome").and_then(|v| v.as_str()) != Some("completed") {
            tour["outcome"] = match outcome {
                TourOutcome::Completed => "completed",
                TourOutcome::Skipped => "skipped",
            }
            .into();
        }
        Ok(())
    })
    .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_interface_tour_state() -> Result<InterfaceTourState, String> {
    tauri::async_runtime::spawn_blocking(|| read_at(&home::default_memory_dir()))
        .await
        .map_err(|error| error.to_string())?
}

#[tauri::command]
pub async fn resolve_interface_tour(outcome: TourOutcome) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || resolve_at(&home::default_memory_dir(), outcome))
        .await
        .map_err(|error| error.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_read_does_not_create_settings() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(read_at(root.path()).unwrap().resolved_version, 0);
        assert!(!home::config_path(root.path()).exists());
    }

    #[test]
    fn skip_and_complete_persist_without_changing_setup_or_other_settings() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(home::config_path(root.path()),
            "# keep\n[desktop.tools]\nexec_command = false\n[desktop.interface_tour]\ncustom_date = 2026-09-10 # keep date\n").unwrap();
        resolve_at(root.path(), TourOutcome::Skipped).unwrap();
        assert_eq!(read_at(root.path()).unwrap().resolved_version, VERSION);
        resolve_at(root.path(), TourOutcome::Completed).unwrap();
        resolve_at(root.path(), TourOutcome::Skipped).unwrap();
        let text = std::fs::read_to_string(home::config_path(root.path())).unwrap();
        assert!(text.contains("# keep"));
        assert!(text.contains("custom_date = 2026-09-10 # keep date"));
        assert!(text.contains("exec_command = false"));
        assert!(text.contains("outcome = \"completed\""));
        assert!(!home::onboarding_path(root.path()).exists());
    }

    #[test]
    fn newer_versions_are_not_downgraded_and_corrupt_settings_are_not_overwritten() {
        let root = tempfile::tempdir().unwrap();
        let path = home::config_path(root.path());
        std::fs::write(&path, "[desktop.interface_tour]\nresolved_version = 99\n").unwrap();
        resolve_at(root.path(), TourOutcome::Skipped).unwrap();
        assert_eq!(read_at(root.path()).unwrap().resolved_version, 99);
        std::fs::write(&path, "[desktop]\ninterface_tour = false\n").unwrap();
        assert!(read_at(root.path()).is_err());
        assert!(resolve_at(root.path(), TourOutcome::Skipped).is_err());
        assert!(std::fs::read_to_string(path)
            .unwrap()
            .contains("interface_tour = false"));
    }
}
