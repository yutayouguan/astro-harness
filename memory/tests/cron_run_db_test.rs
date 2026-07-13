//! 定时任务运行记录库（`CronRunDb`）读写与过滤测试。

use memory::cron_run_db::{CronRunDb, CronRunFilters, NewCronRun};
use tempfile::TempDir;

#[test]
fn insert_finish_and_filter_by_job() {
    let dir = TempDir::new().unwrap();
    let db = CronRunDb::new(dir.path().join("cron.db")).unwrap();
    let id = db
        .insert_running(NewCronRun {
            job_id: "job-1".into(),
            title: "清理".into(),
            agent_id: "workspace".into(),
            schedule: "every:1d".into(),
            task: "清理下载目录".into(),
            fired_at: "2026-07-11T11:00:00+08:00".into(),
            trigger: "due".into(),
            session_id: None,
        })
        .unwrap();
    db.finish_success(
        &id,
        "删了 3 个文件",
        "### 报告\n- a\n- b\n- c",
        "2026-07-11T11:01:00+08:00",
    )
    .unwrap();
    let rows = db
        .list_filtered(CronRunFilters {
            job_id: Some("job-1".into()),
            agent_id: None,
            date_from: None,
            date_to: None,
            limit: 50,
        })
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].status, "success");
    assert!(rows[0].summary.contains("删了"));
}
