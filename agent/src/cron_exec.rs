//! 定时任务（Cron）执行器：将 [`CronJob`] 派发给 Agent 或 Provider 并完成运行记录落库。
//!
//! 负责并发互斥（同一 job 不重叠执行）、可选会话创建、600 秒超时兜底，以及成功/失败
//! 状态写入 `CronRunDb`。任务文案经 [`AgentLoop::run_turn`] 进入主循环；若需继续多轮
//! 工具调用则回落到 [`run_provider_loop`] 直接与 LLM Provider 交互（最多 5 轮）。

use std::path::Path;
use std::time::Duration;

use chrono::Utc;
use futures::StreamExt;
use memory::{
    cron_db_path, cron_dir, default_memory_dir, CronJob, CronRunDb, MemoryManager, NewCronRun,
};
use providers::registry::ProviderRegistry;
use providers::streaming::Usage;
use providers::trait_::ProviderConfig;
use uuid::Uuid;

use crate::loop_::{AgentConfig, AgentLoop, TurnResult};
use crate::messages::to_provider_messages;

/// 执行定时任务所需的 LLM 凭据与路由信息。
///
/// 字段允许部分为空字符串，执行路径会在 Provider 层回落到默认 model 或内置 base URL。
pub struct CronExecCredentials {
    /// Provider 注册名（如 `openai`）；空白时 `run_provider_loop` 使用 `openai`。
    pub provider: String,
    /// 模型 id；空白时使用对应 Provider 的 `default_model()`。
    pub model: String,
    /// API Key；空白时任务记为失败且不调用模型。
    pub api_key: String,
    /// 自定义 API 基址；空白时由 Provider 默认配置决定。
    pub base_url: String,
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
/// 执行结束后的 [`memory::CronRunRow`]（含最终状态、摘要与输出）。
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
) -> anyhow::Result<memory::CronRunRow> {
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
/// 终态 [`memory::CronRunRow`]；成功时 `summary` 由输出首行截取，完整输出存入对应字段。
///
/// # 错误
///
/// - 并发冲突：`job already running`。
/// - 执行期错误：Agent/Provider 返回的 `Err` 会写入 `finish_failure` 后仍尝试 `get` 该行。
/// - 超时：600 秒内未完成则记为「执行超时（600s）」。
/// - 记录异常：插入后无法 `get` 同一 `run_id` 时返回 `cron run vanished`。
pub async fn execute_job_with_roots(
    cron_root: impl AsRef<Path>,
    job: &CronJob,
    creds: CronExecCredentials,
    trigger: &str,
) -> anyhow::Result<memory::CronRunRow> {
    let cron_root = cron_root.as_ref();
    let db = CronRunDb::new(cron_db_path(cron_root))?;

    if db.has_running_for_job(&job.id)? {
        anyhow::bail!("job already running");
    }

    let session_id = if job.show_in_chat {
        Some(Uuid::new_v4().to_string())
    } else {
        None
    };

    if let Some(ref sid) = session_id {
        let summary = format!("定时任务 · {}", job.title);
        if let Ok(mgr) = MemoryManager::new(default_memory_dir()) {
            let _ = mgr.ensure_session(sid, "cron");
            let _ = mgr.session_store.set_session_title(sid, &summary);
        }
    }

    let fired_at = now_rfc3339();
    let run_id = db.insert_running(NewCronRun {
        job_id: job.id.clone(),
        title: job.title.clone(),
        agent_id: job.agent_id.clone(),
        schedule: job.schedule.clone(),
        task: job.task.clone(),
        fired_at,
        trigger: trigger.to_string(),
        session_id: session_id.clone(),
    })?;

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
        memory::UsageDb::try_record(memory::NewUsageEvent {
            ts: Utc::now().to_rfc3339(),
            kind: "cron".into(),
            name: job.id.clone(),
            agent_id: job.agent_id.clone(),
            session_id: row.session_id.clone(),
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            cost_usd: 0.0,
            meta_json: Some(
                serde_json::json!({ "title": job.title, "trigger": trigger }).to_string(),
            ),
        });
    }

    // 有真实 usage 时额外记 llm（成功或失败均尽力写，与聊天错误路径一致）
    if !llm_usage.is_empty() {
        let cost = memory::estimate_llm_cost(
            &model_for_usage,
            llm_usage.prompt_tokens(),
            llm_usage.completion_tokens(),
        );
        memory::UsageDb::try_record(memory::NewUsageEvent {
            ts: Utc::now().to_rfc3339(),
            kind: "llm".into(),
            name: model_for_usage,
            agent_id: job.agent_id.clone(),
            session_id: row.session_id.clone(),
            prompt_tokens: i64::from(llm_usage.prompt_tokens()),
            completion_tokens: i64::from(llm_usage.completion_tokens()),
            total_tokens: i64::from(llm_usage.total_tokens()),
            cost_usd: cost,
            meta_json: Some(
                serde_json::json!({ "source": "cron", "job_id": job.id, "trigger": trigger })
                    .to_string(),
            ),
        });
    }

    Ok(row)
}

