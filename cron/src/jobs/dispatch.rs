//! Agent 工具分发入口。

use super::store::CronStore;

/// Agent 工具入口：根据 `name` 分发 cron_add / list / remove / enable / disable
pub fn dispatch_cron_tool(name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
    let store = CronStore::open_default()?;
    match name {
        "cron_add" | "scheduled" => {
            let schedule = args
                .get("cron")
                .or_else(|| args.get("schedule"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少 cron/schedule 参数"))?;
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少 task 参数"))?;
            let job = store.add(schedule, task)?;
            Ok(format!(
                "已创建定时任务 {}\n调度: {}\n下次: {}\n任务: {}",
                &job.id[..8],
                job.schedule,
                job.next_run_at.as_deref().unwrap_or("-"),
                job.task
            ))
        }
        "cron_list" => {
            let jobs = store.list()?;
            if jobs.is_empty() {
                return Ok("暂无定时任务".into());
            }
            let body = jobs
                .iter()
                .map(|j| {
                    format!(
                        "- [{}] {} | {} | next={} | {}",
                        if j.enabled { "on" } else { "off" },
                        &j.id[..8],
                        j.schedule,
                        j.next_run_at.as_deref().unwrap_or("-"),
                        j.task
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!("## 定时任务\n{body}"))
        }
        "cron_remove" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少 id 参数"))?;
            if store.remove(id)? {
                Ok(format!("已删除定时任务: {id}"))
            } else {
                Ok(format!("未找到定时任务: {id}"))
            }
        }
        "cron_enable" | "cron_disable" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少 id 参数"))?;
            let enabled = name == "cron_enable";
            if store.set_enabled(id, enabled)? {
                Ok(format!(
                    "已{}定时任务: {id}",
                    if enabled { "启用" } else { "禁用" }
                ))
            } else {
                Ok(format!("未找到定时任务: {id}"))
            }
        }
        _ => anyhow::bail!("未知 cron 工具: {name}"),
    }
}

