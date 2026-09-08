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

/// 旧安装不应在升级后被强制拉回首次引导。
///
/// 启动过程会创建默认配置、Provider 文件和 workspace event rollout，因此只有
/// 真实会话表中存在记录才算已使用过的安装。
async fn is_established_install(base: &Path) -> bool {
    let database = home::session_db_path(base);
    if !database.is_file() {
        return false;
    }
    let Ok(store) = session::SessionStore::open(&database).await else {
        // 旧数据库无法读取时不强制展示首次引导，避免遮挡诊断入口。
        return true;
    };
    let active = store
        .list_sessions(session::SessionListFilter::Active, 1)
        .await
        .map(|sessions| !sessions.is_empty())
        .unwrap_or(true);
    if active {
        return true;
    }
    store
        .list_sessions(session::SessionListFilter::Archived, 1)
        .await
        .map(|sessions| !sessions.is_empty())
        .unwrap_or(true)
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

async fn load_at(base: &Path) -> Result<OnboardingStateDto, String> {
    let path = state_path(base);
    if path.is_file() {
        let raw =
            fs::read_to_string(&path).map_err(|error| format!("无法读取首次启动状态：{error}"))?;
        let state = serde_json::from_str::<OnboardingStateDto>(&raw)
            .map_err(|error| format!("首次启动状态已损坏：{error}"))?;
        return Ok(normalize(state));
    }

    if is_established_install(base).await {
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
pub async fn get_onboarding_state() -> Result<OnboardingStateDto, String> {
    load_at(&home::default_memory_dir()).await
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

    #[tokio::test]
    async fn fresh_install_starts_at_intro() {
        let temp = tempfile::tempdir().unwrap();
        let state = load_at(temp.path()).await.unwrap();
        assert!(state.should_show);
        assert!(!state.completed);
        assert_eq!(state.step, "intro");
    }

    #[tokio::test]
    async fn established_install_is_not_interrupted() {
        let temp = tempfile::tempdir().unwrap();
        let database = home::session_db_path(temp.path());
        let store = session::SessionStore::open(&database).await.unwrap();
        store
            .create_session("existing", "desktop", None, None, None)
            .await
            .unwrap();
        let state = load_at(temp.path()).await.unwrap();
        assert!(!state.should_show);
        assert!(state.completed);
        assert!(state.inferred_existing_install);
    }

    #[tokio::test]
    async fn generated_bootstrap_files_do_not_hide_first_run() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("providers.json"), "{}").unwrap();
        fs::write(temp.path().join("config.toml"), "").unwrap();
        let database = home::session_db_path(temp.path());
        session::SessionStore::open(&database).await.unwrap();
        let state = load_at(temp.path()).await.unwrap();
        assert!(state.should_show);
        assert_eq!(state.step, "intro");
    }

    #[tokio::test]
    async fn progress_and_completion_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let state = save_step_at(temp.path(), "provider").unwrap();
        assert_eq!(state.step, "provider");
        assert!(load_at(temp.path()).await.unwrap().should_show);

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
        assert!(!load_at(temp.path()).await.unwrap().should_show);
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
