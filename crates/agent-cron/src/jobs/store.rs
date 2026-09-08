//! 定时任务 JSON 持久化与 CronStore。

use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use chrono::{DateTime, Duration, Local, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use home::{default_memory_dir, ensure_default_workspace_dirs};

use super::model::{
    default_agent_id, normalize_cron_agent_id, title_from_task, CronJob, NewCronJob,
};
use super::schedule::{compute_next_run, ensure_custom_start};

static STORE_LOCK: Mutex<()> = Mutex::new(());

struct StoreLockGuard {
    _process: MutexGuard<'static, ()>,
    _file: fs::File,
}

fn lock_store(root: &std::path::Path) -> anyhow::Result<StoreLockGuard> {
    let process = STORE_LOCK
        .lock()
        .map_err(|error| anyhow::anyhow!("cron store lock poisoned: {error}"))?;
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(root.join("jobs.lock"))?;
    file.lock()?;
    Ok(StoreLockGuard {
        _process: process,
        _file: file,
    })
}

/// `jobs.json` 顶层结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub(crate) struct JobsFile {
    pub(crate) jobs: Vec<CronJob>,
}

/// 定时任务持久化存储（根目录含 `jobs.json` 与 `output/`）
pub struct CronStore {
    root: PathBuf,
}

impl CronStore {
    /// 打开或初始化指定 cron 根目录
    pub fn open(root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        fs::create_dir_all(root.join("output"))?;
        Ok(Self { root })
    }

    /// 打开默认 `~/.astro/automation/cron`（会先确保工作区存在）
    pub fn open_default() -> anyhow::Result<Self> {
        ensure_default_workspace_dirs()?;
        Self::open(home::cron_dir(&default_memory_dir()))
    }

    /// `jobs.json` 路径
    pub fn jobs_path(&self) -> PathBuf {
        self.root.join("jobs.json")
    }

    /// 列出全部任务（加载时补全缺省 title / agent_id）
    pub fn list(&self) -> anyhow::Result<Vec<CronJob>> {
        let _guard = lock_store(&self.root)?;
        Ok(self
            .load_unlocked()?
            .jobs
            .into_iter()
            .filter(|job| job.agent_id == default_agent_id())
            .collect())
    }

    /// 快捷添加：仅 schedule + task，其余用默认值
    pub fn add(&self, schedule: &str, task: &str) -> anyhow::Result<CronJob> {
        self.add_job(NewCronJob {
            schedule: schedule.to_string(),
            task: task.to_string(),
            title: title_from_task(task),
            agent_id: default_agent_id(),
            provider_id: None,
            model: None,
            show_in_chat: false,
        })
    }

    /// 添加新任务：校验 schedule、计算 `next_run_at` 并持久化
    pub fn add_job(&self, input: NewCronJob) -> anyhow::Result<CronJob> {
        let raw_schedule = input.schedule.trim();
        let task = input.task.trim();
        if raw_schedule.is_empty() {
            anyhow::bail!("schedule 不能为空");
        }
        if task.is_empty() {
            anyhow::bail!("task 不能为空");
        }
        let title = if input.title.trim().is_empty() {
            title_from_task(task)
        } else {
            input.title.trim().to_string()
        };
        let agent_id = default_agent_id();
        // 自定义日历周期以创建时刻为相位锚点；随后校验表达式。
        let now = Local::now();
        let schedule = ensure_custom_start(raw_schedule, now);
        let next =
            compute_next_run(&schedule, now)?.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let _guard = lock_store(&self.root)?;
        let mut file = self.load_unlocked()?;
        let job = CronJob {
            id: Uuid::new_v4().to_string(),
            schedule,
            task: task.to_string(),
            title,
            agent_id,
            provider_id: input.provider_id,
            model: input.model,
            enabled: true,
            created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            last_run_at: None,
            next_run_at: Some(next),
            show_in_chat: input.show_in_chat,
        };
        file.jobs.push(job.clone());
        self.save_unlocked(&file)?;
        Ok(job)
    }

