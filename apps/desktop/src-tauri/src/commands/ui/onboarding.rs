//! 首次启动引导状态。
//!
//! 状态由 Desktop 后端持久化，避免 WebView storage 被清理后重复打扰用户。

use crate::commands::providers as provider_commands;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::sync::LazyLock;
use std::sync::Mutex;
use std::time::{Duration, Instant};
static STATE_LOCK: Mutex<()> = Mutex::new(());
use std::path::{Path, PathBuf};

const ONBOARDING_VERSION: u32 = 1;
const VERIFICATION_REQUIRED: &str = "ONBOARDING_VERIFICATION_REQUIRED";
const VERIFICATION_TTL: Duration = Duration::from_secs(15 * 60);

// Credentials already resolved by the provider layer remain in memory only.
// Neither this snapshot nor the receipt registry is Debug/Serialize.
#[derive(Clone, PartialEq, Eq)]
struct ConnectionSnapshot {
    id: String,
    backend: String,
    endpoint: String,
    model: String,
    credential: Option<String>,
}
struct VerifiedConnection {
    snapshot: ConnectionSnapshot,
    created: Instant,
}
static VERIFIED: LazyLock<Mutex<HashMap<String, VerifiedConnection>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

#[derive(Serialize)]
pub struct OnboardingVerificationDto {
    pub ok: bool,
    pub model: String,
    pub latency_ms: u64,
    pub message: String,
    pub verification_token: Option<String>,
}

fn connection_snapshot(id: &str) -> Result<ConnectionSnapshot, String> {
    let state = provider_commands::get_providers_state()?;
    let provider = state
        .providers
        .iter()
        .find(|provider| provider.id == id)
        .ok_or_else(|| VERIFICATION_REQUIRED.to_string())?;
    if !provider.enabled
        || !provider.supports_responses_api
        || (provider.key_source != "not_required" && !provider.has_api_key)
        || provider.model.trim().is_empty()
    {
        return Err(VERIFICATION_REQUIRED.into());
    }
    let config = provider_commands::find_provider(id)?;
    let (_, _, _, credential) = provider_commands::resolve_api_key(&config);
    Ok(ConnectionSnapshot {
        id: id.into(),
        backend: provider.backend_id.clone(),
        endpoint: provider.endpoint.clone(),
        model: provider.model.clone(),
        credential,
    })
}

fn ensure_verified(
    receipt: Option<&VerifiedConnection>,
    current: &ConnectionSnapshot,
) -> Result<(), String> {
    let Some(receipt) = receipt else {
        return Err(VERIFICATION_REQUIRED.into());
    };
    if receipt.created.elapsed() > VERIFICATION_TTL || receipt.snapshot != *current {
        return Err(VERIFICATION_REQUIRED.into());
    }
    Ok(())
}

/// Reuses the canonical provider probe. A successful result alone is not a persisted bypass.
#[tauri::command]
pub async fn verify_onboarding_provider(
    id: String,
    model: String,
) -> Result<OnboardingVerificationDto, String> {
    let before = connection_snapshot(&id)?;
    if before.model != model.trim() {
        return Err(VERIFICATION_REQUIRED.into());
    }
    let result = provider_commands::test_provider(id.clone(), Some(model)).await?;
    let verification_token = if result.ok {
        if connection_snapshot(&id)? != before {
            return Err(VERIFICATION_REQUIRED.into());
        }
        let token = uuid::Uuid::new_v4().to_string();
        let mut verified = VERIFIED.lock().map_err(|_| VERIFICATION_REQUIRED)?;
        verified.retain(|_, receipt| receipt.created.elapsed() <= VERIFICATION_TTL);
        if verified.len() >= 32 {
            verified.clear();
        }
        verified.insert(
            token.clone(),
            VerifiedConnection {
                snapshot: before,
                created: Instant::now(),
            },
        );
        Some(token)
    } else {
        None
    };
    Ok(OnboardingVerificationDto {
        ok: result.ok,
        model: result.model,
        latency_ms: result.latency_ms,
        message: result.message,
        verification_token,
    })
}
const ONBOARDING_FILE: &str = "onboarding.json";
const VALID_STEPS: &[&str] = &["intro", "personalize", "provider", "workspace", "complete"];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct OnboardingDraft {
    pub agent_name: String,
    pub provider_id: String,
    pub model: String,
    pub endpoint: String,
    pub workspace_path: String,
    pub permission_preset: String,
}

impl Default for OnboardingDraft {
    fn default() -> Self {
        Self {
            agent_name: "Astro".into(),
            provider_id: String::new(),
            model: String::new(),
            endpoint: String::new(),
            workspace_path: String::new(),
            permission_preset: "ask_for_approval".into(),
        }
    }
}