/// 以 Agent 主循环执行 `job.task`，必要时进入 Provider 工具多轮循环。
///
/// `session_id` 为 `None` 时为本次执行生成临时 UUID，不关联聊天 UI 会话。
///
/// # 错误
///
/// 预算耗尽、工具深度超限，或出现定时任务不支持的 [`TurnResult`] 变体时返回 `Err`。
async fn run_agent_job(
    job: &CronJob,
    creds: CronExecCredentials,
    session_id: Option<&str>,
) -> anyhow::Result<(String, Usage)> {
    let memory_dir = default_memory_dir();
    let sid = session_id
        .map(str::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let config = AgentConfig::with_defaults(memory_dir);
    let mut agent = AgentLoop::with_session_id(config, sid)?;
    agent.set_chat_credentials(
        &creds.provider,
        &creds.model,
        &creds.api_key,
        &creds.base_url,
    );

    let turn_result = agent.run_turn(&job.task, "cron").await?;
    match turn_result {
        TurnResult::Finished(message) => return Ok((message, Usage::default())),
        TurnResult::Continue { system_prompt, .. } => {
            run_provider_loop(&mut agent, &creds, &system_prompt).await
        }
        TurnResult::BudgetExhausted => anyhow::bail!("对话轮次预算已用尽"),
        TurnResult::MaxDepth => anyhow::bail!("工具调用轮次已达上限"),
        TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
            anyhow::bail!("定时任务不支持该轮次结果")
        }
    }
}

/// 直接与 LLM Provider 进行最多 5 轮流式对话，并在模型返回工具调用时同步执行工具。
///
/// 每轮重新加载 MCP/工具注册表，将 `system_prompt` 与会话消息转为 Provider 格式后
/// `chat_stream`；无工具调用则返回最终助手文本。
///
/// # 错误
///
/// 未知 Provider、流式错误、轮次预算问题，或 5 轮后仍无有效回复时返回 `Err`。
async fn run_provider_loop(
    agent: &mut AgentLoop,
    creds: &CronExecCredentials,
    initial_system_prompt: &str,
) -> anyhow::Result<(String, Usage)> {
    let providers = ProviderRegistry::new();
    let provider_name = if creds.provider.trim().is_empty() {
        "openai".to_string()
    } else {
        creds.provider.clone()
    };

    let provider = providers
        .get(&provider_name)
        .ok_or_else(|| anyhow::anyhow!("未知 Provider: {provider_name}"))?;

    let config = ProviderConfig {
        model: if creds.model.trim().is_empty() {
            provider.default_model().to_string()
        } else {
            creds.model.clone()
        },
        api_key: creds.api_key.clone(),
        base_url: if creds.base_url.trim().is_empty() {
            None
        } else {
            Some(creds.base_url.clone())
        },
        ..ProviderConfig::default()
    };

    let mut system_prompt = initial_system_prompt.to_string();
    let mut last_response = String::new();
    let mut total_usage = Usage::default();

    for _round in 0..5 {
        agent.reload_tools_and_mcp().await;
        let messages = to_provider_messages(&system_prompt, &agent.session_messages);
        let tools = agent.tool_registry().schemas_for_api();

        let mut stream = provider
            .chat_stream(messages, tools, &config)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let mut full_response = String::new();
        // 与 streaming.rs 一致：同轮内覆盖取最后一次 usage，跨轮累加
        let mut round_usage: Option<Usage> = None;
        while let Some(chunk_result) = stream.next().await {
            let chunk = chunk_result.map_err(|e| anyhow::anyhow!("{e}"))?;
            if let Some(token) = chunk.token {
                full_response.push_str(&token);
            }
            if let Some(u) = chunk.usage {
                round_usage = Some(u);
            }
        }
        if let Some(u) = round_usage {
            total_usage.add_assign(u);
        }

        if full_response.is_empty() {
            break;
        }

        last_response = full_response.clone();
        let calls = tools::extract_tool_calls(&full_response);
        let tc = if calls.is_empty() {
            None
        } else {
            Some(
                calls
                    .iter()
                    .map(|c| common::message::ToolCall {
                        id: c.id.clone(),
                        name: c.name.clone(),
                        arguments: c.arguments.clone(),
                    })
                    .collect(),
            )
        };
        agent.record_assistant_message_with_tools(&full_response, tc, None, None)?;

        if calls.is_empty() {
            return Ok((last_response, total_usage));
        }

        for call in calls {
            let result = tokio::task::block_in_place(|| {
                agent.handle_tool_call(&call.name, &call.arguments)
            })
            .unwrap_or_else(|e| format!("工具错误: {e}"));

            agent.record_tool_result_with_id(
                Some(&call.id),
                Some(&call.name),
                &format!(
                    "tool={} args={} result={}",
                    call.name, call.arguments, result
                ),
            )?;
        }

        let turn_result = agent
            .run_turn("", "cron-tool-followup")
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        system_prompt = match turn_result {
            TurnResult::Continue { system_prompt, .. } => system_prompt,
            TurnResult::Finished(message) => return Ok((message, total_usage)),
            TurnResult::BudgetExhausted => anyhow::bail!("对话轮次预算已用尽"),
            TurnResult::MaxDepth => anyhow::bail!("工具调用轮次已达上限"),
            TurnResult::ToolCalls(_) | TurnResult::Interrupted => {
                anyhow::bail!("定时任务不支持该轮次结果")
            }
        };
    }

    if last_response.is_empty() {
        anyhow::bail!("模型未返回有效回复");
    }
    Ok((last_response, total_usage))
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
