//! 定时任务（Cron）执行器：将 [`CronJob`] 派发给 Agent 或 Provider 并完成运行记录落库。
//!
//! 负责并发互斥（同一 job 不重叠执行）、可选会话创建、600 秒超时兜底，以及成功/失败
//! 状态写入 `CronRunDb`。任务文案经 [`AgentLoop::run_turn`] 完成初始化后，由
//! [`super::headless::run_headless_multi_turn`] 驱动完整的 LLM → 工具 → LLM 多轮循环。
//!
//! 进程退出后残留的 `running` 行由 [`reconcile_orphaned_runs`] 回收：本进程未登记为活跃的
//! 记录会按关联会话终态收尾；若之后在聊天中重新生成出结果，失败的「应用退出中断」亦可升级为成功。

use std::collections::HashSet;
use std::path::Path;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use chrono::Utc;
use common::ChatTarget;
use cron::{cron_db_path, cron_dir, CronJob, CronRunDb, NewCronRun};
use home::default_memory_dir;
use providers::registry::ProviderRegistry;
use providers::streaming::Usage;
use session::{SessionStore, StoredMessage};
use uuid::Uuid;

use crate::runtime::usage::apply_llm_usage_dual_write;
use crate::runtime::{AgentConfig, AgentLoop, TurnResult};

use super::headless::run_headless_multi_turn;

/// 进程退出后孤儿 run 的失败文案；会话事后补全时可据此升级为 success。
const CRON_INTERRUPTED_BY_EXIT: &str = "应用退出，执行中断";

static ACTIVE_CRON_RUNS: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

fn mark_active_cron_run(run_id: &str) {
    if let Ok(mut g) = ACTIVE_CRON_RUNS.lock() {
        g.insert(run_id.to_string());
    }
}

fn is_active_cron_run(run_id: &str) -> bool {
    ACTIVE_CRON_RUNS
        .lock()
        .map(|g| g.contains(run_id))
        .unwrap_or(false)
}

/// 作用域结束时从活跃表移除（panic / 正常返回均会清理）。
struct ActiveCronRunGuard(String);

impl ActiveCronRunGuard {
    fn acquire(run_id: &str) -> Self {
        mark_active_cron_run(run_id);
        Self(run_id.to_string())
    }
}

