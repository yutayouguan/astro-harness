//! 启动时后台补装随应用发布的内置 Skills → `~/.astro/skills`

use std::sync::atomic::{AtomicBool, Ordering};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

const EVENT_DEFAULT_SKILLS_SEEDED: &str = "default-skills-seeded";

#[derive(Clone, Serialize)]
struct Payload {
    installed: Vec<String>,
    failed: Vec<String>,
}

/// 在后台线程补装缺失的内置技能；有新装时 emit `default-skills-seeded`。
pub fn spawn_on_startup(app: &AppHandle) {
    static SEED_STARTED: AtomicBool = AtomicBool::new(false);
    if SEED_STARTED
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return;
    }

    let handle = app.clone();
    std::thread::spawn(move || {
        let report = skills::seed_bundled_skills();
        for name in &report.failed {
            tracing::warn!("default skill seed failed: {name}");
        }
        for name in &report.removed {
            tracing::info!("retired bundled skill removed: {name}");
        }
        if report.installed.is_empty() {
            return;
        }
        let _ = handle.emit(
            EVENT_DEFAULT_SKILLS_SEEDED,
            Payload {
                installed: report.installed,
                failed: report.failed,
            },
        );
    });
}
