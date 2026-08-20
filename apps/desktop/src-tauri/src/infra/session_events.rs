//! 本机 `session_event` 载荷 + gRPC `SubscribeSessionEvents` 桥接。

use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{Notify, RwLock};
use tracing::debug;

use super::grpc::{default_grpc_address, endpoint_url};

const EVENT_NAME: &str = "session_event";

/// 前端 `listen("session_event")` 的事件体。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionEventDto {
    pub session_id: Option<String>,
    pub agent_id: String,
    pub ts_ms: i64,
    /// Backend replay cursor. Local-only events use zero / empty stream id.
    pub event_id: u64,
    pub stream_id: String,
    pub memory_updated: Option<MemoryUpdatedDto>,
    pub pending_changed: Option<PendingChangedDto>,
    pub session_metadata_changed: Option<SessionMetadataChangedDto>,
    pub agent_thread_changed: Option<AgentThreadChangedDto>,
    pub resync_required: Option<SessionResyncRequiredDto>,
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

/// 会话元数据变化（如自动/手动标题）。
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadataChangedDto {
    pub title: String,
}

/// Codex V2 Agent Thread activity with a complete current projection.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentThreadChangedDto {
    pub activity_sequence: u64,
    pub root_thread_id: String,
    pub thread_id: String,
    pub parent_thread_id: String,
    pub canonical_path: String,
    pub task_name: String,
    pub agent_type: String,
    pub session_id: String,
    pub status_kind: String,
    pub status_payload_json: String,
    pub activity_kind: String,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResyncRequiredDto {
    pub reason: String,
}

/// 订阅过滤（可热更新）。
#[derive(Debug, Clone, Default)]
struct FilterState {
    session_id: Option<String>,
    agent_id: Option<String>,
}

/// 进程内共享过滤 + 重启信号世代。
struct SessionEventsBridge {
    filter: RwLock<FilterState>,
    /// 递增后当前订阅循环应退出并按新 filter 重连。
    generation: RwLock<u64>,
    filter_changed: Notify,
}

impl SessionEventsBridge {
    fn new() -> Self {
        Self {
            filter: RwLock::new(FilterState::default()),
            generation: RwLock::new(0),
            filter_changed: Notify::new(),
        }
    }
}

/// 当前 UTC 毫秒时间戳。
pub fn now_ts_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 向前端 emit `session_event`（无订阅者时静默）。
pub fn emit_session_event(app: &AppHandle, ev: SessionEventDto) {
    let _ = app.emit(EVENT_NAME, ev);
}

fn proto_to_dto(ev: proto::SessionEvent) -> SessionEventDto {
    let session_id = {
        let s = ev.session_id.trim();
        if s.is_empty() {
            None
        } else {
            Some(s.to_string())
        }
    };
    let (
        memory_updated,
        pending_changed,
        session_metadata_changed,
        agent_thread_changed,
        resync_required,
    ) = match ev.payload {
        Some(proto::session_event::Payload::MemoryUpdated(m)) => (
            Some(MemoryUpdatedDto {
                source: m.source,
                target: m.target,
                summary: m.summary,
                live_written: m.live_written,
            }),
            None,
            None,
            None,
            None,
        ),
        Some(proto::session_event::Payload::PendingChanged(p)) => (
            None,
            Some(PendingChangedDto {
                pending_count: p.pending_count,
                reason: p.reason,
            }),
            None,
            None,
            None,
        ),
        Some(proto::session_event::Payload::SessionMetadataChanged(m)) => (
            None,
            None,
            Some(SessionMetadataChangedDto { title: m.title }),
            None,
            None,
        ),
        Some(proto::session_event::Payload::AgentThreadChanged(thread)) => (
            None,
            None,
            None,
            Some(AgentThreadChangedDto {
                activity_sequence: thread.activity_sequence,
                root_thread_id: thread.root_thread_id,
                thread_id: thread.thread_id,
                parent_thread_id: thread.parent_thread_id,
                canonical_path: thread.canonical_path,
                task_name: thread.task_name,
                agent_type: thread.agent_type,
                session_id: thread.session_id,
                status_kind: thread.status_kind,
                status_payload_json: thread.status_payload_json,
                activity_kind: thread.activity_kind,
            }),
            None,
        ),
        Some(proto::session_event::Payload::ResyncRequired(reset)) => (
            None,
            None,
            None,
            None,
            Some(SessionResyncRequiredDto {
                reason: reset.reason,
            }),
        ),
        None => (None, None, None, None, None),
    };
    SessionEventDto {
        session_id,
        agent_id: ev.agent_id,
        ts_ms: if ev.ts_ms != 0 { ev.ts_ms } else { now_ts_ms() },
        event_id: ev.event_id,
        stream_id: ev.stream_id,
        memory_updated,
        pending_changed,
        session_metadata_changed,
        agent_thread_changed,
        resync_required,
    }
}

/// App 启动时注册 bridge 并启动重连循环。
pub fn start_bridge(app: &AppHandle) {
    let bridge = Arc::new(SessionEventsBridge::new());
    app.manage(bridge.clone());
    let app2 = app.clone();
    tauri::async_runtime::spawn(async move {
        run_subscribe_loop(app2, bridge).await;
    });
}