impl Drop for ActiveCronRunGuard {
    fn drop(&mut self) {
        if let Ok(mut g) = ACTIVE_CRON_RUNS.lock() {
            g.remove(&self.0);
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SessionRunOutcome {
    Success { output: String },
    Incomplete,
    NoSession,
}

fn assistant_has_tool_calls(m: &StoredMessage) -> bool {
    match &m.tool_calls {
        Some(serde_json::Value::Array(arr)) => !arr.is_empty(),
        Some(_) => true,
        None => false,
    }
}

fn outcome_from_messages(msgs: &[StoredMessage]) -> SessionRunOutcome {
    for m in msgs.iter().rev() {
        match m.role.as_str() {
            "tool" => continue,
            "assistant" => {
                if assistant_has_tool_calls(m) {
                    return SessionRunOutcome::Incomplete;
                }
                let text = m.content.as_deref().unwrap_or("").trim();
                if text.is_empty() {
                    continue;
                }
                return SessionRunOutcome::Success {
                    output: text.to_string(),
                };
            }
            "user" => return SessionRunOutcome::Incomplete,
            _ => continue,
        }
    }
    SessionRunOutcome::Incomplete
}

fn session_run_outcome(
    sessions: Option<&SessionStore>,
    session_id: Option<&str>,
) -> SessionRunOutcome {
    let Some(sid) = session_id.filter(|s| !s.is_empty()) else {
        return SessionRunOutcome::NoSession;
    };
    let Some(store) = sessions else {
        return SessionRunOutcome::NoSession;
    };
    match store.get_messages(sid) {
        Ok(msgs) => outcome_from_messages(&msgs),
        Err(_) => SessionRunOutcome::NoSession,
    }
}

fn apply_session_outcome(
    db: &CronRunDb,
    run_id: &str,
    outcome: &SessionRunOutcome,
    finished_at: &str,
) -> anyhow::Result<bool> {
    match outcome {
        SessionRunOutcome::Success { output } => {
            let summary = summary_from_output(output);
            db.finish_success(run_id, &summary, output, finished_at)?;
            Ok(true)
        }
        SessionRunOutcome::Incomplete | SessionRunOutcome::NoSession => {
            db.finish_failure(run_id, CRON_INTERRUPTED_BY_EXIT, "", finished_at)?;
            Ok(true)
        }
    }
}

/// 回收本进程未在执行的孤儿 `running` 行，并尝试把「应用退出中断」失败升级为成功。
///
/// 供后端启动、ticker 与前端 list/get 调用；无孤儿时开销很小。
pub fn reconcile_orphaned_runs() -> anyhow::Result<u32> {
    reconcile_orphaned_runs_with_roots(cron_dir(), default_memory_dir())
}

/// 同 [`reconcile_orphaned_runs`]，可指定 cron / memory 根目录（测试用）。
pub fn reconcile_orphaned_runs_with_roots(
    cron_root: impl AsRef<Path>,
    memory_dir: impl AsRef<Path>,
) -> anyhow::Result<u32> {
    let db = CronRunDb::new(cron_db_path(cron_root.as_ref()))?;
    let sessions =
        SessionStore::open_sessions_dir(&memory_dir.as_ref().join("sessions")).ok();
    let finished_at = now_rfc3339();
    let mut changed = 0u32;

    for row in db.list_running()? {
        if is_active_cron_run(&row.id) {
            continue;
        }
        let outcome = session_run_outcome(sessions.as_ref(), row.session_id.as_deref());
        if apply_session_outcome(&db, &row.id, &outcome, &finished_at)? {
            changed += 1;
        }
    }

    // 启动时已标成「应用退出中断」的记录：用户在聊天里重新生成出结果后升级为成功。
    for row in db.list_failure_with_error(CRON_INTERRUPTED_BY_EXIT)? {
        if let SessionRunOutcome::Success { output } =
            session_run_outcome(sessions.as_ref(), row.session_id.as_deref())
        {
            let summary = summary_from_output(&output);
            db.finish_success(&row.id, &summary, &output, &finished_at)?;
            changed += 1;
        }
    }

    Ok(changed)
}

/// 执行定时任务所需的 LLM 凭据与路由信息。
///
/// 字段允许部分为空字符串，执行路径会在 Provider 层回落到默认 model 或内置 base URL。
/// `targets` 为空时由 primary 四字段合成单目标；非空时走 chat fallback 链。
#[derive(Clone)]
pub struct CronExecCredentials {
    /// Provider 注册名（如 `openai`）；空白时 `run_provider_loop` 使用 `openai`。
    pub provider: String,
    /// 模型 id；空白时使用对应 Provider 的 `default_model()`。
    pub model: String,
    /// API Key；空白时任务记为失败且不调用模型。
    pub api_key: String,
    /// 自定义 API 基址；空白时由 Provider 默认配置决定。
    pub base_url: String,
    /// 含 primary 的聊天目标链；空则从四字段合成。
    pub targets: Vec<ChatTarget>,
}

impl CronExecCredentials {
    /// 有效聊天目标：优先 `targets`，否则由四字段合成单元素链。
    fn effective_targets(&self, registry: &ProviderRegistry) -> Vec<ChatTarget> {
        if !self.targets.is_empty() {
            return self.targets.clone();
        }
        let backend = if self.provider.trim().is_empty() {
            "openai".to_string()
        } else {
            self.provider.clone()
        };
        let model = if self.model.trim().is_empty() {
            registry
                .get(&backend)
                .map(|p| p.default_model().to_string())
                .unwrap_or_default()
        } else {
            self.model.clone()
        };
        vec![ChatTarget {
            provider_id: backend.clone(),
            backend_id: backend,
            model,
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone(),
        }]
    }
}

/// 使用默认 cron 数据根目录执行一条定时任务。
///
/// 等价于 `execute_job_with_roots(cron_dir(), ...)`，供 CLI 或调度器在标准布局下调用。
///
/// # 参数
///
/// - `job`：待执行的 [`CronJob`] 定义（含任务文案、agent_id、是否展示到聊天等）。
/// - `creds`：LLM 访问凭据。
/// - `trigger`：触发来源标签（如 `schedule`、`manual`），写入运行记录。
///
/// # 返回
///
/// 执行结束后的 [`cron::CronRunRow`]（含最终状态、摘要与输出）。
///
/// # 错误
///
/// - 同一 `job.id` 已有 `running` 记录时返回「job already running」。
/// - 数据库读写失败、Agent/Provider 执行失败或超时后仍会在库中标记失败，并返回对应 `Err`
///   或包含失败状态的行（API Key 缺失场景返回已落库的失败行）。
pub async fn execute_job(
    job: &CronJob,
    creds: CronExecCredentials,
    trigger: &str,
) -> anyhow::Result<cron::CronRunRow> {
    execute_job_with_roots(cron_dir(), job, creds, trigger).await
}

/// 在指定 cron 根目录下执行定时任务并完整记录生命周期。
///
/// 流程：检查运行互斥 → 可选创建聊天会话 → 插入 `running` 行 → 校验 API Key →
/// 带 600s 超时的 Agent 执行 → 更新为 success/failure。
///
/// # 参数
///
/// - `cron_root`：cron  SQLite 与元数据所在根目录。
/// - `job`、`creds`、`trigger`：同 [`execute_job`](execute_job)。
///
/// # 返回
///
/// 终态 [`cron::CronRunRow`]；成功时 `summary` 由输出首行截取，完整输出存入对应字段。
///
/// # 错误
///
/// - 并发冲突：`job already running`。
/// - 执行期错误：Agent/Provider 返回的 `Err` 会写入 `finish_failure` 后仍尝试 `get` 该行。
/// - 超时：600 秒内未完成则记为「执行超时（600s）」。
/// - 记录异常：插入后无法 `get` 同一 `run_id` 时返回 `cron run vanished`。
///
/// # Send
///
/// `AgentLoop`/`SessionStore` 含 rusqlite `RefCell`，内部 future 非 Send。
/// 本函数经 `spawn_blocking` + `current_thread` runtime 隔离，对外返回 Send future，
/// 可供 Tauri command / 多线程 runtime 直接 `.await`。
pub async fn execute_job_with_roots(
    cron_root: impl AsRef<Path>,
    job: &CronJob,
    creds: CronExecCredentials,
    trigger: &str,
) -> anyhow::Result<cron::CronRunRow> {
    let cron_root = cron_root.as_ref().to_path_buf();
    let job = job.clone();
    let trigger = trigger.to_string();
    tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| anyhow::anyhow!("cron runtime: {e}"))?;
        rt.block_on(execute_job_with_roots_local(
            &cron_root, &job, creds, &trigger,
        ))
    })
    .await
    .map_err(|e| anyhow::anyhow!("cron join: {e}"))?
}

