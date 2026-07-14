//! 本机 `session_event` 载荷（与 gRPC SessionEvent 同形，camelCase 给前端 listen）。

use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// 前端 `listen("session_event")` 的事件体。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEventDto {
    pub session_id: Option<String>,
    pub agent_id: String,
    pub ts_ms: i64,
    pub memory_updated: Option<MemoryUpdatedDto>,
    pub pending_changed: Option<PendingChangedDto>,
}

/// 记忆已更新（或仅入 pending）摘要。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MemoryUpdatedDto {
    pub source: String,
    pub target: String,
    pub summary: String,
    pub live_written: bool,
}

/// Pending 队列计数变化。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingChangedDto {
    pub pending_count: u32,
    pub reason: String,
}

/// 当前 UTC 毫秒时间戳。
pub fn now_ts_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 向前端 emit `session_event`（无订阅者时静默）。
pub fn emit_session_event(app: &AppHandle, ev: SessionEventDto) {
    let _ = app.emit("session_event", ev);
}
