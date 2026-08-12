//! 工作流定时触发：每 30s 扫描启用的 ScheduledTrigger 工作流，到期时执行。

use chrono::Local;
use std::collections::HashMap;
use std::path::PathBuf;

use workflow::model::NodeType;
use workflow::run_db::WorkflowRunDb;
use workflow::store::WorkflowStore;

/// 调度状态：追踪每个工作流的下次执行时间
static NEXT_RUN: std::sync::Mutex<Option<HashMap<String, chrono::DateTime<Local>>>> =
    std::sync::Mutex::new(None);

fn state_file() -> PathBuf {
    home::default_memory_dir()
        .join("workflows")
        .join("schedule_state.json")
}

fn load_state() -> HashMap<String, String> {
    std::fs::read_to_string(state_file())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_state(state: &HashMap<String, String>) {
    if let Ok(json) = serde_json::to_string_pretty(state) {
        let _ = std::fs::write(state_file(), json);
    }
}

/// 每 30s 由 ticker 线程调用
pub async fn tick_workflows() {
    if let Err(e) = tick_inner().await {
        tracing::warn!(error = %e, "workflow ticker error");
    }
}

async fn tick_inner() -> anyhow::Result<()> {
    let store = WorkflowStore::open_default()?;
    let workflows = store.list()?;
    let now = Local::now();

    let mut persisted = load_state();
    let mut changed = false;

    for wf in &workflows {
        if !wf.enabled {
            continue;
        }

        // 找到第一个启用的 ScheduledTrigger 节点
        let trigger_node = wf
            .nodes
            .iter()
            .find(|n| !n.disabled && n.node_type == NodeType::ScheduledTrigger);
        let trigger_node = match trigger_node {
            Some(n) => n,
            None => continue,
        };

        let schedule = trigger_node
            .config
            .get("schedule")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if schedule.is_empty() {
            continue;
        }

        // 获取或计算下次运行时间
        let next_run = {
            let mut guard = NEXT_RUN.lock().unwrap_or_else(|e| e.into_inner());
            let map = guard.get_or_insert_with(|| {
                let mut m = HashMap::new();
                for (id, time_str) in &persisted {
                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(time_str) {
                        m.insert(id.clone(), dt.with_timezone(&Local));
                    }
                }
                m
            });

            if let Some(dt) = map.get(&wf.id) {
                *dt
            } else {
                let dt = cron::compute_next_run(schedule, now)?;
                map.insert(wf.id.clone(), dt);
                persisted.insert(wf.id.clone(), dt.to_rfc3339());
                changed = true;
                dt
            }
        };

        if now < next_run {
            continue;
        }

        // 到期！执行工作流
        tracing::info!(
            workflow = %wf.name,
            schedule = %schedule,
            "定时触发工作流"
        );

        let wf_clone = wf.clone();
        let schedule_clone = schedule.to_string();
        let wf_id = wf.id.clone();

        // spawn_blocking 隔离 rusqlite
        let result = tokio::task::spawn_blocking(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()?;
            rt.block_on(async {
                let run_db = WorkflowRunDb::open_default()?;
                workflow::engine::execute_workflow(
                    &wf_clone,
                    serde_json::json!({ "trigger": "scheduled", "schedule": schedule_clone }),
                    "scheduled",
                    &run_db,
                )
                .await
            })
        })
        .await;

        match result {
            Ok(Ok(run_result)) => {
                tracing::info!(
                    workflow = %wf.name,
                    run_id = %run_result.run_id,
                    steps = run_result.steps_executed,
                    "定时工作流执行完成"
                );
            }
            Ok(Err(e)) => {
                tracing::error!(workflow = %wf.name, error = %e, "定时工作流执行失败");
            }
            Err(e) => {
                tracing::error!(workflow = %wf.name, error = %e, "定时工作流执行 panic");
            }
        }

        // 计算下一次运行时间
        let next = cron::compute_next_run(schedule, now)?;
        {
            let mut guard = NEXT_RUN.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(map) = guard.as_mut() {
                map.insert(wf_id.clone(), next);
            }
        }
        persisted.insert(wf_id, next.to_rfc3339());
        changed = true;
    }

    if changed {
        save_state(&persisted);
    }

    Ok(())
}
