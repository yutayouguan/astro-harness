//! 桌面系统通知：注册 [`types::notify`] 钩子，供 cron / 入梦等后台任务使用。

use std::sync::Arc;

use tauri::{AppHandle, Runtime};
use tauri_plugin_notification::{NotificationExt, PermissionState};

/// 向操作系统弹出一条通知（失败仅打日志）。
pub fn show<R: Runtime>(app: &AppHandle<R>, title: &str, body: &str) {
    if let Err(err) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(error = %err, title, "desktop notification failed");
    }
}

/// 启动时请求权限，并把 `types::notify_important` 接到系统通知（主线程 show）。
pub fn install<R: Runtime>(app: &AppHandle<R>) {
    match app.notification().permission_state() {
        Ok(PermissionState::Granted) => {}
        Ok(_) => {
            if let Err(err) = app.notification().request_permission() {
                tracing::warn!(error = %err, "notification permission request failed");
            }
        }
        Err(err) => {
            tracing::warn!(error = %err, "notification permission_state failed");
        }
    }

    let app = app.clone();
    types::set_important_notify_handler(Arc::new(move |notice| {
        let app2 = app.clone();
        let title = notice.title;
        let body = notice.body;
        if let Err(err) = app.run_on_main_thread(move || {
            show(&app2, &title, &body);
        }) {
            tracing::warn!(error = %err, "schedule notification on main thread failed");
        }
    }));
}
