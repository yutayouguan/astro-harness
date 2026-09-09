//! 重要事件桌面通知钩子：由桌面壳注册，backend / 命令侧触发。
//!
//! 独立 `cargo run -p backend` 未注册时静默跳过，不影响无 UI 运行。
//! 标题文案跟随 [`set_notify_locale`]（默认中文）。

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, OnceLock};

/// 一条面向用户的重要通知。
#[derive(Debug, Clone)]
pub struct ImportantNotice {
    pub kind: Option<ImportantKind>,
    pub title: String,
    pub body: String,
}

/// 预置重要事件类型（标题双语，正文由调用方拼）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportantKind {
    CronSuccess,
    CronFailure,
    DreamSuccess,
    DreamFailure,
}

type Handler = Arc<dyn Fn(ImportantNotice) + Send + Sync>;

static HANDLER: OnceLock<Handler> = OnceLock::new();
/// 0 = zh，1 = en
static LOCALE: AtomicU8 = AtomicU8::new(0);

/// 由桌面壳在启动时注册（仅生效一次）。
pub fn set_important_notify_handler(handler: Handler) {
    let _ = HANDLER.set(handler);
}

/// 同步界面语言（`"zh"` / `"en"`），影响 [`notify_kind`] 标题。
pub fn set_notify_locale(locale: &str) {
    let v = match locale.trim().to_ascii_lowercase().as_str() {
        "en" => 1u8,
        _ => 0u8,
    };
    LOCALE.store(v, Ordering::Relaxed);
}

fn is_en() -> bool {
    LOCALE.load(Ordering::Relaxed) == 1
}

pub fn notify_locale() -> &'static str {
    if is_en() {
        "en"
    } else {
        "zh"
    }
}

impl ImportantKind {
    fn title(self) -> &'static str {
        match (self, is_en()) {
            (Self::CronSuccess, false) => "定时任务完成",
            (Self::CronSuccess, true) => "Scheduled task finished",
            (Self::CronFailure, false) => "定时任务失败",
            (Self::CronFailure, true) => "Scheduled task failed",
            (Self::DreamSuccess, false) => "入梦完成",
            (Self::DreamSuccess, true) => "Dreaming finished",
            (Self::DreamFailure, false) => "入梦失败",
            (Self::DreamFailure, true) => "Dreaming failed",
        }
    }
}

/// 触发重要通知；未注册 handler 时为 no-op。
pub fn notify_important(title: impl Into<String>, body: impl Into<String>) {
    if let Some(handler) = HANDLER.get() {
        handler(ImportantNotice {
            kind: None,
            title: title.into(),
            body: body.into(),
        });
    }
}

/// 按当前语言发预置标题的重要通知。
pub fn notify_kind(kind: ImportantKind, body: impl Into<String>) {
    if let Some(handler) = HANDLER.get() {
        handler(ImportantNotice {
            kind: Some(kind),
            title: kind.title().into(),
            body: body.into(),
        });
    }
}

/// 入梦成功通知正文（双语）。
pub fn dream_success_body(diaries: usize, memories: u64) -> String {
    if is_en() {
        format!("Processed {diaries} diaries, wrote {memories} memories")
    } else {
        format!("处理 {diaries} 篇日记，写入 {memories} 条记忆")
    }
}

/// UTF-8 安全截断（用于通知正文）。
pub fn truncate_notify(s: &str, max_chars: usize) -> String {
    let mut it = s.chars();
    let head: String = it.by_ref().take(max_chars).collect();
    if it.next().is_some() {
        format!("{head}…")
    } else {
        head
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_notify_ellipsis() {
        assert_eq!(truncate_notify("abc", 5), "abc");
        assert_eq!(truncate_notify("abcdefgh", 5), "abcde…");
        assert_eq!(truncate_notify("你好世界啊", 2), "你好…");
    }

    #[test]
    fn locale_switches_kind_title() {
        set_notify_locale("zh");
        assert_eq!(ImportantKind::CronSuccess.title(), "定时任务完成");
        set_notify_locale("en");
        assert_eq!(
            ImportantKind::CronSuccess.title(),
            "Scheduled task finished"
        );
        set_notify_locale("zh");
    }
}