/// 非 Send 的实际执行体；仅在 `current_thread` runtime / LocalSet 内调用。
async fn execute_job_with_roots_local(
    cron_root: &Path,
    job: &CronJob,
    creds: CronExecCredentials,
    trigger: &str,
) -> anyhow::Result<cron::CronRunRow> {
    let db = CronRunDb::new(cron_db_path(cron_root))?;

    if db.has_running_for_job(&job.id)? {
        anyhow::bail!("job already running");
    }

    let session_id = Some(Uuid::new_v4().to_string());

    // 与 run_agent_job / AgentLoop 共用同一 memory_dir 下的 SessionStore。
    // show_in_chat 仅影响侧栏展示；执行记录 / Tracing 始终需要 session。
    let memory_dir = default_memory_dir();
    let sessions = SessionStore::open_sessions_dir(&memory_dir.join("sessions")).ok();

    if let Some(ref sid) = session_id {
        let summary = format!("定时任务 · {}", job.title);
        if let Some(ref store) = sessions {
            let _ = store.ensure_session(sid, "cron");
            let _ = store.set_session_title(sid, &summary);
        }
    }

    let fired_at = now_rfc3339();
    let agent_id = cron::normalize_cron_agent_id(&job.agent_id);
    let run_id = db.insert_running(NewCronRun {
        job_id: job.id.clone(),
        title: job.title.clone(),
        agent_id: agent_id.clone(),
        schedule: job.schedule.clone(),
        task: job.task.clone(),
        fired_at,
        trigger: trigger.to_string(),
        session_id: session_id.clone(),
    })?;
    let _active = ActiveCronRunGuard::acquire(&run_id);

    if creds.api_key.trim().is_empty() {
        db.finish_failure(
            &run_id,
            "未配置 API Key，无法执行定时任务",
            "",
            &now_rfc3339(),
        )?;
        return db
            .get(&run_id)?
            .ok_or_else(|| anyhow::anyhow!("cron run vanished: {run_id}"));
    }

    let model_for_usage = if creds.model.trim().is_empty() {
        "unknown".to_string()
    } else {
        creds.model.clone()
    };
    let billing_provider = if creds.provider.trim().is_empty() {
        None
    } else {
        Some(creds.provider.clone())
    };
    let billing_base_url = if creds.base_url.trim().is_empty() {
        None
    } else {
        Some(creds.base_url.clone())
    };
    let billing_api_key = if creds.api_key.trim().is_empty() {
        None
    } else {
        Some(creds.api_key.clone())
    };

    let exec_result = tokio::time::timeout(
        Duration::from_secs(600),
        run_agent_job(job, creds, session_id.as_deref()),
    )
    .await;

    let mut llm_usage = Usage::default();
    match exec_result {
        Ok(Ok((output, usage))) => {
            llm_usage = usage;
            let summary = summary_from_output(&output);
            db.finish_success(&run_id, &summary, &output, &now_rfc3339())?;
        }
        Ok(Err(err)) => {
            db.finish_failure(&run_id, &err.to_string(), "", &now_rfc3339())?;
        }
        Err(_) => {
            db.finish_failure(&run_id, "执行超时（600s）", "", &now_rfc3339())?;
        }
    }

    let row = db
        .get(&run_id)?
        .ok_or_else(|| anyhow::anyhow!("cron run vanished: {run_id}"))?;

    if row.status == "success" {
        usage::UsageDb::try_record(usage::NewUsageEvent {
            ts: Utc::now().to_rfc3339(),
            kind: "cron".into(),
            name: job.id.clone(),
            agent_id: agent_id.clone(),
            session_id: row.session_id.clone(),
            turn_id: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: Some(
                serde_json::json!({ "title": job.title, "trigger": trigger }).to_string(),
            ),
        });
    }

    // 有真实 usage 时额外记 llm（成功或失败均尽力写，与聊天错误路径一致）
    if !llm_usage.is_empty() {
        apply_llm_usage_dual_write(
            &agent_id,
            row.session_id.as_deref(),
            None,
            &model_for_usage,
            &llm_usage,
            billing_provider.as_deref().unwrap_or(""),
            billing_base_url.as_deref().unwrap_or(""),
            billing_api_key.as_deref().unwrap_or(""),
            Some(
                serde_json::json!({ "source": "cron", "job_id": job.id, "trigger": trigger })
                    .to_string(),
            ),
            sessions.as_ref(),
        );
    }

    Ok(row)
}

