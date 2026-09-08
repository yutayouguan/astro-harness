//! 定时任务调度：定义、持久化、到期扫描与工具分发。
//!
//! 职责：
//! - 将任务定义持久化到 `~/.astro/automation/cron/jobs.json`
//! - 解析 `every:` / `custom:` / 五段 cron 调度表达式并计算下次运行时间
//! - `claim_due` / `tick` 扫描到期任务并推进 `next_run_at`
//! - 为 Agent 工具与 Extractor 提供自然语言 → 结构化任务的入口
//!
//! 不变量：
//! - 任务 id 为 UUID，持久化前会校验 schedule 可解析
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
    use chrono::{Datelike, Local, Timelike};
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
    fn custom_calendar_schedules_cover_all_supported_frequencies() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 10, 10, 0).unwrap();

        let hourly = compute_next_run("custom:hourly;every=1;minute=30", after).unwrap();
        assert_eq!((hourly.hour(), hourly.minute()), (10, 30));

        let daily = compute_next_run("custom:daily;every=1;time=08:20", after).unwrap();
        assert_eq!((daily.day(), daily.hour(), daily.minute()), (12, 8, 20));

        let weekly = compute_next_run("custom:weekly;every=1;wd=1,3;time=08:00", after).unwrap();
        assert_eq!(weekly.weekday().num_days_from_sunday(), 1);
        assert_eq!((weekly.hour(), weekly.minute()), (8, 0));

        let monthly = compute_next_run("custom:monthly;every=1;day=15;time=09:30", after).unwrap();
        assert_eq!((monthly.month(), monthly.day()), (7, 15));
        assert_eq!((monthly.hour(), monthly.minute()), (9, 30));

        let yearly =
            compute_next_run("custom:yearly;every=1;month=1;day=1;time=08:00", after).unwrap();
        assert_eq!((yearly.year(), yearly.month(), yearly.day()), (2027, 1, 1));
        assert_eq!((yearly.hour(), yearly.minute()), (8, 0));
    }

    #[test]
    fn custom_calendar_schedule_uses_persisted_start_as_interval_phase() {
        use chrono::TimeZone;
        let created = Local.with_ymd_and_hms(2026, 7, 11, 10, 10, 0).unwrap();

        let first = compute_next_run(
            "custom:yearly;every=3;month=12;day=31;time=08:00;start=2026-07-11T10:10",
            created,
        )
        .unwrap();
        assert_eq!((first.year(), first.month(), first.day()), (2026, 12, 31));

        let next = compute_next_run(
            "custom:yearly;every=3;month=12;day=31;time=08:00;start=2026-07-11T10:10",
            first,
        )
        .unwrap();
        assert_eq!((next.year(), next.month(), next.day()), (2029, 12, 31));

        let first_daily = compute_next_run(
            "custom:daily;every=3;time=08:00;start=2026-07-11T10:10",
            created,
        )
        .unwrap();
        assert_eq!((first_daily.month(), first_daily.day()), (7, 12));
        let next_daily = compute_next_run(
            "custom:daily;every=3;time=08:00;start=2026-07-11T10:10",
            first_daily,
        )
        .unwrap();
        assert_eq!((next_daily.month(), next_daily.day()), (7, 15));
    }

    #[test]
    fn custom_yearly_leap_day_anchors_to_first_valid_occurrence() {
        use chrono::TimeZone;
        let created = Local.with_ymd_and_hms(2026, 7, 11, 10, 10, 0).unwrap();
        let first = compute_next_run(
            "custom:yearly;every=4;month=2;day=29;time=08:00;start=2026-07-11T10:10",
            created,
        )
        .unwrap();
        assert_eq!((first.year(), first.month(), first.day()), (2028, 2, 29));

        let next = compute_next_run(
            "custom:yearly;every=4;month=2;day=29;time=08:00;start=2026-07-11T10:10",
            first,
        )
        .unwrap();
        assert_eq!((next.year(), next.month(), next.day()), (2032, 2, 29));
    }

    #[test]
    fn custom_calendar_schedule_rejects_invalid_fields() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 10, 10, 0).unwrap();
        assert!(
            compute_next_run("custom:yearly;every=1;month=13;day=1;time=08:00", after).is_err()
        );
        assert!(compute_next_run("custom:weekly;every=0;wd=1;time=08:00", after).is_err());
        assert!(compute_next_run("custom:daily;every=1", after).is_err());
        assert!(compute_next_run("custom:hourly;every=1;time=08:00", after).is_err());
        assert!(compute_next_run("custom:daily;every=1;every=2;time=08:00", after).is_err());
        assert!(compute_next_run(
            "custom:daily;every=1;time=08:00;start=2026-99-99T10:10",
            after,
        )
        .is_err());
    }

    #[test]
    fn interval_schedule_rejects_overflow_without_panicking() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 10, 10, 0).unwrap();
        assert!(compute_next_run("every:9223372036854775807d", after).is_err());
    }

    #[test]
    fn store_persists_a_start_anchor_for_new_custom_schedules() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store
            .add("custom:daily;every=2;time=08:00", "锚点测试")
            .unwrap();
        assert!(job.schedule.contains(";start="));
        assert!(compute_next_run(&job.schedule, Local::now()).is_ok());
    }

    #[test]
    fn load_migrates_custom_schedule_without_start_from_created_at() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("jobs.json");
        std::fs::write(
            &path,
            r#"{
              "jobs": [{
                "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
                "schedule": "custom:daily;every=2;time=08:00",
                "task": "迁移锚点",
                "title": "迁移锚点",
                "agent_id": "default",
                "enabled": true,
                "created_at": "2026-07-11T10:10:00+08:00",
                "show_in_chat": false
              }]
            }"#,
        )
        .unwrap();

        let store = CronStore::open(dir.path()).unwrap();
        let jobs = store.list().unwrap();
        assert!(jobs[0].schedule.contains(";start="));
        let persisted = std::fs::read_to_string(path).unwrap();
        assert!(persisted.contains(";start="));
    }

    #[test]
    fn one_time_schedule_is_rejected() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 4, 0, 0).unwrap();
        assert!(compute_next_run("once:2026-07-11T04:22:00+08:00", after).is_err());
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
        assert_eq!(normalize_cron_agent_id(""), home::DEFAULT_AGENT_ID);
        assert_eq!(normalize_cron_agent_id("default"), home::DEFAULT_AGENT_ID);
        assert_eq!(normalize_cron_agent_id("DEFAULT"), home::DEFAULT_AGENT_ID);
        assert_eq!(normalize_cron_agent_id("workspace"), home::DEFAULT_AGENT_ID);
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
        assert_eq!(jobs[0].agent_id, home::DEFAULT_AGENT_ID);
    }

    #[test]
    fn add_defaults_to_default_agent() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store.add("every:1h", "hi").unwrap();
        assert_eq!(job.agent_id, home::DEFAULT_AGENT_ID);
    }

    #[test]
    fn concurrent_adds_preserve_every_job() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().to_path_buf();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
        let mut handles = Vec::new();
        for index in 0..8 {
            let root = root.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            handles.push(std::thread::spawn(move || {
                let store = CronStore::open(root).unwrap();
                barrier.wait();
                store
                    .add("every:1h", &format!("concurrent-{index}"))
                    .unwrap();
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }
        let jobs = CronStore::open(dir.path()).unwrap().list().unwrap();
        assert_eq!(jobs.len(), 8);
    }
}
