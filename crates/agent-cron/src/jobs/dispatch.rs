//! Agent 工具分发入口：单一 `cron` 工具按 `action` 分支。

use super::store::CronStore;

/// Agent 工具入口：根据 `args.action` 分发 add / list / remove / enable / disable。
pub fn dispatch_cron_tool(args: &serde_json::Value) -> anyhow::Result<String> {
    let action = args
        .get("action")
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("缺少 action（add|list|remove|enable|disable）"))?
        .to_ascii_lowercase();

    let store = CronStore::open_default()?;

    match action.as_str() {
        "add" => {
            let schedule = args
                .get("schedule")
                .or_else(|| args.get("cron"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("add 需要 schedule 或 cron"))?;
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("add 需要 task"))?;
            let job = store.add(schedule, task)?;
            Ok(format!(
                "已创建定时任务 {}\n调度: {}\n下次: {}\n任务: {}",
                &job.id[..8],
                job.schedule,
                job.next_run_at.as_deref().unwrap_or("-"),
                job.task
            ))
        }
        "list" => {
            let jobs = store.list()?;
            if jobs.is_empty() {
                return Ok("暂无定时任务".into());
            }
            let body = jobs
                .iter()
                .map(|j| {
                    let state = if j.archived_at.is_some() {
                        "archived"
                    } else if j.enabled {
                        "on"
                    } else {
                        "off"
                    };
                    format!(
                        "- [{}] {} | {} | next={} | {}",
                        state,
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
        "remove" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("remove 需要 id"))?;
            if store.remove(id)? {
                Ok(format!("已删除定时任务: {id}"))
            } else {
                Ok(format!("未找到定时任务: {id}"))
            }
        }
        "enable" | "disable" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("{action} 需要 id"))?;
            let enabled = action == "enable";
            if store.set_enabled(id, enabled)? {
                Ok(format!(
                    "已{}定时任务: {id}",
                    if enabled { "启用" } else { "禁用" }
                ))
            } else {
                Ok(format!("未找到定时任务: {id}"))
            }
        }
        other => anyhow::bail!("未知 cron action: {other}（add|list|remove|enable|disable）"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_missing_action() {
        let err = dispatch_cron_tool(&json!({ "task": "x" })).unwrap_err();
        assert!(err.to_string().contains("action"), "{err}");
    }
}