/// 插入 `running` 行后立即返回，并在后台跑完 Agent（供 UI「立即执行」边跑边看）。
///
/// 调度器到期触发仍应使用 [`execute_job`]（同步等到终态）。
pub async fn spawn_job(
    job: &CronJob,
    creds: CronExecCredentials,
    trigger: &str,
) -> anyhow::Result<cron::CronRunRow> {
    let cron_root = cron_dir().to_path_buf();
    let job = job.clone();
    let trigger = trigger.to_string();
    let creds_bg = creds.clone();
    let job_bg = job.clone();
    let trigger_bg = trigger.clone();
    let cron_root_bg = cron_root.clone();

    let row = tokio::task::spawn_blocking(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| anyhow::anyhow!("cron runtime: {e}"))?;
        rt.block_on(begin_job_local(&cron_root, &job, &trigger))
    })
    .await
    .map_err(|e| anyhow::anyhow!("cron join: {e}"))??;

    let run_id = row.id.clone();
    let session_id = row.session_id.clone();
    tokio::task::spawn(async move {
        let result = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|e| anyhow::anyhow!("cron runtime: {e}"))?;
            rt.block_on(complete_job_local(
                &cron_root_bg,
                &job_bg,
                creds_bg,
                &trigger_bg,
                &run_id,
                session_id,
            ))
        })
        .await;
        match result {
            Err(e) => tracing::error!(error = %e, "cron spawn_job background join failed"),
            Ok(Err(e)) => tracing::error!(error = %e, "cron spawn_job background failed"),
            Ok(Ok(_)) => {}
        }
    });

    Ok(row)
}

