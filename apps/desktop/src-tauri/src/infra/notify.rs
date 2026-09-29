//! 桌面系统通知：注册 [`types::notify`] 钩子，供 cron / 入梦等后台任务使用。

use std::sync::Arc;

use crate::commands::chat::ChatStreamEvent;
use tauri::{AppHandle, Emitter, Runtime};
use tauri_plugin_notification::{NotificationExt, PermissionState};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskNotice {
    Complete,
    Failed,
    NeedsInput,
}

fn notice_copy(kind: TaskNotice) -> &'static str {
    match (kind, types::notify::notify_locale()) {
        (TaskNotice::Complete, "en") => "Task completed",
        (TaskNotice::Failed, "en") => "Task failed",
        (TaskNotice::NeedsInput, "en") => "Action required",
        (TaskNotice::Complete, _) => "任务已完成",
        (TaskNotice::Failed, _) => "任务执行失败",
        (TaskNotice::NeedsInput, _) => "任务需要你的确认",
    }
}

/// Never expose task titles, prompts, tool arguments, or error bodies on the lock screen.
pub(crate) fn show_task<R: Runtime>(app: &AppHandle<R>, kind: TaskNotice) {
    if !crate::commands::ui::desktop_preferences::notifications_enabled_at(
        &home::default_memory_dir(),
    )
    .unwrap_or(false)
    {
        return;
    }
    if !matches!(
        app.notification().permission_state(),
        Ok(PermissionState::Granted)
    ) {
        return;
    }
    if let Err(err) = app
        .notification()
        .builder()
        .title("Astro Harness")
        .body(notice_copy(kind))
        .show()
    {
        tracing::warn!(error = %err, "desktop notification failed");
    }
}

pub(crate) fn notice_for_event(event: &ChatStreamEvent) -> Option<TaskNotice> {
    match event {
        ChatStreamEvent::RunFinished { outcome_type, .. } => match outcome_type.as_str() {
            "success" => Some(TaskNotice::Complete),
            "error" => Some(TaskNotice::Failed),
            "hitl_waiting" => Some(TaskNotice::NeedsInput),
            _ => None,
        },
        _ => None,
    }
}

pub(crate) fn emit_live_task_notices(app: &AppHandle, events: &[ChatStreamEvent]) {
    for event in events {
        if let Some(kind) = notice_for_event(event) {
            let cloned = app.clone();
            let _ = app.run_on_main_thread(move || show_task(&cloned, kind));
        }
    }
}

/// Register delivery only. Permission prompts are exclusively user-triggered.
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    let notify_app = app.clone();
    types::set_important_notify_handler(Arc::new(move |notice| {
        let kind = match notice.kind {
            Some(types::ImportantKind::CronSuccess) => TaskNotice::Complete,
            Some(types::ImportantKind::CronFailure) => TaskNotice::Failed,
            _ => return,
        };
        let app2 = notify_app.clone();
        if let Err(err) = notify_app.run_on_main_thread(move || {
            show_task(&app2, kind);
        }) {
            tracing::warn!(error = %err, "schedule notification on main thread failed");
        }
    }));

    let style_app = app.clone();
    types::set_ui_style_change_handler(Arc::new(move || {
        if let Err(error) = style_app.emit("ui-style-changed", ()) {
            tracing::warn!(%error, "emit ui-style-changed failed");
        }
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_terminal_or_action_required_events_notify_without_content() {
        for (outcome, expected) in [
            ("success", Some(TaskNotice::Complete)),
            ("error", Some(TaskNotice::Failed)),
            ("hitl_waiting", Some(TaskNotice::NeedsInput)),
            ("interrupt", None),
        ] {
            let event = ChatStreamEvent::RunFinished {
                run_id: "private-thread".into(),
                outcome_type: outcome.into(),
                interrupts_json: "SECRET task contents".into(),
            };
            assert_eq!(notice_for_event(&event), expected);
            if let Some(kind) = expected {
                assert!(!notice_copy(kind).contains("SECRET"));
            }
        }
        assert_eq!(
            notice_for_event(&ChatStreamEvent::Token {
                content: "secret".into()
            }),
            None
        );
        assert_eq!(notice_for_event(&ChatStreamEvent::Done), None);
    }
}