    /// 按 id 或 id 前缀更新任务；未找到返回 `Ok(None)`
    pub fn update_job(
        &self,
        id_or_prefix: &str,
        input: NewCronJob,
    ) -> anyhow::Result<Option<CronJob>> {
        let raw_schedule = input.schedule.trim();
        let task = input.task.trim();
        if raw_schedule.is_empty() {
            anyhow::bail!("schedule 不能为空");
        }
        if task.is_empty() {
            anyhow::bail!("task 不能为空");
        }
        let title = if input.title.trim().is_empty() {
            title_from_task(task)
        } else {
            input.title.trim().to_string()
        };
        let agent_id = default_agent_id();
        let now = Local::now();
        let schedule = ensure_custom_start(raw_schedule, now);
        let next =
            compute_next_run(&schedule, now)?.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let _guard = lock_store(&self.root)?;
        let mut file = self.load_unlocked()?;
        let mut updated = None;
        for job in &mut file.jobs {
            if job.id == id_or_prefix || job.id.starts_with(id_or_prefix) {
                job.schedule = schedule.clone();
                job.task = task.to_string();
                job.title = title;
                job.agent_id = agent_id;
                job.provider_id = input.provider_id;
                job.model = input.model;
                job.show_in_chat = input.show_in_chat;
                job.next_run_at = Some(next);
                updated = Some(job.clone());
                break;
            }
        }
        if updated.is_some() {
            self.save_unlocked(&file)?;
        }
        Ok(updated)
    }

    /// 按 id 或前缀删除任务；未找到返回 `Ok(false)`
    pub fn remove(&self, id_or_prefix: &str) -> anyhow::Result<bool> {
        let _guard = lock_store(&self.root)?;
        let mut file = self.load_unlocked()?;
        let before = file.jobs.len();
        file.jobs
            .retain(|j| j.id != id_or_prefix && !j.id.starts_with(id_or_prefix));
        if file.jobs.len() == before {
            return Ok(false);
        }
        self.save_unlocked(&file)?;
        Ok(true)
    }