/// 仅创建 running 行与 session（不跑 Agent）。
async fn begin_job_local(
    cron_root: &Path,
    job: &CronJob,
    trigger: &str,
) -> anyhow::Result<cron::CronRunRow> {
    let db = CronRunDb::new(cron_db_path(cron_root))?;

    if db.has_running_for_job(&job.id)? {
        anyhow::bail!("job already running");
    }

    let session_id = Some(Uuid::new_v4().to_string());
    let memory_dir = default_memory_dir();
    let sessions = SessionStore::open_sessions_dir(&memory_dir.join("sessions")).ok();
    if let Some(ref sid) = session_id {
        let summary = format!("定时任务 · {}", job.title);
        if let Some(ref store) = sessions {
            let _ = store.ensure_session(sid, "cron");
            let _ = store.set_session_title(sid, &summary);
        }
    }

    let fired_at = now_rfc3339();
    let agent_id = cron::normalize_cron_agent_id(&job.agent_id);
    let run_id = db.insert_running(NewCronRun {
        job_id: job.id.clone(),
        title: job.title.clone(),
        agent_id: agent_id.clone(),
        schedule: job.schedule.clone(),
        task: job.task.clone(),
        fired_at,
        trigger: trigger.to_string(),
        session_id: session_id.clone(),
    })?;
    // 在 complete 接手前先占住，避免 list/get 把刚插入的行当孤儿回收。
    mark_active_cron_run(&run_id);

    db.get(&run_id)?
        .ok_or_else(|| anyhow::anyhow!("cron run vanished: {run_id}"))
}

/// 对已存在的 running 行执行 Agent 并 `finish_*`。
async fn complete_job_local(
    cron_root: &Path,
    job: &CronJob,
    creds: CronExecCredentials,
    trigger: &str,
    run_id: &str,
    session_id: Option<String>,
) -> anyhow::Result<cron::CronRunRow> {
    let _active = ActiveCronRunGuard::acquire(run_id);
    let db = CronRunDb::new(cron_db_path(cron_root))?;
    let memory_dir = default_memory_dir();
    let sessions = SessionStore::open_sessions_dir(&memory_dir.join("sessions")).ok();
    let agent_id = cron::normalize_cron_agent_id(&job.agent_id);

    if creds.api_key.trim().is_empty() {
        db.finish_failure(
            run_id,
            "未配置 API Key，无法执行定时任务",
            "",
            &now_rfc3339(),
        )?;
        return db
            .get(run_id)?
            .ok_or_else(|| anyhow::anyhow!("cron run vanished: {run_id}"));
    }

    let model_for_usage = if creds.model.trim().is_empty() {
        "unknown".to_string()
    } else {
        creds.model.clone()
    };
    let billing_provider = if creds.provider.trim().is_empty() {
        None
    } else {
        Some(creds.provider.clone())
    };
    let billing_base_url = if creds.base_url.trim().is_empty() {
        None
    } else {
        Some(creds.base_url.clone())
    };
    let billing_api_key = if creds.api_key.trim().is_empty() {
        None
    } else {
        Some(creds.api_key.clone())
    };

    let exec_result = tokio::time::timeout(
        Duration::from_secs(600),
        run_agent_job(job, creds, session_id.as_deref()),
    )
    .await;

    let mut llm_usage = Usage::default();
    match exec_result {
        Ok(Ok((output, usage))) => {
            llm_usage = usage;
            let summary = summary_from_output(&output);
            db.finish_success(run_id, &summary, &output, &now_rfc3339())?;
        }
        Ok(Err(err)) => {
            db.finish_failure(run_id, &err.to_string(), "", &now_rfc3339())?;
        }
        Err(_) => {
            db.finish_failure(run_id, "执行超时（600s）", "", &now_rfc3339())?;
        }
    }

    let row = db
        .get(run_id)?
        .ok_or_else(|| anyhow::anyhow!("cron run vanished: {run_id}"))?;

    if row.status == "success" {
        usage::UsageDb::try_record(usage::NewUsageEvent {
            ts: Utc::now().to_rfc3339(),
            kind: "cron".into(),
            name: job.id.clone(),
            agent_id: agent_id.clone(),
            session_id: row.session_id.clone(),
            turn_id: None,
            input_tokens: 0,
            output_tokens: 0,
            cache_read_tokens: 0,
            cache_write_tokens: 0,
            reasoning_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            cost_status: None,
            cost_source: None,
            pricing_version: None,
            billing_provider: None,
            billing_base_url: None,
            billing_mode: None,
            meta_json: Some(
                serde_json::json!({ "title": job.title, "trigger": trigger }).to_string(),
            ),
        });
    }

    if !llm_usage.is_empty() {
        apply_llm_usage_dual_write(
            &agent_id,
            row.session_id.as_deref(),
            None,
            &model_for_usage,
            &llm_usage,
            billing_provider.as_deref().unwrap_or(""),
            billing_base_url.as_deref().unwrap_or(""),
            billing_api_key.as_deref().unwrap_or(""),
            Some(
                serde_json::json!({ "source": "cron", "job_id": job.id, "trigger": trigger })
                    .to_string(),
            ),
            sessions.as_ref(),
        );
    }

    Ok(row)
}

