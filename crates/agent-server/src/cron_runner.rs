//! 后台 cron ticker：认领到期任务并调用 `agent::exec::cron`。
//!
//! 凭据解析后异步认领到期任务并执行。
//!
//! 凭据解析：读 `providers.json` + **仅环境变量** API Key（无 keyring；GUI 手动跑走 Tauri）。

use agent::exec::cron::{self as cron_exec, CronExecCredentials};
use cron::{CronJob, CronStore};
use home::default_memory_dir;
use serde::Deserialize;
use types::{expand_model_targets, FallbackRef, ModelTarget};

#[derive(Debug, Deserialize)]
struct ProvidersFile {
    #[serde(default)]
    active_provider_id: Option<String>,
    #[serde(default)]
    providers: Vec<ProviderEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct ProviderEntry {
    id: String,
    kind: String,
    #[serde(default)]
    endpoint: String,
    #[serde(default)]
    model: String,
    #[serde(default = "default_true")]
    enabled: bool,
    #[serde(default)]
    fallback: Vec<FallbackEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct FallbackEntry {
    provider_id: String,
    #[serde(default)]
    model: Option<String>,
}

fn default_true() -> bool {
    true
}

/// `providers.json` 的 kind → providers crate backend id。
fn kind_to_backend(kind: &str) -> &str {
    match kind.trim() {
        "anthropic" => "claude",
        "custom" => "openai",
        "minmax" => "minimax",
        other => other,
    }
}

fn load_providers_file() -> Option<ProvidersFile> {
    let path = default_memory_dir().join("providers.json");
    let raw = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&raw).ok()
}

fn entry_supports_agent_responses(entry: &ProviderEntry) -> bool {
    providers::dispatch::supports_agent_responses(kind_to_backend(&entry.kind))
}

fn entry_to_target(entry: &ProviderEntry) -> Option<ModelTarget> {
    if !entry.enabled {
        return None;
    }
    let backend_id = kind_to_backend(&entry.kind).to_string();
    if !entry_supports_agent_responses(entry) {
        return None;
    }
    let api_key = providers::read_env_api_key(&backend_id).unwrap_or_default();
    let allow_empty_key = backend_id == "ollama";
    if api_key.trim().is_empty() && !allow_empty_key {
        return None;
    }
    let base_url = if entry.endpoint.trim().is_empty() {
        std::env::var(format!("{}_BASE_URL", backend_id.to_uppercase())).unwrap_or_default()
    } else {
        entry.endpoint.clone()
    };
    Some(ModelTarget {
        provider_id: entry.id.clone(),
        backend_id,
        model: entry.model.clone(),
        api_key,
        base_url,
    })
}

fn find_primary_entry<'a>(file: &'a ProvidersFile, job: &CronJob) -> Option<&'a ProviderEntry> {
    if let Some(id) = job.provider_id.as_deref().filter(|s| !s.is_empty()) {
        if let Some(p) = file.providers.iter().find(|p| p.id == id) {
            return entry_supports_agent_responses(p).then_some(p);
        }
        if let Some(p) = file.providers.iter().find(|p| {
            p.enabled
                && entry_supports_agent_responses(p)
                && (kind_to_backend(&p.kind) == id || p.kind == id)
        }) {
            return Some(p);
        }
        return None;
    }
    if let Some(active) = file
        .active_provider_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .and_then(|id| {
            file.providers
                .iter()
                .find(|p| p.id == id && entry_supports_agent_responses(p))
        })
    {
        return Some(active);
    }
    file.providers
        .iter()
        .find(|p| p.enabled && entry_supports_agent_responses(p))
}