/// 更新订阅过滤并触发重连。
#[tauri::command]
pub async fn set_session_events_filter(
    app: AppHandle,
    session_id: Option<String>,
    agent_id: Option<String>,
) -> Result<(), String> {
    let bridge = app
        .try_state::<Arc<SessionEventsBridge>>()
        .ok_or_else(|| "session events bridge not started".to_string())?;
    {
        let mut f = bridge.filter.write().await;
        f.session_id = session_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        f.agent_id = agent_id
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
    }
    {
        let mut g = bridge.generation.write().await;
        *g += 1;
    }
    bridge.filter_changed.notify_one();
    Ok(())
}

async fn run_subscribe_loop(app: AppHandle, bridge: Arc<SessionEventsBridge>) {
    let mut backoff_ms: u64 = 500;
    let mut stream_id = String::new();
    let mut after_event_id = 0;
    loop {
        let gen_at_start = *bridge.generation.read().await;
        let (session_id, agent_id) = {
            let f = bridge.filter.read().await;
            (
                f.session_id.clone().unwrap_or_default(),
                f.agent_id.clone().unwrap_or_default(),
            )
        };

        match subscribe_once(
            &app,
            &bridge,
            gen_at_start,
            session_id,
            agent_id,
            &mut stream_id,
            &mut after_event_id,
        )
        .await
        {
            Ok(()) => {
                backoff_ms = 500;
            }
            Err(e) => {
                debug!(error = %e, "session events subscribe ended");
            }
        }

        // 若 generation 已变（热切换 filter），立刻重连；否则退避
        let filter_changed = bridge.filter_changed.notified();
        let gen_now = *bridge.generation.read().await;
        if gen_now == gen_at_start {
            tokio::select! {
                () = tokio::time::sleep(Duration::from_millis(backoff_ms)) => {
                    backoff_ms = (backoff_ms.saturating_mul(2)).min(15_000);
                }
                () = filter_changed => {}
            }
        }
    }
}

async fn subscribe_once(
    app: &AppHandle,
    bridge: &SessionEventsBridge,
    gen_at_start: u64,
    session_id: String,
    agent_id: String,
    stream_id: &mut String,
    after_event_id: &mut u64,
) -> Result<(), String> {
    let url = endpoint_url(&default_grpc_address());
    let mut client = proto::astro_service_client::AstroServiceClient::connect(url)
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = client
        .subscribe_session_events(proto::SubscribeSessionEventsRequest {
            session_id,
            agent_id,
            after_event_id: *after_event_id,
            stream_id: stream_id.clone(),
        })
        .await
        .map_err(|e| e.to_string())?
        .into_inner();

    loop {
        let filter_changed = bridge.filter_changed.notified();
        if *bridge.generation.read().await != gen_at_start {
            return Ok(());
        }
        let ev = tokio::select! {
            result = stream.message() => result.map_err(|e| e.to_string())?,
            () = filter_changed => return Ok(()),
        };
        let Some(ev) = ev else {
            return Err("session events stream closed".into());
        };
        if !ev.stream_id.is_empty() && ev.stream_id != *stream_id {
            *stream_id = ev.stream_id.clone();
            *after_event_id = 0;
        }
        if ev.event_id != 0 {
            *after_event_id = ev.event_id;
        }
        emit_session_event(app, proto_to_dto(ev));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proto_to_dto_maps_complete_agent_thread_projection() {
        let dto = proto_to_dto(proto::SessionEvent {
            session_id: "root".into(),
            agent_id: "reviewer".into(),
            ts_ms: 1,
            event_id: 2,
            stream_id: "stream".into(),
            payload: Some(proto::session_event::Payload::AgentThreadChanged(
                proto::AgentThreadChangedEvent {
                    activity_sequence: 7,
                    root_thread_id: "root".into(),
                    thread_id: "thread".into(),
                    parent_thread_id: "root".into(),
                    canonical_path: "/root/worker".into(),
                    task_name: "worker".into(),
                    agent_type: "reviewer".into(),
                    session_id: "thread-session".into(),
                    status_kind: "running".into(),
                    status_payload_json: r#"{"kind":"running"}"#.into(),
                    activity_kind: "status_changed".into(),
                },
            )),
        });

        let projection = dto.agent_thread_changed.expect("agent thread projection");
        assert_eq!(projection.activity_sequence, 7);
        assert_eq!(projection.root_thread_id, "root");
        assert_eq!(projection.thread_id, "thread");
        assert_eq!(projection.parent_thread_id, "root");
        assert_eq!(projection.canonical_path, "/root/worker");
        assert_eq!(projection.task_name, "worker");
        assert_eq!(projection.agent_type, "reviewer");
        assert_eq!(projection.session_id, "thread-session");
        assert_eq!(projection.status_kind, "running");
        assert_eq!(projection.status_payload_json, r#"{"kind":"running"}"#);
        assert_eq!(projection.activity_kind, "status_changed");
    }

    #[test]
    fn proto_to_dto_maps_resync_contract() {
        let dto = proto_to_dto(proto::SessionEvent {
            session_id: "root".into(),
            agent_id: "default".into(),
            ts_ms: 1,
            event_id: 3,
            stream_id: "new-generation".into(),
            payload: Some(proto::session_event::Payload::ResyncRequired(
                proto::SessionResyncRequiredEvent {
                    reason: "replay_gap".into(),
                },
            )),
        });

        assert_eq!(
            dto.resync_required.expect("resync dto").reason,
            "replay_gap"
        );
    }
}