/// 以 Agent 主循环执行 `job.task`，通过 [`run_headless_multi_turn`] 驱动完整工具循环。
///
/// 按 `job.agent_id` 绑定已有 Agent 工作区（SOUL / MEMORY / 工具门控 / MCP），
/// 不依赖全局活跃 Agent。`session_id` 为 `None` 时为本次执行生成临时 UUID，
/// 不关联聊天 UI 会话。
async fn run_agent_job(
    job: &CronJob,
    creds: CronExecCredentials,
    session_id: Option<&str>,
) -> anyhow::Result<(String, Usage)> {
    let memory_dir = default_memory_dir();
    let sid = session_id
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());
    let agent_id = cron::normalize_cron_agent_id(&job.agent_id);

    let mut config = AgentConfig::with_defaults(memory_dir.clone());
    // with_defaults 读的是活跃 agent 的 SOUL；覆盖为任务指定 agent。
    let ws = home::agent_workspace_dir(&memory_dir, &agent_id);
    if let Ok(soul) = std::fs::read_to_string(ws.join("SOUL.md")) {
        config.soul = soul;
    }

    let mut agent = AgentLoop::with_session_id_for_agent(config, sid, &agent_id)?;
    agent.set_chat_credentials(
        &creds.provider,
        &creds.model,
        &creds.api_key,
        &creds.base_url,
    );
    let registry = ProviderRegistry::new();
    let targets = creds.effective_targets(&registry);
    agent.set_chat_targets(targets.clone());

    let system_prompt = match agent.run_turn(&job.task, "cron").await? {
        TurnResult::Finished(message) => return Ok((message, Usage::default())),
        TurnResult::Continue { system_prompt, .. } => system_prompt,
        TurnResult::BudgetExhausted => anyhow::bail!("对话轮次预算已用尽"),
        TurnResult::MaxDepth => anyhow::bail!("工具调用轮次已达上限"),
        TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
            anyhow::bail!("定时任务不支持该轮次结果")
        }
    };

    run_headless_multi_turn(&mut agent, targets, system_prompt).await
}