/// 从任务字段、`providers.json` 与环境变量解析执行凭据（含 fallback 链）。
fn resolve_cron_credentials(job: &CronJob) -> CronExecCredentials {
    if let Some(file) = load_providers_file() {
        if let Some(primary_entry) = find_primary_entry(&file, job) {
            if let Some(mut primary) = entry_to_target(primary_entry) {
                if let Some(m) = job
                    .model
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                {
                    primary.model = m.to_string();
                }
                let refs: Vec<FallbackRef> = primary_entry
                    .fallback
                    .iter()
                    .map(|e| FallbackRef {
                        provider_id: e.provider_id.clone(),
                        model: e.model.clone(),
                    })
                    .collect();
                let targets = expand_model_targets(&primary, &refs, |id| {
                    file.providers
                        .iter()
                        .find(|p| p.id == id)
                        .and_then(entry_to_target)
                });
                return CronExecCredentials {
                    provider: primary.backend_id.clone(),
                    model: primary.model.clone(),
                    api_key: primary.api_key.clone(),
                    base_url: primary.base_url.clone(),
                    targets,
                };
            }
        }
    }

    let provider = job
        .provider_id
        .clone()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "openai".into());
    let requested_backend = kind_to_backend(&provider);
    let backend = if providers::dispatch::supports_agent_responses(requested_backend) {
        requested_backend.to_string()
    } else {
        "openai".to_string()
    };
    let model = job.model.clone().unwrap_or_default();
    let api_key = providers::read_env_api_key(&backend).unwrap_or_default();
    let base_url =
        std::env::var(format!("{}_BASE_URL", backend.to_uppercase())).unwrap_or_default();
    CronExecCredentials {
        provider: backend,
        model,
        api_key,
        base_url,
        targets: vec![],
    }
}

/// 认领到期任务并逐个执行；打开 store / claim 失败时提前返回。
pub async fn tick_and_execute() {
    if let Err(err) = cron_exec::reconcile_orphaned_runs().await {
        tracing::warn!(error = %err, "cron tick: reconcile orphaned runs failed");
    }

    let jobs = {
        let store = match CronStore::open_default() {
            Ok(store) => store,
            Err(err) => {
                tracing::warn!(error = %err, "cron tick: failed to open store");
                return;
            }
        };

        match store.claim_due() {
            Ok(jobs) => jobs,
            Err(err) => {
                tracing::warn!(error = %err, "cron tick failed");
                return;
            }
        }
    };

    for job in jobs {
        let creds = resolve_cron_credentials(&job);
        let label = cron_notify_label(&job);
        match cron_exec::execute_job(&job, creds, "due").await {
            Ok(row) => {
                tracing::info!(
                    job_id = %job.id,
                    run_id = %row.id,
                    status = %row.status,
                    schedule = %job.schedule,
                    task = %job.task,
                    "cron job executed"
                );
                if row.status == "success" {
                    let body = if row.summary.trim().is_empty() {
                        label
                    } else {
                        format!("{label}\n{}", types::truncate_notify(&row.summary, 120))
                    };
                    types::notify_kind(types::ImportantKind::CronSuccess, body);
                } else {
                    let detail = row
                        .error
                        .as_deref()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .map(|s| types::truncate_notify(s, 120))
                        .or_else(|| {
                            let s = row.summary.trim();
                            if s.is_empty() {
                                None
                            } else {
                                Some(types::truncate_notify(s, 120))
                            }
                        })
                        .unwrap_or_else(|| row.status.clone());
                    types::notify_kind(
                        types::ImportantKind::CronFailure,
                        format!("{label}\n{detail}"),
                    );
                }
            }
            Err(err) => {
                tracing::warn!(
                    job_id = %job.id,
                    schedule = %job.schedule,
                    task = %job.task,
                    error = %err,
                    "cron job execution failed"
                );
                types::notify_kind(
                    types::ImportantKind::CronFailure,
                    format!("{label}\n{}", types::truncate_notify(&err.to_string(), 120)),
                );
            }
        }
    }
}

fn cron_notify_label(job: &CronJob) -> String {
    let title = job.title.trim();
    if !title.is_empty() {
        return title.to_string();
    }
    types::truncate_notify(job.task.trim(), 48)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_maps_anthropic_to_claude() {
        assert_eq!(kind_to_backend("anthropic"), "claude");
        assert_eq!(kind_to_backend("openai"), "openai");
    }

    #[test]
    fn expand_from_file_entries_env_only() {
        let primary = ModelTarget {
            provider_id: "p0".into(),
            backend_id: "openai".into(),
            model: "gpt".into(),
            api_key: "k0".into(),
            base_url: "https://api.openai.com/v1".into(),
        };
        let refs = vec![FallbackRef {
            provider_id: "p1".into(),
            model: Some("deepseek-chat".into()),
        }];
        let chain = expand_model_targets(&primary, &refs, |id| {
            if id == "p1" {
                Some(ModelTarget {
                    provider_id: "p1".into(),
                    backend_id: "deepseek".into(),
                    model: "deepseek-v3".into(),
                    api_key: "k1".into(),
                    base_url: "https://api.anthropic.com".into(),
                })
            } else {
                None
            }
        });
        assert_eq!(chain.len(), 2);
        assert_eq!(chain[1].model, "deepseek-chat");
    }
}
