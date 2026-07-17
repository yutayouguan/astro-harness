//! 重要事件桌面通知钩子：由桌面壳注册，backend / 命令侧触发。
//!
//! 独立 `cargo run -p backend` 未注册时静默跳过，不影响无 UI 运行。

use std::sync::{Arc, OnceLock};

/// 一条面向用户的重要通知。
#[derive(Debug, Clone)]
pub struct ImportantNotice {
    pub title: String,
    pub body: String,
}

type Handler = Arc<dyn Fn(ImportantNotice) + Send + Sync>;

static HANDLER: OnceLock<Handler> = OnceLock::new();

/// 由桌面壳在启动时注册（仅生效一次）。
pub fn set_important_notify_handler(handler: Handler) {
    let _ = HANDLER.set(handler);
}

/// 触发重要通知；未注册 handler 时为 no-op。
pub fn notify_important(title: impl Into<String>, body: impl Into<String>) {
    if let Some(handler) = HANDLER.get() {
        handler(ImportantNotice {
            title: title.into(),
            body: body.into(),
        });
    }
}