/// 返回当前 UTC 时间的 RFC3339 字符串（秒精度，含时区偏移）。
///
/// 用于 cron 运行记录的 `fired_at` 与完成时间戳。
fn now_rfc3339() -> String {
    Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 从模型完整输出中提取一行摘要，供运行记录列表展示。
///
/// 取首个非空行并截断至 200 个 Unicode 标量；若无非空行则对全文截断。
fn summary_from_output(output: &str) -> String {
    output
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(output)
        .chars()
        .take(200)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use session::NewMessage;
    use tempfile::TempDir;

    fn stored(
        role: &str,
        content: Option<&str>,
        tool_calls: Option<serde_json::Value>,
    ) -> StoredMessage {
        StoredMessage {
            id: 0,
            session_id: "s".into(),
            role: role.into(),
            content: content.map(str::to_string),
            compressed_content: None,
            tool_call_id: None,
            tool_calls,
            tool_name: None,
            timestamp: 0.0,
            token_count: None,
            finish_reason: None,
            reasoning: None,
            reasoning_content: None,
            reasoning_details: None,
            codex_reasoning_items: None,
            codex_message_items: None,
            media_json: None,
        }
    }

    #[test]
    fn outcome_prefers_final_assistant_without_tools() {
        let msgs = vec![
            stored("user", Some("task"), None),
            stored(
                "assistant",
                Some("calling"),
                Some(json!([{"id": "1", "function": {"name": "x"}}])),
            ),
            stored("tool", Some("ok"), None),
            stored("assistant", Some("最终报告"), None),
        ];
        assert_eq!(
            outcome_from_messages(&msgs),
            SessionRunOutcome::Success {
                output: "最终报告".into()
            }
        );
    }

    #[test]
    fn outcome_incomplete_when_last_assistant_has_tools() {
        let msgs = vec![
            stored("user", Some("task"), None),
            stored(
                "assistant",
                Some(""),
                Some(json!([{"id": "1"}])),
            ),
        ];
        assert_eq!(outcome_from_messages(&msgs), SessionRunOutcome::Incomplete);
    }

    #[test]
    fn reconcile_marks_orphan_running_from_session_or_interrupt() {
        let cron_dir = TempDir::new().unwrap();
        let mem_dir = TempDir::new().unwrap();
        let sessions_dir = mem_dir.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let store = SessionStore::open_sessions_dir(&sessions_dir).unwrap();
        store.ensure_session("sess-ok", "cron").unwrap();
        store
            .append_message(NewMessage {
                session_id: "sess-ok",
                role: "user",
                content: Some("do it"),
                ..NewMessage::empty("sess-ok", "user")
            })
            .unwrap();
        store
            .append_message(NewMessage {
                session_id: "sess-ok",
                role: "assistant",
                content: Some("晨报完成"),
                ..NewMessage::empty("sess-ok", "assistant")
            })
            .unwrap();

        let db = CronRunDb::new(cron_dir.path().join("cron.db")).unwrap();
        let ok_id = db
            .insert_running(NewCronRun {
                job_id: "j1".into(),
                title: "ok".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "t".into(),
                fired_at: "2026-07-20T10:00:00+08:00".into(),
                trigger: "manual".into(),
                session_id: Some("sess-ok".into()),
            })
            .unwrap();
        let bare_id = db
            .insert_running(NewCronRun {
                job_id: "j2".into(),
                title: "bare".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "t".into(),
                fired_at: "2026-07-20T10:01:00+08:00".into(),
                trigger: "manual".into(),
                session_id: None,
            })
            .unwrap();

        let n = reconcile_orphaned_runs_with_roots(cron_dir.path(), mem_dir.path()).unwrap();
        assert_eq!(n, 2);
        let ok = db.get(&ok_id).unwrap().unwrap();
        assert_eq!(ok.status, "success");
        assert!(ok.output.contains("晨报完成"));
        let bare = db.get(&bare_id).unwrap().unwrap();
        assert_eq!(bare.status, "failure");
        assert_eq!(bare.error.as_deref(), Some(CRON_INTERRUPTED_BY_EXIT));
    }

    #[test]
    fn reconcile_upgrades_interrupted_after_session_completes() {
        let cron_dir = TempDir::new().unwrap();
        let mem_dir = TempDir::new().unwrap();
        let sessions_dir = mem_dir.path().join("sessions");
        std::fs::create_dir_all(&sessions_dir).unwrap();
        let store = SessionStore::open_sessions_dir(&sessions_dir).unwrap();
        store.ensure_session("sess-later", "cron").unwrap();

        let db = CronRunDb::new(cron_dir.path().join("cron.db")).unwrap();
        let id = db
            .insert_running(NewCronRun {
                job_id: "j".into(),
                title: "t".into(),
                agent_id: "workspace".into(),
                schedule: "every:1d".into(),
                task: "task".into(),
                fired_at: "2026-07-20T10:00:00+08:00".into(),
                trigger: "manual".into(),
                session_id: Some("sess-later".into()),
            })
            .unwrap();
        db.finish_failure(
            &id,
            CRON_INTERRUPTED_BY_EXIT,
            "",
            "2026-07-20T10:05:00+08:00",
        )
        .unwrap();

        store
            .append_message(NewMessage {
                session_id: "sess-later",
                role: "user",
                content: Some("retry"),
                ..NewMessage::empty("sess-later", "user")
            })
            .unwrap();
        store
            .append_message(NewMessage {
                session_id: "sess-later",
                role: "assistant",
                content: Some("重新生成后的结果"),
                ..NewMessage::empty("sess-later", "assistant")
            })
            .unwrap();

        let n = reconcile_orphaned_runs_with_roots(cron_dir.path(), mem_dir.path()).unwrap();
        assert_eq!(n, 1);
        let row = db.get(&id).unwrap().unwrap();
        assert_eq!(row.status, "success");
        assert!(row.output.contains("重新生成"));
    }
}
