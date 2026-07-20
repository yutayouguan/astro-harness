//! 定时任务调度：定义、持久化、到期扫描与工具分发。
//!
//! 职责：
//! - 将任务定义持久化到 `~/.astro/cron/jobs.json`
//! - 解析 `every:` / 五段 cron / `once:` 调度表达式并计算下次运行时间
//! - `claim_due` / `tick` 扫描到期任务并推进 `next_run_at`
//! - 为 Agent 工具与 Extractor 提供自然语言 → 结构化任务的入口
//!
//! 不变量：
//! - 任务 id 为 UUID，持久化前会校验 schedule 可解析
//! - `once:` 任务触发后自动禁用且清空 `next_run_at`
//! - `jobs.json` 通过临时文件原子写入，避免半写损坏

mod dispatch;
mod model;
mod schedule;
mod store;
mod tick;

pub use dispatch::dispatch_cron_tool;
pub use model::*;
pub use schedule::compute_next_run;
pub use store::{cron_dir, CronStore};
pub use tick::tick_default;

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Datelike, Local, Timelike};
    use tempfile::TempDir;

    #[test]
    fn every_schedule_and_tick() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store.add("every:1m", "提醒喝水").unwrap();
        assert!(job.next_run_at.is_some());

        // 强制到期
        let mut file = store.load().unwrap();
        file.jobs[0].next_run_at = Some("2000-01-01T00:00:00+00:00".into());
        store.save(&file).unwrap();

        let fired = store.claim_due().unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].task, "提醒喝水");
        assert!(dir
            .path()
            .join("output")
            .read_dir()
            .unwrap()
            .next()
            .is_none());
        let after = store.list().unwrap();
        assert!(after[0].next_run_at.as_ref().unwrap().as_str() > "2000");
    }

    #[test]
    fn legacy_job_defaults_show_in_chat_false() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("jobs.json");
        std::fs::write(
            &path,
            r#"{
          "jobs": [{
            "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "schedule": "every:5m",
            "task": "提醒喝水\n第二行",
            "enabled": true,
            "created_at": "2026-01-01T00:00:00Z"
          }]
        }"#,
        )
        .unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let jobs = store.list().unwrap();
        assert!(!jobs[0].show_in_chat);
    }

    #[test]
    fn claim_due_advances_schedule_without_output_files() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        store.add("every:1m", "x").unwrap();
        let mut file = store.load().unwrap();
        file.jobs[0].next_run_at = Some("2000-01-01T00:00:00+00:00".into());
        store.save(&file).unwrap();
        let due = store.claim_due().unwrap();
        assert_eq!(due.len(), 1);
        assert!(dir
            .path()
            .join("output")
            .read_dir()
            .unwrap()
            .next()
            .is_none());
        let after = store.list().unwrap();
        assert!(after[0].next_run_at.as_ref().unwrap().as_str() > "2000");
    }

    #[test]
    fn five_field_cron_parses() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 10, 0, 0).unwrap();
        let next = compute_next_run("30 10 * * *", after).unwrap();
        assert_eq!(next.hour(), 10);
        assert_eq!(next.minute(), 30);
    }

    #[test]
    fn remove_by_prefix() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store.add("every:5m", "t").unwrap();
        assert!(store.remove(&job.id[..8]).unwrap());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn legacy_job_json_gets_default_title_and_agent() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("jobs.json");
        std::fs::write(
            &path,
            r#"{
          "jobs": [{
            "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "schedule": "every:5m",
            "task": "提醒喝水\n第二行",
            "enabled": true,
            "created_at": "2026-01-01T00:00:00Z"
          }]
        }"#,
        )
        .unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let jobs = store.list().unwrap();
        assert_eq!(jobs[0].title, "提醒喝水");
        assert_eq!(jobs[0].agent_id, "default");
        assert!(jobs[0].provider_id.is_none());
        assert!(jobs[0].model.is_none());
    }

    #[test]
    fn once_schedule_next_run_is_that_instant() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 4, 0, 0).unwrap();
        let next = compute_next_run("once:2026-07-11T04:22:00+08:00", after).unwrap();
        let expected = DateTime::parse_from_rfc3339("2026-07-11T04:22:00+08:00")
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(next, expected);
    }

    #[test]
    fn tick_disables_once_job() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store
            .add("once:2000-01-01T00:00:00+00:00", "一次性任务")
            .unwrap();
        assert!(job.enabled);
        let fired = store.tick().unwrap();
        assert_eq!(fired.len(), 1);
        let jobs = store.list().unwrap();
        assert!(!jobs[0].enabled);
        assert!(jobs[0].next_run_at.is_none());
        assert!(jobs[0].last_run_at.is_some());
    }

    #[test]
    fn set_enabled_rejects_expired_once_job() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store
            .add("once:2000-01-01T00:00:00+00:00", "已过期单次")
            .unwrap();
        let fired = store.tick().unwrap();
        assert_eq!(fired.len(), 1);
        assert!(!store.list().unwrap()[0].enabled);

        let err = store.set_enabled(&job.id, true).unwrap_err();
        assert!(
            err.to_string().contains("无法启用已过期的单次任务"),
            "unexpected error: {err}"
        );
        assert!(!store.list().unwrap()[0].enabled);
    }

    #[test]
    fn once_in_the_past_errors_or_returns_past_for_tick() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 12, 0, 0, 0).unwrap();
        // 约定：once 时间已过则 compute_next_run 仍返回该时刻（让 tick 能判定 due）
        let next = compute_next_run("once:2026-07-11T04:22:00+08:00", after).unwrap();
        assert!(next < after);
    }

    #[test]
    fn every_with_weekday_filter() {
        use chrono::TimeZone;
        // 2026-07-11 是周六
        let sat = Local.with_ymd_and_hms(2026, 7, 11, 10, 0, 0).unwrap();
        let next = compute_next_run("every:1h;wd=1-5", sat).unwrap();
        // 应跳到下周一附近
        assert_eq!(next.weekday().num_days_from_monday(), 0);
    }

    #[test]
    fn cron_weekday_list() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 3, 0, 0).unwrap(); // Sat
        let next = compute_next_run("22 4 * * 1,2,3,4,5", after).unwrap();
        assert!(next.weekday().num_days_from_monday() < 5);
        assert_eq!(next.hour(), 4);
        assert_eq!(next.minute(), 22);
    }

    #[test]
    fn normalize_cron_extract_fills_title_and_validates() {
        let draft = CronJobExtract {
            schedule: " every:5m ".into(),
            task: " 提醒喝水 ".into(),
            title: None,
        };
        let ok = normalize_cron_extract(draft).unwrap();
        assert_eq!(ok.schedule, "every:5m");
        assert_eq!(ok.task, "提醒喝水");
        assert_eq!(ok.title.as_deref(), Some("提醒喝水"));
    }

    #[test]
    fn normalize_cron_extract_rejects_bad_schedule() {
        let draft = CronJobExtract {
            schedule: "sometime".into(),
            task: "x".into(),
            title: None,
        };
        assert!(normalize_cron_extract(draft).is_err());
    }

    #[test]
    fn five_field_cron_accepts_weekday_range() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 3, 0, 0).unwrap(); // Sat
        let next = compute_next_run("0 9 * * 1-5", after).unwrap();
        assert!(next.weekday().num_days_from_monday() < 5);
        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 0);
    }

    #[test]
    fn normalize_cron_agent_id_maps_legacy_default() {
        assert_eq!(normalize_cron_agent_id(""), "workspace");
        assert_eq!(normalize_cron_agent_id("default"), "workspace");
        assert_eq!(normalize_cron_agent_id("DEFAULT"), "workspace");
        assert_eq!(normalize_cron_agent_id("workspace"), "workspace");
        assert_eq!(normalize_cron_agent_id("Coder"), "coder");
    }

    #[test]
    fn load_migrates_legacy_default_agent_id() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("jobs.json");
        std::fs::write(
            &path,
            r#"{
          "jobs": [{
            "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "schedule": "every:5m",
            "task": "提醒喝水",
            "agent_id": "default",
            "enabled": true,
            "created_at": "2026-01-01T00:00:00Z"
          }]
        }"#,
        )
        .unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let jobs = store.list().unwrap();
        assert_eq!(jobs[0].agent_id, "workspace");
    }

    #[test]
    fn add_defaults_to_workspace_agent() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store.add("every:1h", "hi").unwrap();
        assert_eq!(job.agent_id, "workspace");
    }
}
