//! 首次启动引导状态。
//!
//! 状态由 Desktop 后端持久化，避免 WebView storage 被清理后重复打扰用户。

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

const ONBOARDING_VERSION: u32 = 1;
const ONBOARDING_FILE: &str = "onboarding.json";
const VALID_STEPS: &[&str] = &["intro", "personalize", "provider", "workspace", "complete"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingStateDto {
    pub version: u32,
    pub step: String,
    pub completed: bool,
    pub should_show: bool,
    pub inferred_existing_install: bool,
    pub updated_at: Option<String>,
}

impl OnboardingStateDto {
    fn fresh() -> Self {
        Self {
            version: ONBOARDING_VERSION,
            step: "intro".to_string(),
            completed: false,
            should_show: true,
            inferred_existing_install: false,
            updated_at: None,
        }
    }
}

fn state_path(base: &Path) -> PathBuf {
    base.join(ONBOARDING_FILE)
}

fn directory_has_entries(path: &Path) -> bool {
    fs::read_dir(path)
        .ok()
        .and_then(|mut entries| entries.next())
        .is_some()
}

/// 旧安装不应在升级后被强制拉回首次引导。
///
/// 这里只检查明确代表用户已使用过 Astro 的持久化产物，不使用可能在空白
/// 启动过程中就被创建的空 SQLite 文件。
fn is_established_install(base: &Path) -> bool {
    base.join("providers.json").is_file()
        || base.join("config.toml").is_file()
        || directory_has_entries(&base.join("sessions").join("rollouts"))
}

fn normalize(mut state: OnboardingStateDto) -> OnboardingStateDto {
    state.version = ONBOARDING_VERSION;
    if !VALID_STEPS.contains(&state.step.as_str()) {
        state.step = "intro".to_string();
    }
    if state.completed {
        state.step = "complete".to_string();
        state.should_show = false;
    } else {
        if state.step == "complete" {
            state.step = "intro".to_string();
        }
        state.should_show = true;
    }
    state
}

fn load_at(base: &Path) -> Result<OnboardingStateDto, String> {
    let path = state_path(base);
    if path.is_file() {
        let raw =
            fs::read_to_string(&path).map_err(|error| format!("无法读取首次启动状态：{error}"))?;
        let state = serde_json::from_str::<OnboardingStateDto>(&raw)
            .map_err(|error| format!("首次启动状态已损坏：{error}"))?;
        return Ok(normalize(state));
    }

    if is_established_install(base) {
        return Ok(OnboardingStateDto {
            version: ONBOARDING_VERSION,
            step: "complete".to_string(),
            completed: true,
            should_show: false,
            inferred_existing_install: true,
            updated_at: None,
        });
    }

    Ok(OnboardingStateDto::fresh())
}

fn write_at(base: &Path, mut state: OnboardingStateDto) -> Result<OnboardingStateDto, String> {
    fs::create_dir_all(base).map_err(|error| format!("无法创建 Astro 数据目录：{error}"))?;
    state.version = ONBOARDING_VERSION;
    state.should_show = !state.completed;
    state.inferred_existing_install = false;
    state.updated_at = Some(chrono::Utc::now().to_rfc3339());
    let bytes = serde_json::to_vec_pretty(&state)
        .map_err(|error| format!("无法序列化首次启动状态：{error}"))?;
    let path = state_path(base);
    let temporary = base.join(format!("onboarding-{}.tmp", uuid::Uuid::new_v4().simple()));
    fs::write(&temporary, bytes).map_err(|error| format!("无法写入首次启动状态：{error}"))?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(&path).map_err(|error| format!("无法更新首次启动状态：{error}"))?;
    }
    fs::rename(&temporary, &path).map_err(|error| format!("无法保存首次启动状态：{error}"))?;
    Ok(state)
}

fn save_step_at(base: &Path, step: &str) -> Result<OnboardingStateDto, String> {
    if !VALID_STEPS.contains(&step) || step == "complete" {
        return Err(format!("无效的首次启动步骤：{step}"));
    }
    let mut state = OnboardingStateDto::fresh();
    state.step = step.to_string();
    write_at(base, state)
}

#[tauri::command]
pub fn get_onboarding_state() -> Result<OnboardingStateDto, String> {
    load_at(&home::default_memory_dir())
}

#[tauri::command]
pub fn save_onboarding_progress(step: String) -> Result<OnboardingStateDto, String> {
    save_step_at(&home::default_memory_dir(), step.trim())
}

#[tauri::command]
pub fn complete_onboarding() -> Result<OnboardingStateDto, String> {
    write_at(
        &home::default_memory_dir(),
        OnboardingStateDto {
            version: ONBOARDING_VERSION,
            step: "complete".to_string(),
            completed: true,
            should_show: false,
            inferred_existing_install: false,
            updated_at: None,
        },
    )
}

#[tauri::command]
pub fn reset_onboarding_state() -> Result<OnboardingStateDto, String> {
    write_at(&home::default_memory_dir(), OnboardingStateDto::fresh())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_install_starts_at_intro() {
        let temp = tempfile::tempdir().unwrap();
        let state = load_at(temp.path()).unwrap();
        assert!(state.should_show);
        assert!(!state.completed);
        assert_eq!(state.step, "intro");
    }

    #[test]
    fn established_install_is_not_interrupted() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("providers.json"), "{}").unwrap();
        let state = load_at(temp.path()).unwrap();
        assert!(!state.should_show);
        assert!(state.completed);
        assert!(state.inferred_existing_install);
    }

    #[test]
    fn progress_and_completion_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let state = save_step_at(temp.path(), "provider").unwrap();
        assert_eq!(state.step, "provider");
        assert!(load_at(temp.path()).unwrap().should_show);

        let completed = write_at(
            temp.path(),
            OnboardingStateDto {
                version: ONBOARDING_VERSION,
                step: "complete".into(),
                completed: true,
                should_show: false,
                inferred_existing_install: false,
                updated_at: None,
            },
        )
        .unwrap();
        assert!(completed.completed);
        assert!(!load_at(temp.path()).unwrap().should_show);
    }

    #[test]
    fn invalid_progress_step_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        assert!(save_step_at(temp.path(), "unknown").is_err());
        assert!(!state_path(temp.path()).exists());
    }

    #[test]
    fn incomplete_state_cannot_resume_on_complete_screen() {
        let state = normalize(OnboardingStateDto {
            version: ONBOARDING_VERSION,
            step: "complete".into(),
            completed: false,
            should_show: true,
            inferred_existing_install: false,
            updated_at: None,
        });
        assert_eq!(state.step, "intro");
        assert!(state.should_show);
    }
}