impl OnboardingDraft {
    fn validate(&self) -> Result<(), String> {
        if self.agent_name.chars().count() > 64 || self.agent_name.chars().any(char::is_control) {
            return Err("Agent 名称无效".into());
        }
        if self.provider_id.len() > 256
            || self.model.len() > 512
            || self.endpoint.len() > 4096
            || self.workspace_path.len() > 4096
        {
            return Err("初始化草稿字段过长".into());
        }
        if !["ask_for_approval", "approve_for_me"].contains(&self.permission_preset.as_str()) {
            return Err("初始化权限选项无效".into());
        }
        if !self.endpoint.is_empty() {
            let url = url::Url::parse(&self.endpoint).map_err(|_| "服务地址无效")?;
            if !["http", "https"].contains(&url.scheme())
                || !url.username().is_empty()
                || url.password().is_some()
                || url.query_pairs().any(|(key, _)| {
                    let key = key.to_ascii_lowercase();
                    ["key", "token", "secret", "password", "authorization"]
                        .iter()
                        .any(|part| key.contains(part))
                })
            {
                return Err("服务地址不能包含凭证".into());
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnboardingStateDto {
    pub version: u32,
    pub step: String,
    pub completed: bool,
    pub should_show: bool,
    pub inferred_existing_install: bool,
    pub updated_at: Option<String>,
    #[serde(default)]
    pub draft: OnboardingDraft,
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
            draft: OnboardingDraft::default(),
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
async fn is_established_install(base: &Path) -> Result<bool, String> {
    let database = home::session_db_path(base);
    if !database.is_file() {
        return Ok(false);
    }
    let store = session::SessionStore::open(&database)
        .await
        .map_err(|_| "无法确认历史会话，请修复数据库后重试初始化".to_string())?;
    let active = store
        .list_sessions(session::SessionListFilter::Active, 1)
        .await
        .map_err(|_| "无法读取历史会话".to_string())?;
    if !active.is_empty() {
        return Ok(true);
    }
    let archived = store
        .list_sessions(session::SessionListFilter::Archived, 1)
        .await
        .map_err(|_| "无法读取归档会话".to_string())?;
    Ok(!archived.is_empty())
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

    if is_established_install(base).await? {
        return Ok(OnboardingStateDto {
            version: ONBOARDING_VERSION,
            step: "complete".to_string(),
            completed: true,
            should_show: false,
            inferred_existing_install: true,
            updated_at: None,
            draft: OnboardingDraft::default(),
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
    let saved = (|| -> std::io::Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        // rename replaces the destination without deleting the previous state first.
        fs::rename(&temporary, &path)
    })();
    if let Err(error) = saved {
        let _ = fs::remove_file(&temporary);
        return Err(format!("无法保存首次启动状态：{error}"));
    }
    Ok(state)
}

fn save_step_at(
    base: &Path,
    step: &str,
    draft: OnboardingDraft,
) -> Result<OnboardingStateDto, String> {
    if !VALID_STEPS.contains(&step) || step == "complete" {
        return Err(format!("无效的首次启动步骤：{step}"));
    }
    draft.validate()?;
    let _guard = STATE_LOCK.lock().map_err(|_| "初始化状态锁不可用")?;
    let mut state = read_saved_at(base)?;
    // Only reset_onboarding_state may reopen a finished onboarding.
    if state.completed {
        return Ok(state);
    }
    state.draft = draft;
    state.step = step.to_string();
    write_at(base, state)
}

fn read_saved_at(base: &Path) -> Result<OnboardingStateDto, String> {
    let path = state_path(base);
    if !path.exists() {
        return Ok(OnboardingStateDto::fresh());
    }
    let raw = fs::read_to_string(path).map_err(|error| error.to_string())?;
    serde_json::from_str(&raw)
        .map(normalize)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_onboarding_state() -> Result<OnboardingStateDto, String> {
    load_at(&home::default_memory_dir()).await
}

#[tauri::command]
pub fn save_onboarding_progress(
    step: String,
    draft: OnboardingDraft,
) -> Result<OnboardingStateDto, String> {
    save_step_at(&home::default_memory_dir(), step.trim(), draft)
}

#[tauri::command]
pub fn complete_onboarding(verification_token: String) -> Result<OnboardingStateDto, String> {
    let state = provider_commands::get_providers_state()?;
    let active = state
        .active_provider_id
        .as_deref()
        .ok_or(VERIFICATION_REQUIRED)?;
    let current = connection_snapshot(active)?;
    let mut verified = VERIFIED.lock().map_err(|_| VERIFICATION_REQUIRED)?;
    ensure_verified(verified.get(&verification_token), &current)?;
    let _guard = STATE_LOCK.lock().map_err(|_| "初始化状态锁不可用")?;
    let base = home::default_memory_dir();
    let mut state = read_saved_at(&base)?;
    state.completed = true;
    state.step = "complete".into();
    let saved = write_at(&base, state)?;
    verified.remove(&verification_token);
    Ok(saved)
}

#[tauri::command]
pub fn reset_onboarding_state() -> Result<OnboardingStateDto, String> {
    VERIFIED.lock().map_err(|_| VERIFICATION_REQUIRED)?.clear();
    let _guard = STATE_LOCK.lock().map_err(|_| "初始化状态锁不可用")?;
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
        fs::write(
            temp.path().join("config.toml"),
            "[desktop.providers]\nproviders = []\n",
        )
        .unwrap();
        let database = home::session_db_path(temp.path());
        session::SessionStore::open(&database).await.unwrap();
        let state = load_at(temp.path()).await.unwrap();
        assert!(state.should_show);
        assert_eq!(state.step, "intro");
    }

    #[tokio::test]
    async fn progress_and_completion_round_trip() {
        let temp = tempfile::tempdir().unwrap();
        let state = save_step_at(temp.path(), "provider", OnboardingDraft::default()).unwrap();
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
                draft: OnboardingDraft::default(),
            },
        )
        .unwrap();
        assert!(completed.completed);
        assert!(!load_at(temp.path()).await.unwrap().should_show);
    }

    #[test]
    fn invalid_progress_step_is_rejected() {
        let temp = tempfile::tempdir().unwrap();
        assert!(save_step_at(temp.path(), "unknown", OnboardingDraft::default()).is_err());
        assert!(!state_path(temp.path()).exists());
    }

    #[tokio::test]
    async fn restores_non_secret_draft() {
        let temp = tempfile::tempdir().unwrap();
        let draft = OnboardingDraft {
            agent_name: "Nova".into(),
            workspace_path: "/tmp/work".into(),
            ..Default::default()
        };
        save_step_at(temp.path(), "workspace", draft).unwrap();
        let state = load_at(temp.path()).await.unwrap();
        assert_eq!(state.draft.agent_name, "Nova");
        assert_eq!(state.draft.workspace_path, "/tmp/work");
    }

    #[test]
    fn secret_fields_are_not_accepted_in_progress() {
        assert!(serde_json::from_str::<OnboardingDraft>(r#"{"api_key":"secret"}"#).is_err());
        assert!(serde_json::from_str::<OnboardingDraft>(r#"{"apiKey":"secret"}"#).is_err());
        let draft = OnboardingDraft {
            endpoint: "https://example.com?api_key=secret".into(),
            ..Default::default()
        };
        assert!(draft.validate().is_err());
    }

    #[test]
    fn late_progress_cannot_reopen_completion() {
        let temp = tempfile::tempdir().unwrap();
        let mut state = OnboardingStateDto::fresh();
        state.completed = true;
        state.step = "complete".into();
        write_at(temp.path(), state).unwrap();
        let saved = save_step_at(temp.path(), "provider", OnboardingDraft::default()).unwrap();
        assert!(saved.completed);
        assert_eq!(saved.step, "complete");
        write_at(temp.path(), OnboardingStateDto::fresh()).unwrap();
        assert!(!read_saved_at(temp.path()).unwrap().completed);
    }

    #[test]
    fn invalid_permissions_and_oversized_drafts_are_rejected() {
        let temp = tempfile::tempdir().unwrap();
        let draft = OnboardingDraft {
            permission_preset: "full_access".into(),
            ..Default::default()
        };
        assert!(save_step_at(temp.path(), "workspace", draft).is_err());
        let draft = OnboardingDraft {
            model: "x".repeat(513),
            ..Default::default()
        };
        assert!(save_step_at(temp.path(), "provider", draft).is_err());
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
            draft: OnboardingDraft::default(),
        });
        assert_eq!(state.step, "intro");
        assert!(state.should_show);
    }
    fn test_connection() -> ConnectionSnapshot {
        ConnectionSnapshot {
            id: "test".into(),
            backend: "openai".into(),
            endpoint: "http://127.0.0.1/v1".into(),
            model: "qa-model".into(),
            credential: Some("dummy-only".into()),
        }
    }

    #[test]
    fn completion_requires_recent_success_for_the_same_model_and_credentials() {
        let current = test_connection();
        assert!(ensure_verified(None, &current).is_err());
        let mut receipt = VerifiedConnection {
            snapshot: current.clone(),
            created: Instant::now(),
        };
        assert!(ensure_verified(Some(&receipt), &current).is_ok());
        for changed in [
            ConnectionSnapshot {
                model: "other-model".into(),
                ..current.clone()
            },
            ConnectionSnapshot {
                endpoint: "https://other.example/v1".into(),
                ..current.clone()
            },
            ConnectionSnapshot {
                credential: Some("changed".into()),
                ..current.clone()
            },
            ConnectionSnapshot {
                id: "other".into(),
                ..current.clone()
            },
        ] {
            assert!(ensure_verified(Some(&receipt), &changed).is_err());
        }
        receipt.created = Instant::now() - VERIFICATION_TTL - Duration::from_secs(1);
        assert!(ensure_verified(Some(&receipt), &current).is_err());
    }
}