    /// 手动执行后更新 `last_run_at`（不推进 `next_run_at`）
    pub fn touch_last_run(
        &self,
        id_or_prefix: &str,
        fired_at: Option<String>,
    ) -> anyhow::Result<bool> {
        let fired_at = fired_at
            .unwrap_or_else(|| Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true));
        let _guard = lock_store(&self.root)?;
        let mut file = self.load_unlocked()?;
        let mut found = false;
        for job in &mut file.jobs {
            if job.id == id_or_prefix || job.id.starts_with(id_or_prefix) {
                job.last_run_at = Some(fired_at);
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
        self.save_unlocked(&file)?;
        Ok(true)
    }

    /// 启用或禁用任务；启用时会补算缺失的 `next_run_at`。
    pub fn set_enabled(&self, id_or_prefix: &str, enabled: bool) -> anyhow::Result<bool> {
        let _guard = lock_store(&self.root)?;
        let mut file = self.load_unlocked()?;
        let mut found = false;
        for job in &mut file.jobs {
            if job.id == id_or_prefix || job.id.starts_with(id_or_prefix) {
                if enabled && job.next_run_at.is_none() {
                    job.next_run_at = Some(
                        compute_next_run(&job.schedule, Local::now())?
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    );
                }
                job.enabled = enabled;
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
        self.save_unlocked(&file)?;
        Ok(true)
    }

    /// 扫描到期任务：更新 next_run，返回已触发的任务（不写 output JSON）
    pub fn claim_due(&self) -> anyhow::Result<Vec<CronJob>> {
        let now = Local::now();
        let _guard = lock_store(&self.root)?;
        let mut file = self.load_unlocked()?;
        let mut fired = Vec::new();

        for job in &mut file.jobs {
            if !job.enabled || job.agent_id != default_agent_id() {
                continue;
            }
            let due = match &job.next_run_at {
                Some(next) => match DateTime::parse_from_rfc3339(next) {
                    Ok(dt) => dt <= now,
                    Err(_) => true,
                },
                None => true,
            };
            if !due {
                continue;
            }

            let fired_at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            job.last_run_at = Some(fired_at);
            job.next_run_at = Some(
                compute_next_run(&job.schedule, now + Duration::seconds(1))?
                    .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            );
            fired.push(job.clone());
        }

        if !fired.is_empty() {
            self.save_unlocked(&file)?;
        } else {
            // 只在有 next_run 需要初始化时才写文件
            let mut needs_save = false;
            for job in &mut file.jobs {
                if job.enabled && job.agent_id == default_agent_id() && job.next_run_at.is_none() {
                    job.next_run_at = Some(
                        compute_next_run(&job.schedule, now)?
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    );
                    needs_save = true;
                }
            }
            if needs_save {
                self.save_unlocked(&file)?;
            }
        }

        Ok(fired)
    }

    /// 扫描到期任务：写 heartbeat，再 `claim_due`。
    pub fn tick(&self) -> anyhow::Result<Vec<CronJob>> {
        let now = Local::now();
        self.write_heartbeat(&now)?;
        let fired = self.claim_due()?;
        if !fired.is_empty() {
            self.write_last_success(&now)?;
        }
        Ok(fired)
    }

    /// 从磁盘加载 `jobs.json`；不存在或空文件返回空列表
    #[cfg(test)]
    pub(crate) fn load(&self) -> anyhow::Result<JobsFile> {
        let _guard = lock_store(&self.root)?;
        self.load_unlocked()
    }

    fn load_unlocked(&self) -> anyhow::Result<JobsFile> {
        let path = self.jobs_path();
        if !path.exists() {
            return Ok(JobsFile::default());
        }
        let raw = fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            return Ok(JobsFile::default());
        }
        let mut file: JobsFile = serde_json::from_str(&raw)?;
        let mut migrated = false;
        for job in &mut file.jobs {
            if job.title.trim().is_empty() {
                job.title = title_from_task(&job.task);
            }
            job.agent_id = normalize_cron_agent_id(&job.agent_id);
            if let Ok(created_at) = DateTime::parse_from_rfc3339(&job.created_at) {
                let created_at = created_at.with_timezone(&Local);
                let anchored_schedule = ensure_custom_start(&job.schedule, created_at);
                if anchored_schedule != job.schedule
                    && compute_next_run(&anchored_schedule, created_at).is_ok()
                {
                    job.schedule = anchored_schedule;
                    migrated = true;
                }
            }
        }
        if migrated {
            self.save_unlocked(&file)?;
        }
        Ok(file)
    }

    /// 原子写入 `jobs.json`（先写 `.json.tmp` 再 rename）
    #[cfg(test)]
    pub(crate) fn save(&self, file: &JobsFile) -> anyhow::Result<()> {
        let _guard = lock_store(&self.root)?;
        self.save_unlocked(file)
    }

    fn save_unlocked(&self, file: &JobsFile) -> anyhow::Result<()> {
        let path = self.jobs_path();
        let tmp = path.with_extension(format!("json.{}.tmp", Uuid::new_v4().simple()));
        fs::write(&tmp, serde_json::to_string_pretty(file)?)?;
        #[cfg(target_os = "windows")]
        if path.exists() {
            fs::remove_file(&path)?;
        }
        if let Err(error) = fs::rename(&tmp, &path) {
            let _ = fs::remove_file(&tmp);
            return Err(error.into());
        }
        Ok(())
    }

    /// 记录调度器心跳时间（`ticker_heartbeat` 文件）
    fn write_heartbeat(&self, now: &DateTime<Local>) -> anyhow::Result<()> {
        fs::write(
            self.root.join("ticker_heartbeat"),
            now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        )?;
        Ok(())
    }

    /// 记录最近一次成功触发扫描的时间
    fn write_last_success(&self, now: &DateTime<Local>) -> anyhow::Result<()> {
        fs::write(
            self.root.join("ticker_last_success"),
            now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        )?;
        Ok(())
    }
}

/// 默认 cron 根目录：`~/.astro/automation/cron`
pub fn cron_dir() -> PathBuf {
    home::cron_dir(&default_memory_dir())
}
