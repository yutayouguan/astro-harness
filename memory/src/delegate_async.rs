//! 异步委派任务注册表与 spawner 钩子。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::delegate_spawn::DelegateRunRequest;
use crate::workspace::default_memory_dir;

/// 异步委派任务状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AsyncDelegateStatus {
    Running,
    Done,
    Failed,
    Cancelled,
}

/// 注册表中的一条异步委派。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncDelegateRecord {
    pub id: String,
    pub parent_session_id: String,
    pub parent_agent_id: String,
    pub status: AsyncDelegateStatus,
    #[serde(default)]
    pub result_json: String,
    #[serde(default)]
    pub error: String,
    pub created_at: String,
    #[serde(default)]
    pub finished_at: String,
    /// 取消标志（执行结束时若已取消则标 Cancelled）。
    #[serde(skip)]
    pub cancel_requested: bool,
}

#[derive(Clone, Default)]
pub struct AsyncDelegateRegistry {
    inner: Arc<Mutex<HashMap<String, AsyncDelegateRecord>>>,
}

impl AsyncDelegateRegistry {
    pub fn global() -> &'static AsyncDelegateRegistry {
        static REG: OnceLock<AsyncDelegateRegistry> = OnceLock::new();
        REG.get_or_init(AsyncDelegateRegistry::default)
    }

    pub fn insert_running(
        &self,
        parent_session_id: &str,
        parent_agent_id: &str,
    ) -> String {
        let id = Uuid::new_v4().to_string();
        let rec = AsyncDelegateRecord {
            id: id.clone(),
            parent_session_id: parent_session_id.to_string(),
            parent_agent_id: parent_agent_id.to_string(),
            status: AsyncDelegateStatus::Running,
            result_json: String::new(),
            error: String::new(),
            created_at: Utc::now().to_rfc3339(),
            finished_at: String::new(),
            cancel_requested: false,
        };
        self.inner.lock().unwrap().insert(id.clone(), rec.clone());
        let _ = save_side_file(&rec);
        id
    }

    /// 崩溃恢复：把磁盘上的 running 记录装入内存（不触发执行）。
    pub fn restore_running_record(&self, rec: AsyncDelegateRecord) {
        let mut map = self.inner.lock().unwrap();
        if !map.contains_key(&rec.id) {
            map.insert(rec.id.clone(), rec);
        }
    }

    pub fn get(&self, id: &str) -> Option<AsyncDelegateRecord> {
        self.inner.lock().unwrap().get(id).cloned()
    }

    pub fn request_cancel(&self, id: &str) -> Result<AsyncDelegateRecord, String> {
        let mut map = self.inner.lock().unwrap();
        let rec = map
            .get_mut(id)
            .ok_or_else(|| format!("unknown task_id: {id}"))?;
        match rec.status {
            AsyncDelegateStatus::Done | AsyncDelegateStatus::Failed | AsyncDelegateStatus::Cancelled => {
                // already finished
            }
            AsyncDelegateStatus::Running => {
                rec.cancel_requested = true;
                rec.status = AsyncDelegateStatus::Cancelled;
                rec.finished_at = Utc::now().to_rfc3339();
                rec.error = "cancelled by user".into();
                let _ = save_side_file(rec);
            }
        }
        Ok(rec.clone())
    }

    pub fn finish_ok(&self, id: &str, result_json: String) {
        let mut map = self.inner.lock().unwrap();
        if let Some(rec) = map.get_mut(id) {
            if rec.cancel_requested || rec.status == AsyncDelegateStatus::Cancelled {
                // keep cancelled
                let _ = save_side_file(rec);
                return;
            }
            rec.status = AsyncDelegateStatus::Done;
            rec.result_json = result_json;
            rec.finished_at = Utc::now().to_rfc3339();
            let _ = save_side_file(rec);
        }
    }

    pub fn finish_err(&self, id: &str, error: String) {
        let mut map = self.inner.lock().unwrap();
        if let Some(rec) = map.get_mut(id) {
            if rec.cancel_requested || rec.status == AsyncDelegateStatus::Cancelled {
                let _ = save_side_file(rec);
                return;
            }
            rec.status = AsyncDelegateStatus::Failed;
            rec.error = error;
            rec.finished_at = Utc::now().to_rfc3339();
            let _ = save_side_file(rec);
        }
    }

    pub fn is_cancel_requested(&self, id: &str) -> bool {
        self.inner
            .lock()
            .unwrap()
            .get(id)
            .map(|r| r.cancel_requested)
            .unwrap_or(false)
    }

    /// 异步轮询直到终态或超时（不阻塞 runtime worker）。
    pub async fn wait_until_finished(
        &self,
        id: &str,
        timeout: Duration,
    ) -> Result<AsyncDelegateRecord, String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(rec) = self.get(id) {
                match rec.status {
                    AsyncDelegateStatus::Done
                    | AsyncDelegateStatus::Failed
                    | AsyncDelegateStatus::Cancelled => return Ok(rec),
                    AsyncDelegateStatus::Running => {}
                }
            } else {
                return Err(format!("unknown task_id: {id}"));
            }
            if Instant::now() >= deadline {
                return Err(format!("timeout waiting for task_id={id}"));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

fn side_dir() -> PathBuf {
    default_memory_dir().join("delegate_async")
}

fn save_side_file(rec: &AsyncDelegateRecord) -> std::io::Result<()> {
    let dir = side_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", rec.id));
    let json = serde_json::to_vec_pretty(rec)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

fn save_request_file(task_id: &str, req: &DelegateRunRequest) -> std::io::Result<()> {
    let dir = side_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{task_id}.request.json"));
    let json = serde_json::to_vec_pretty(req)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

fn load_request_file(task_id: &str) -> Option<DelegateRunRequest> {
    let path = side_dir().join(format!("{task_id}.request.json"));
    let bytes = std::fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 扫描旁路目录中仍为 running 的任务，供重启续跑。
pub fn list_persisted_running() -> Vec<(String, DelegateRunRequest)> {
    let dir = side_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for ent in entries.flatten() {
        let path = ent.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.ends_with(".json") || name.ends_with(".request.json") {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };
        let Ok(rec) = serde_json::from_slice::<AsyncDelegateRecord>(&bytes) else {
            continue;
        };
        if rec.status != AsyncDelegateStatus::Running {
            continue;
        }
        let Some(req) = load_request_file(&rec.id) else {
            continue;
        };
        if req.api_key.trim().is_empty() {
            continue;
        }
        out.push((rec.id, req));
    }
    out
}

/// 异步 spawn：`(task_id, DelegateRunRequest)`。
pub type DelegateAsyncSpawner =
    Arc<dyn Fn(String, DelegateRunRequest) + Send + Sync + 'static>;

static ASYNC_SPAWNER: OnceLock<Mutex<Option<DelegateAsyncSpawner>>> = OnceLock::new();

/// 注册异步委派 spawner；可重复调用（测试覆盖 / 热替换）。
pub fn set_delegate_async_spawner(spawner: DelegateAsyncSpawner) {
    let slot = ASYNC_SPAWNER.get_or_init(|| Mutex::new(None));
    *slot.lock().unwrap() = Some(spawner);
}

fn take_async_spawner() -> Option<DelegateAsyncSpawner> {
    ASYNC_SPAWNER
        .get()
        .and_then(|m| m.lock().unwrap().clone())
}

/// 创建 running 记录并触发后台执行；立即返回 `task_id`。
pub fn start_delegate_async(req: DelegateRunRequest) -> anyhow::Result<String> {
    let reg = AsyncDelegateRegistry::global();
    let task_id = reg.insert_running(&req.parent_session_id, &req.parent_agent_id);
    let _ = save_request_file(&task_id, &req);
    match take_async_spawner() {
        Some(f) => {
            f(task_id.clone(), req);
            Ok(task_id)
        }
        None => {
            reg.finish_err(
                &task_id,
                "delegate async spawner not registered".into(),
            );
            anyhow::bail!("delegate async spawner not registered")
        }
    }
}

/// 重启后续跑：装入 running 记录并再次交给 spawner。
pub fn resume_incomplete_async_delegates() {
    let Some(spawner) = take_async_spawner() else {
        tracing::warn!("async delegate resume skipped: spawner not registered");
        return;
    };
    let reg = AsyncDelegateRegistry::global();
    for (task_id, req) in list_persisted_running() {
        if reg.get(&task_id).is_none() {
            reg.restore_running_record(AsyncDelegateRecord {
                id: task_id.clone(),
                parent_session_id: req.parent_session_id.clone(),
                parent_agent_id: req.parent_agent_id.clone(),
                status: AsyncDelegateStatus::Running,
                result_json: String::new(),
                error: String::new(),
                created_at: Utc::now().to_rfc3339(),
                finished_at: String::new(),
                cancel_requested: false,
            });
        }
        spawner(task_id, req);
    }
}

pub fn async_delegate_status(task_id: &str) -> Result<AsyncDelegateRecord, String> {
    AsyncDelegateRegistry::global()
        .get(task_id)
        .ok_or_else(|| format!("unknown task_id: {task_id}"))
}

pub async fn async_delegate_collect(
    task_id: &str,
    timeout_secs: u64,
) -> Result<AsyncDelegateRecord, String> {
    AsyncDelegateRegistry::global()
        .wait_until_finished(task_id, Duration::from_secs(timeout_secs.max(1)))
        .await
}

pub fn async_delegate_cancel(task_id: &str) -> Result<AsyncDelegateRecord, String> {
    AsyncDelegateRegistry::global().request_cancel(task_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::delegate_spawn::DelegateTaskSpec;
    use std::sync::Arc;

    #[test]
    fn finish_ok_and_cancel_discard() {
        let reg = AsyncDelegateRegistry::default();
        let id = reg.insert_running("sess", "agent");
        assert_eq!(reg.get(&id).unwrap().status, AsyncDelegateStatus::Running);

        reg.request_cancel(&id).unwrap();
        assert_eq!(reg.get(&id).unwrap().status, AsyncDelegateStatus::Cancelled);

        reg.finish_ok(&id, r#"{"x":1}"#.into());
        let rec = reg.get(&id).unwrap();
        assert_eq!(rec.status, AsyncDelegateStatus::Cancelled);
        assert!(rec.result_json.is_empty());
    }

    #[tokio::test]
    async fn wait_until_finished_ok() {
        let reg = AsyncDelegateRegistry::default();
        let id = reg.insert_running("sess", "agent");
        let reg2 = reg.clone();
        let id2 = id.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            reg2.finish_ok(&id2, r#"{"ok":true}"#.into());
        });
        let rec = reg
            .wait_until_finished(&id, Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(rec.status, AsyncDelegateStatus::Done);
        assert!(rec.result_json.contains("ok"));
    }

    #[test]
    fn persist_request_and_list_running() {
        let dir = tempfile::tempdir().unwrap();
        std::env::set_var("ASTRO_MEMORY_DIR", dir.path());
        set_delegate_async_spawner(Arc::new(|_id, _req| {}));
        let req = DelegateRunRequest {
            parent_agent_id: "a".into(),
            parent_session_id: "s".into(),
            provider: "openai".into(),
            model: "m".into(),
            api_key: "k".into(),
            base_url: String::new(),
            chat_targets: vec![],
            tasks: vec![DelegateTaskSpec {
                goal: "g".into(),
                context: String::new(),
            }],
            max_concurrent: 1,
            caller_depth: 0,
            max_spawn_depth: 1,
        };
        let id = start_delegate_async(req).unwrap();
        let listed = list_persisted_running();
        assert!(
            listed
                .iter()
                .any(|(tid, r)| tid == &id && r.tasks[0].goal == "g"),
            "listed={listed:?}"
        );
        std::env::remove_var("ASTRO_MEMORY_DIR");
    }
}

