//! One long-lived Codex-style Thread event connection for the desktop shell.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;

use agent_protocol::TurnItem;
use proto::astro_service_client::AstroServiceClient;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::{watch, RwLock};
use tracing::debug;

use super::grpc::{default_grpc_address, endpoint_url};
use super::session_events::{
    emit_session_event, now_ts_ms, MemoryUpdatedDto, PendingChangedDto, SessionEventDto,
    SessionMetadataChangedDto,
};
use crate::commands::chat::{
    ChatStreamEvent, ContextUsageItemDto, ContextUsageSegmentDto, MediaAssetDto,
};

const SNAPSHOT_EVENT: &str = "thread_snapshot";

#[derive(Default)]
struct ActiveState {
    threads: HashSet<String>,
    turns: HashMap<String, String>,
    terminal_turns: HashMap<String, String>,
    activations: HashMap<String, u64>,
    next_activation: u64,
}

/// Process-wide connection state shared by chat commands and the event pump.
pub struct ThreadEventsBridge {
    connection_id: String,
    ready: watch::Sender<bool>,
    active_threads: RwLock<ActiveState>,
}

impl Default for ThreadEventsBridge {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadEventsBridge {
    pub fn new() -> Self {
        let (ready, _) = watch::channel(false);
        Self {
            connection_id: uuid::Uuid::new_v4().to_string(),
            ready,
            active_threads: RwLock::new(ActiveState::default()),
        }
    }

    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }

    pub async fn wait_ready(&self) {
        let mut ready = self.ready.subscribe();
        loop {
            if *ready.borrow_and_update() {
                return;
            }
            if ready.changed().await.is_err() {
                return;
            }
        }
    }

    fn set_ready(&self, value: bool) {
        self.ready.send_replace(value);
    }

    fn mark_recovering(&self) {
        self.set_ready(false);
    }

    fn mark_recovered(&self) {
        self.set_ready(true);
    }

    fn complete_recovery(&self, result: Result<(), String>) -> Result<(), String> {
        result?;
        self.mark_recovered();
        Ok(())
    }

    fn is_ready(&self) -> bool {
        *self.ready.borrow()
    }

    pub async fn activate(&self, thread_id: impl Into<String>) -> u64 {
        let thread_id = thread_id.into();
        let mut state = self.active_threads.write().await;
        state.next_activation = state.next_activation.wrapping_add(1).max(1);
        let activation = state.next_activation;
        state.threads.insert(thread_id.clone());
        state.turns.remove(&thread_id);
        state.activations.insert(thread_id, activation);
        activation
    }

    pub async fn deactivate_if_current(&self, thread_id: &str, activation: u64) {
        let mut state = self.active_threads.write().await;
        if state.activations.get(thread_id).copied() != Some(activation) {
            return;
        }
        state.threads.remove(thread_id);
        state.turns.remove(thread_id);
        state.activations.remove(thread_id);
    }

    pub async fn bind_turn(&self, thread_id: &str, turn_id: &str) {
        if turn_id.is_empty() {
            return;
        }
        let mut state = self.active_threads.write().await;
        if state.threads.contains(thread_id) {
            state.turns.insert(thread_id.into(), turn_id.into());
        }
    }

    pub async fn bind_turn_if_current(&self, thread_id: &str, activation: u64, turn_id: &str) {
        if turn_id.is_empty() {
            return;
        }
        let mut state = self.active_threads.write().await;
        if state.activations.get(thread_id).copied() == Some(activation) {
            state.turns.insert(thread_id.into(), turn_id.into());
        }
    }

    async fn active_ids(&self) -> Vec<String> {
        self.active_threads
            .read()
            .await
            .threads
            .iter()
            .cloned()
            .collect()
    }

    #[cfg(test)]
    async fn is_active(&self, thread_id: &str) -> bool {
        self.active_threads.read().await.threads.contains(thread_id)
    }

    /// Accept each terminal once and never let a stale terminal retire a newer Turn.
    async fn accept_terminal(&self, thread_id: &str, turn_id: &str) -> bool {
        let mut state = self.active_threads.write().await;
        if state
            .terminal_turns
            .get(thread_id)
            .is_some_and(|seen| seen == turn_id)
        {
            return false;
        }
        if state
            .turns
            .get(thread_id)
            .is_some_and(|active| active != turn_id)
        {
            return false;
        }
        state
            .terminal_turns
            .insert(thread_id.into(), turn_id.into());
        state.threads.remove(thread_id);
        state.turns.remove(thread_id);
        state.activations.remove(thread_id);
        true
    }
}

#[derive(Debug)]
struct RetryBackoff {
    next_ms: u64,
}

impl Default for RetryBackoff {
    fn default() -> Self {
        Self { next_ms: 500 }
    }
}

impl RetryBackoff {
    fn next_delay(&mut self) -> Duration {
        let delay = Duration::from_millis(self.next_ms);
        self.next_ms = self.next_ms.saturating_mul(2).min(15_000);
        delay
    }

    fn reset(&mut self) {
        self.next_ms = 500;
    }
}

pub fn accepted_turn_id(response: proto::SubmitTurnResponse) -> Result<String, String> {
    if response.disposition == "not_submitted" {
        Err(if response.reason.trim().is_empty() {
            "turn was not submitted".into()
        } else {
            response.reason
        })
    } else {
        Ok(response.turn_id)
    }
}

/// Register the shared bridge and start its reconnecting connection loop.
pub fn start_bridge(app: &AppHandle) {
    let bridge = Arc::new(ThreadEventsBridge::new());
    app.manage(Arc::clone(&bridge));
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        run_subscribe_loop(app, bridge).await;
    });
}

async fn run_subscribe_loop(app: AppHandle, bridge: Arc<ThreadEventsBridge>) {
    let mut backoff = RetryBackoff::default();
    loop {
        bridge.mark_recovering();
        match subscribe_once(&app, &bridge).await {
            Ok(()) => debug!("thread event stream closed"),
            Err(error) => debug!(%error, "thread event stream failed"),
        }
        if bridge.is_ready() {
            backoff.reset();
        }
        bridge.mark_recovering();
        tokio::time::sleep(backoff.next_delay()).await;
    }
}

async fn subscribe_once(app: &AppHandle, bridge: &ThreadEventsBridge) -> Result<(), String> {
    let endpoint = endpoint_url(&default_grpc_address());
    let mut client = AstroServiceClient::connect(endpoint)
        .await
        .map_err(|error| error.to_string())?;
    let mut stream = client
        .subscribe_thread_events(proto::SubscribeThreadEventsRequest {
            connection_id: bridge.connection_id().into(),
        })
        .await
        .map_err(|error| error.to_string())?
        .into_inner();

    // Reading begins before Resume RPCs. This prevents the server's bounded transport from
    // classifying a reconnecting desktop as a slow consumer while snapshots are rebuilt.
    let (live_tx, mut live_rx) = tokio::sync::mpsc::channel(128);
    tauri::async_runtime::spawn(async move {
        loop {
            match stream.message().await {
                Ok(Some(event)) => {
                    if live_tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                Ok(None) => {
                    let _ = live_tx.send(Err("thread event stream closed".into())).await;
                    return;
                }
                Err(error) => {
                    let _ = live_tx.send(Err(error.to_string())).await;
                    return;
                }
            }
        }
    });

    // Generation barrier: every durable snapshot is emitted before any buffered event from
    // this accepted stream. Public readiness remains false until recovery completes, so a new
    // SubmitTurn cannot race an old terminal snapshot.
    let mut snapshots = Vec::new();
    for thread_id in bridge.active_ids().await {
        match client
            .resume_thread(proto::ResumeThreadRequest {
                connection_id: bridge.connection_id().into(),
                thread_id: thread_id.clone(),
                include_turns: true,
            })
            .await
        {
            Ok(response) => {
                if let Some(snapshot) = response.into_inner().thread {
                    snapshots.push(snapshot);
                }
            }
            Err(error) => {
                return bridge.complete_recovery(Err(format!(
                    "failed to resume active thread {thread_id}: {error}"
                )));
            }
        }
    }

    let mut buffered = Vec::new();
    while let Ok(event) = live_rx.try_recv() {
        buffered.push(event);
    }
    for delivery in reconnect_delivery_order(snapshots, buffered) {
        match delivery {
            ReconnectDelivery::Snapshot(snapshot) => {
                let thread_id = snapshot.thread_id.clone();
                emit_snapshot(app, &snapshot);
                let reconciled = reconcile_snapshot(&snapshot);
                if let Some(turn_id) = reconciled.active_turn_id.as_deref() {
                    bridge.bind_turn(&thread_id, turn_id).await;
                }
                if !reconciled.keep_active {
                    let terminal_turn_id =
                        reconciled.terminal_turn_id.as_deref().unwrap_or_default();
                    if bridge.accept_terminal(&thread_id, terminal_turn_id).await {
                        emit_chat_events(app, &thread_id, reconciled.terminal);
                    }
                }
            }
            ReconnectDelivery::Live(event) => {
                process_live_event(app, bridge, event?).await;
            }
        }
    }
    bridge.complete_recovery(Ok(()))?;

    while let Some(event) = live_rx.recv().await {
        let event = event?;
        process_live_event(app, bridge, event).await;
    }
    Err("thread event reader stopped".into())
}

async fn process_live_event(
    app: &AppHandle,
    bridge: &ThreadEventsBridge,
    event: proto::ThreadEvent,
) {
    let thread_id = event.thread_id.clone();
    let turn_id = event.turn_id.clone();
    if let Some(proto::thread_event::Payload::TurnStarted(started)) = event.payload.as_ref() {
        bridge.bind_turn(&thread_id, &started.turn_id).await;
    }
    if let Some(proto::thread_event::Payload::Extension(extension)) = event.payload.as_ref() {
        if let Some(session_event) = extension_to_session_event(&thread_id, extension.clone()) {
            emit_session_event(app, session_event);
        }
    }
    let terminal = matches!(
        event.payload,
        Some(proto::thread_event::Payload::TurnComplete(_))
            | Some(proto::thread_event::Payload::TurnAborted(_))
    );
    if terminal && !bridge.accept_terminal(&thread_id, &turn_id).await {
        return;
    }
    emit_chat_events(app, &thread_id, map_thread_event(event));
}

enum ReconnectDelivery {
    Snapshot(proto::ThreadSnapshot),
    Live(Result<proto::ThreadEvent, String>),
}

fn reconnect_delivery_order(
    snapshots: Vec<proto::ThreadSnapshot>,
    buffered_live: Vec<Result<proto::ThreadEvent, String>>,
) -> Vec<ReconnectDelivery> {
    snapshots
        .into_iter()
        .map(ReconnectDelivery::Snapshot)
        .chain(buffered_live.into_iter().map(ReconnectDelivery::Live))
        .collect()
}

fn emit_chat_events(app: &AppHandle, thread_id: &str, events: Vec<ChatStreamEvent>) {
    let event_name = format!("chat_stream_{thread_id}");
    for event in events {
        let is_done = matches!(event, ChatStreamEvent::Done);
        let _ = app.emit(&event_name, event);
        if is_done {
            crate::commands::evolution_run::spawn_maybe_auto_evolution(app.clone());
            crate::commands::evolution_run::spawn_maybe_curator(app.clone());
        }
    }
}

fn map_thread_event(event: proto::ThreadEvent) -> Vec<ChatStreamEvent> {
    use proto::thread_event::Payload;
    let thread_id = event.thread_id;
    let turn_id = event.turn_id;
    match event.payload {
        Some(Payload::TurnStarted(started)) => vec![ChatStreamEvent::RunStarted {
            thread_id,
            run_id: started.turn_id,
        }],
        Some(Payload::ItemStarted(item)) => map_item_event(item, true),
        Some(Payload::ItemCompleted(item)) => map_item_event(item, false),
        Some(Payload::AgentMessageDelta(delta)) => {
            vec![ChatStreamEvent::Token { content: delta.delta }]
        }
        Some(Payload::ReasoningDelta(delta)) => {
            vec![ChatStreamEvent::Reasoning { content: delta.delta }]
        }
        Some(Payload::PlanDelta(delta)) => vec![activity(delta.item_id, "plan_delta", delta.delta)],
        Some(Payload::ExecOutputDelta(delta)) => {
            vec![activity(delta.item_id, "exec_output_delta", delta.delta)]
        }
        Some(Payload::PatchDelta(delta)) => {
            vec![activity(delta.item_id, "patch_delta", delta.delta)]
        }
        Some(Payload::ControlRequest(control)) => map_control_request(turn_id, control),
        Some(Payload::TokenCount(tokens)) => vec![ChatStreamEvent::Usage {
            prompt_tokens: tokens.input_tokens.min(u32::MAX.into()) as u32,
            completion_tokens: tokens.output_tokens.min(u32::MAX.into()) as u32,
            total_tokens: tokens.total_tokens.min(u32::MAX.into()) as u32,
        }],
        Some(Payload::Error(error)) => vec![ChatStreamEvent::Error {
            message: error.message,
        }],
        Some(Payload::Warning(warning)) => vec![activity(
            turn_id,
            "warning",
            serde_json::json!({"message":warning.message,"error_type":warning.error_type})
                .to_string(),
        )],
        Some(Payload::TurnComplete(complete)) => {
            let mut events = Vec::new();
            if complete.has_error {
                if let Some(error) = complete.error {
                    events.push(ChatStreamEvent::Error {
                        message: error.message,
                    });
                }
            }
            events.extend(terminal_events(
                turn_id,
                if complete.has_error { "error" } else { "success" },
                "[]".into(),
            ));
            events
        }
        Some(Payload::TurnAborted(aborted)) => terminal_events(
            turn_id,
            "interrupt",
            serde_json::json!([{"id":"","reason":aborted.reason,"message":"","tool_call_id":"","response_schema_json":"","expires_at":"","metadata_json":""}]).to_string(),
        ),
        Some(Payload::Extension(extension)) => map_extension_to_chat(extension),
        Some(Payload::ShutdownComplete(_)) => vec![activity(
            turn_id,
            "shutdown_complete",
            serde_json::json!({"shutdown_complete":true}).to_string(),
        )],
        None => vec![ChatStreamEvent::Error {
            message: "thread event has no payload".into(),
        }],
    }
}

fn terminal_events(
    run_id: String,
    outcome_type: &str,
    interrupts_json: String,
) -> Vec<ChatStreamEvent> {
    vec![
        ChatStreamEvent::RunFinished {
            run_id,
            outcome_type: outcome_type.into(),
            interrupts_json,
        },
        ChatStreamEvent::Done,
    ]
}

fn activity(
    message_id: impl Into<String>,
    activity_type: &str,
    content_json: impl Into<String>,
) -> ChatStreamEvent {
    ChatStreamEvent::Activity {
        message_id: message_id.into(),
        activity_type: activity_type.into(),
        content_json: content_json.into(),
        replace: false,
    }
}

fn map_control_request(
    run_id: String,
    control: proto::ThreadControlRequest,
) -> Vec<ChatStreamEvent> {
    let payload = serde_json::from_str::<serde_json::Value>(&control.payload_json)
        .unwrap_or(serde_json::Value::Null);
    if control.kind == "dynamic_tool_call" {
        return vec![ChatStreamEvent::ToolCallDelta {
            index: payload
                .get("index")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default()
                .min(u32::MAX.into()) as u32,
            id: control.item_id,
            name: payload
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
            arguments: payload
                .get("delta")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .into(),
        }];
    }
    if control.kind == "dynamic_tool_response" {
        return Vec::new();
    }
    if !matches!(
        control.kind.as_str(),
        "exec_approval"
            | "apply_patch_approval"
            | "request_permissions"
            | "request_user_input"
            | "elicitation"
    ) {
        return vec![activity(
            control.item_id,
            &control.kind,
            control.payload_json,
        )];
    }
    vec![ChatStreamEvent::RunFinished {
        run_id,
        outcome_type: "hitl_waiting".into(),
        interrupts_json: serde_json::json!([{
            "id": control.request_id,
            "reason": payload.get("reason").and_then(serde_json::Value::as_str).unwrap_or(&control.kind),
            "message": payload.get("message").and_then(serde_json::Value::as_str).unwrap_or_default(),
            "tool_call_id": control.item_id,
            "response_schema_json": payload.get("response_schema").cloned().unwrap_or_default().to_string(),
            "expires_at": payload.get("expires_at").and_then(serde_json::Value::as_str).unwrap_or_default(),
            "metadata_json": serde_json::json!({"kind":control.kind,"operations":payload.get("operations").cloned().unwrap_or_default(),"payload":payload}).to_string(),
        }])
        .to_string(),
    }]
}

fn map_item_event(item_event: proto::ThreadItemEvent, started: bool) -> Vec<ChatStreamEvent> {
    let Some(item) = item_event.item else {
        return vec![ChatStreamEvent::Error {
            message: "thread item event has no item".into(),
        }];
    };
    match serde_json::from_str::<TurnItem>(&item.payload_json) {
        Ok(TurnItem::CommandExecution(tool))
        | Ok(TurnItem::DynamicToolCall(tool))
        | Ok(TurnItem::McpToolCall(tool))
        | Ok(TurnItem::CollabAgentToolCall(tool))
        | Ok(TurnItem::WebSearch(tool))
        | Ok(TurnItem::ImageView(tool))
        | Ok(TurnItem::ImageGeneration(tool))
        | Ok(TurnItem::FileChange(tool)) => vec![ChatStreamEvent::ToolCall {
            id: tool.id,
            name: tool.name,
            arguments_json: tool.arguments.to_string(),
            result: tool
                .output
                .map(|value| match value {
                    serde_json::Value::String(text) => text,
                    other => other.to_string(),
                })
                .unwrap_or_default(),
            phase: if started { "started" } else { "completed" }.into(),
            media: tool.media.into_iter().map(media_asset_dto).collect(),
        }],
        Ok(TurnItem::AgentMessage(text)) if started => {
            vec![ChatStreamEvent::Token {
                content: text.content,
            }]
        }
        Ok(TurnItem::Reasoning(text)) if started => {
            vec![ChatStreamEvent::Reasoning {
                content: text.content,
            }]
        }
        Ok(TurnItem::AgentMessage(_)) | Ok(TurnItem::Reasoning(_)) => Vec::new(),
        Ok(TurnItem::HookPrompt(text)) => vec![ChatStreamEvent::Hook {
            name: "hook_prompt".into(),
            detail: text.content,
            outcome: if started { "started" } else { "completed" }.into(),
        }],
        Ok(TurnItem::Extension(extension)) if extension.namespace == "astro.memory" => {
            vec![memory_update_from_value(&extension.payload)]
        }
        Ok(_) => vec![activity(item.id, &item.item_type, item.payload_json)],
        Err(error) => vec![ChatStreamEvent::Error {
            message: format!("invalid thread item: {error}"),
        }],
    }
}

fn media_asset_dto(asset: types::MediaAsset) -> MediaAssetDto {
    let (ref_kind, ref_value) = match asset.reference {
        types::MediaRef::WorkspacePath(path) => ("workspace_path", path),
        types::MediaRef::DataUrl(url) => ("data_url", url),
        types::MediaRef::RemoteUri(uri) => ("remote_uri", uri),
    };
    let kind = match asset.kind {
        types::MediaKind::Image => "image",
        types::MediaKind::Audio => "audio",
        types::MediaKind::Video => "video",
        types::MediaKind::File => "file",
    };
    MediaAssetDto {
        kind: kind.into(),
        mime_type: asset.mime_type,
        ref_kind: ref_kind.into(),
        ref_value,
        label: asset.label,
        id: asset.id,
    }
}

fn map_extension_to_chat(extension: proto::ThreadExtension) -> Vec<ChatStreamEvent> {
    match extension.namespace.as_str() {
        "astro.thread_settings"
        | "astro.thread_rollback"
        | "astro.pending"
        | "astro.session_metadata" => Vec::new(),
        "astro.context_usage" => vec![context_usage_event(&extension.payload_json)],
        "astro.memory" => {
            match serde_json::from_str::<serde_json::Value>(&extension.payload_json) {
                Ok(payload) => vec![memory_update_from_value(&payload)],
                Err(error) => vec![ChatStreamEvent::Error {
                    message: format!("invalid memory update payload: {error}"),
                }],
            }
        }
        _ => vec![activity(
            extension.item_id,
            &extension.namespace,
            extension.payload_json,
        )],
    }
}

fn memory_update_from_value(payload: &serde_json::Value) -> ChatStreamEvent {
    ChatStreamEvent::MemoryUpdate {
        operation: payload
            .get("op")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("memory")
            .into(),
        content: payload
            .get("content")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default()
            .into(),
    }
}

fn context_usage_event(payload: &str) -> ChatStreamEvent {
    let value = serde_json::from_str::<serde_json::Value>(payload).unwrap_or_default();
    ChatStreamEvent::ContextUsage {
        context_window: json_u32(&value, "context_window"),
        total_tokens: json_u32(&value, "total_tokens"),
        segments: value
            .get("segments")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .map(|segment| ContextUsageSegmentDto {
                id: json_str(segment, "id"),
                tokens: json_u32(segment, "tokens"),
                count: json_u32(segment, "count"),
                items: segment
                    .get("items")
                    .and_then(serde_json::Value::as_array)
                    .into_iter()
                    .flatten()
                    .map(|item| ContextUsageItemDto {
                        id: json_str(item, "id"),
                        label: json_str(item, "label"),
                        tokens: json_u32(item, "tokens"),
                    })
                    .collect(),
            })
            .collect(),
        updated_at: value
            .get("updated_at")
            .and_then(serde_json::Value::as_i64)
            .unwrap_or_default(),
        recommend_compact: value
            .get("recommend_compact")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or_default(),
    }
}

fn json_str(value: &serde_json::Value, key: &str) -> String {
    value
        .get(key)
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .into()
}

fn json_u32(value: &serde_json::Value, key: &str) -> u32 {
    value
        .get(key)
        .and_then(serde_json::Value::as_u64)
        .unwrap_or_default()
        .min(u32::MAX.into()) as u32
}

fn extension_to_session_event(
    thread_id: &str,
    extension: proto::ThreadExtension,
) -> Option<SessionEventDto> {
    let value = serde_json::from_str::<serde_json::Value>(&extension.payload_json).ok()?;
    let (memory_updated, pending_changed, session_metadata_changed) =
        match extension.namespace.as_str() {
            "astro.memory" => (
                Some(MemoryUpdatedDto {
                    source: json_str(&value, "source"),
                    target: json_str(&value, "target"),
                    summary: json_str(&value, "summary"),
                    live_written: value
                        .get("live_written")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or_default(),
                }),
                None,
                None,
            ),
            "astro.pending" => (
                None,
                Some(PendingChangedDto {
                    pending_count: json_u32(&value, "pending_count"),
                    reason: json_str(&value, "reason"),
                }),
                None,
            ),
            "astro.session_metadata" => (
                None,
                None,
                Some(SessionMetadataChangedDto {
                    title: json_str(&value, "title"),
                }),
            ),
            _ => return None,
        };
    Some(SessionEventDto {
        session_id: Some(thread_id.into()),
        agent_id: String::new(),
        ts_ms: now_ts_ms(),
        event_id: 0,
        stream_id: String::new(),
        memory_updated,
        pending_changed,
        session_metadata_changed,
    })
}

struct SnapshotReconcile {
    terminal: Vec<ChatStreamEvent>,
    terminal_turn_id: Option<String>,
    active_turn_id: Option<String>,
    keep_active: bool,
}

fn reconcile_snapshot(snapshot: &proto::ThreadSnapshot) -> SnapshotReconcile {
    if snapshot.has_active_turn {
        if let Some(turn) = snapshot.active_turn.as_ref() {
            if turn.status == "in_progress" {
                return SnapshotReconcile {
                    terminal: Vec::new(),
                    terminal_turn_id: None,
                    active_turn_id: Some(turn.id.clone()),
                    keep_active: true,
                };
            }
        }
    }
    let terminal_turn = snapshot
        .active_turn
        .as_ref()
        .filter(|turn| matches!(turn.status.as_str(), "completed" | "failed" | "aborted"))
        .or_else(|| {
            snapshot
                .turns
                .iter()
                .rev()
                .find(|turn| matches!(turn.status.as_str(), "completed" | "failed" | "aborted"))
        });
    if let Some(turn) = terminal_turn {
        let outcome = match turn.status.as_str() {
            "failed" => "error",
            "aborted" => "interrupt",
            _ => "success",
        };
        let mut terminal = Vec::new();
        if turn.status == "failed" && turn.has_error {
            if let Some(error) = turn.error.as_ref() {
                terminal.push(ChatStreamEvent::Error {
                    message: error.message.clone(),
                });
            }
        }
        terminal.extend(terminal_events(turn.id.clone(), outcome, "[]".into()));
        return SnapshotReconcile {
            terminal,
            terminal_turn_id: Some(turn.id.clone()),
            active_turn_id: None,
            keep_active: false,
        };
    }
    SnapshotReconcile {
        terminal: Vec::new(),
        terminal_turn_id: None,
        active_turn_id: None,
        keep_active: true,
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadSnapshotDto<'a> {
    thread_id: &'a str,
    status: &'a str,
    turns: Vec<ThreadTurnDto<'a>>,
    active_turn: Option<ThreadTurnDto<'a>>,
    has_active_turn: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadTurnDto<'a> {
    id: &'a str,
    status: &'a str,
    items: Vec<ThreadItemDto<'a>>,
    last_agent_message: &'a str,
    has_error: bool,
    error: Option<ThreadErrorDto<'a>>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadItemDto<'a> {
    id: &'a str,
    item_type: &'a str,
    status: &'a str,
    payload_json: &'a str,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ThreadErrorDto<'a> {
    message: &'a str,
    error_type: &'a str,
}

fn turn_dto(turn: &proto::ThreadTurn) -> ThreadTurnDto<'_> {
    ThreadTurnDto {
        id: &turn.id,
        status: &turn.status,
        items: turn
            .items
            .iter()
            .map(|item| ThreadItemDto {
                id: &item.id,
                item_type: &item.item_type,
                status: &item.status,
                payload_json: &item.payload_json,
            })
            .collect(),
        last_agent_message: &turn.last_agent_message,
        has_error: turn.has_error,
        error: turn.error.as_ref().map(|error| ThreadErrorDto {
            message: &error.message,
            error_type: &error.error_type,
        }),
    }
}

fn snapshot_dto(snapshot: &proto::ThreadSnapshot) -> ThreadSnapshotDto<'_> {
    ThreadSnapshotDto {
        thread_id: &snapshot.thread_id,
        status: &snapshot.status,
        turns: snapshot.turns.iter().map(turn_dto).collect(),
        active_turn: snapshot.active_turn.as_ref().map(turn_dto),
        has_active_turn: snapshot.has_active_turn,
    }
}

fn emit_snapshot(app: &AppHandle, snapshot: &proto::ThreadSnapshot) {
    let _ = app.emit(SNAPSHOT_EVENT, snapshot_dto(snapshot));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal_event(thread_id: &str, turn_id: &str) -> proto::ThreadEvent {
        proto::ThreadEvent {
            thread_id: thread_id.into(),
            turn_id: turn_id.into(),
            payload: Some(proto::thread_event::Payload::TurnComplete(
                proto::ThreadTurnComplete {
                    last_agent_message: "done".into(),
                    error: None,
                    has_error: false,
                },
            )),
        }
    }

    #[test]
    fn terminal_thread_event_maps_to_run_finished_then_done() {
        let mapped = map_thread_event(terminal_event("session-1", "turn-1"));
        assert!(matches!(
            mapped.as_slice(),
            [
                ChatStreamEvent::RunFinished { outcome_type, .. },
                ChatStreamEvent::Done
            ] if outcome_type == "success"
        ));
    }

    #[test]
    fn memory_extension_maps_to_existing_session_event_shape() {
        let event = proto::ThreadExtension {
            item_id: "memory-1".into(),
            namespace: "astro.memory".into(),
            payload_json: serde_json::json!({
                "source":"review",
                "target":"memory",
                "summary":"updated",
                "live_written":true
            })
            .to_string(),
        };
        let mapped = extension_to_session_event("session-1", event).expect("memory event");
        let memory = mapped.memory_updated.expect("memory payload");
        assert_eq!(mapped.session_id.as_deref(), Some("session-1"));
        assert_eq!(memory.source, "review");
        assert!(memory.live_written);
    }

    #[test]
    fn terminal_snapshot_synthesizes_terminal_before_buffered_live_events() {
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "idle".into(),
            turns: vec![proto::ThreadTurn {
                id: "turn-1".into(),
                status: "completed".into(),
                items: vec![],
                last_agent_message: "done".into(),
                error: None,
                has_error: false,
            }],
            active_turn: None,
            has_active_turn: false,
        };
        let outcome = reconcile_snapshot(&snapshot);
        assert!(matches!(
            outcome.terminal.as_slice(),
            [
                ChatStreamEvent::RunFinished { outcome_type, .. },
                ChatStreamEvent::Done
            ] if outcome_type == "success"
        ));
        assert!(!outcome.keep_active);
    }

    #[test]
    fn running_snapshot_keeps_thread_active_without_terminal_projection() {
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "running".into(),
            turns: vec![],
            active_turn: Some(proto::ThreadTurn {
                id: "turn-1".into(),
                status: "in_progress".into(),
                items: vec![],
                last_agent_message: String::new(),
                error: None,
                has_error: false,
            }),
            has_active_turn: true,
        };
        let outcome = reconcile_snapshot(&snapshot);
        assert!(outcome.keep_active);
        assert!(outcome.terminal.is_empty());
    }

    #[test]
    fn reconnect_generation_delivers_every_snapshot_before_buffered_live() {
        let snapshots = vec![proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "running".into(),
            turns: vec![],
            active_turn: None,
            has_active_turn: false,
        }];
        let live = vec![Ok(proto::ThreadEvent {
            thread_id: "session-1".into(),
            turn_id: "turn-1".into(),
            payload: Some(proto::thread_event::Payload::AgentMessageDelta(
                proto::ThreadDelta {
                    item_id: "message-1".into(),
                    delta: "late".into(),
                },
            )),
        })];
        let ordered = reconnect_delivery_order(snapshots, live);
        assert!(matches!(
            ordered.first(),
            Some(ReconnectDelivery::Snapshot(_))
        ));
        assert!(matches!(ordered.get(1), Some(ReconnectDelivery::Live(_))));
    }

    #[tokio::test]
    async fn duplicate_terminal_is_suppressed_and_removes_active_thread() {
        let bridge = ThreadEventsBridge::new();
        bridge.activate("session-1").await;
        assert!(bridge.accept_terminal("session-1", "turn-1").await);
        assert!(!bridge.accept_terminal("session-1", "turn-1").await);
        assert!(!bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn stale_submit_failure_cannot_remove_a_newer_activation() {
        let bridge = ThreadEventsBridge::new();
        let old = bridge.activate("session-1").await;
        let current = bridge.activate("session-1").await;
        bridge.deactivate_if_current("session-1", old).await;
        assert!(bridge.is_active("session-1").await);
        bridge.deactivate_if_current("session-1", current).await;
        assert!(!bridge.is_active("session-1").await);
    }

    #[tokio::test]
    async fn ready_state_cannot_lose_a_wakeup() {
        let bridge = std::sync::Arc::new(ThreadEventsBridge::new());
        bridge.set_ready(true);
        tokio::time::timeout(std::time::Duration::from_millis(50), bridge.wait_ready())
            .await
            .expect("already-ready state must return immediately");
    }

    #[tokio::test]
    async fn reconnect_is_not_publicly_ready_until_snapshot_barrier_finishes() {
        let bridge = std::sync::Arc::new(ThreadEventsBridge::new());
        bridge.mark_recovering();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(10), bridge.wait_ready())
                .await
                .is_err()
        );
        bridge.mark_recovered();
        tokio::time::timeout(std::time::Duration::from_millis(50), bridge.wait_ready())
            .await
            .expect("completed recovery must release submitters");
    }

    #[tokio::test]
    async fn failed_resume_cannot_publish_connection_as_ready() {
        let bridge = ThreadEventsBridge::new();
        bridge.mark_recovering();
        assert!(bridge
            .complete_recovery(Err("resume failed".to_string()))
            .is_err());
        assert!(!bridge.is_ready());
        bridge.complete_recovery(Ok(())).unwrap();
        assert!(bridge.is_ready());
    }

    #[test]
    fn emitted_snapshot_keeps_turn_items_and_error_shape() {
        let snapshot = proto::ThreadSnapshot {
            thread_id: "session-1".into(),
            status: "errored".into(),
            turns: vec![proto::ThreadTurn {
                id: "turn-1".into(),
                status: "failed".into(),
                items: vec![proto::ThreadItem {
                    id: "tool-1".into(),
                    item_type: "command_execution".into(),
                    status: "failed".into(),
                    payload_json: r#"{"type":"command_execution"}"#.into(),
                }],
                last_agent_message: String::new(),
                error: Some(proto::ThreadError {
                    message: "boom".into(),
                    error_type: "provider".into(),
                }),
                has_error: true,
            }],
            active_turn: None,
            has_active_turn: false,
        };
        let value = serde_json::to_value(snapshot_dto(&snapshot)).unwrap();
        assert_eq!(value["turns"][0]["items"][0]["id"], "tool-1");
        assert_eq!(value["turns"][0]["error"]["errorType"], "provider");
    }

    #[test]
    fn reconnect_backoff_starts_at_500ms_and_caps_at_15s() {
        let mut backoff = RetryBackoff::default();
        assert_eq!(backoff.next_delay(), std::time::Duration::from_millis(500));
        for _ in 0..10 {
            backoff.next_delay();
        }
        assert_eq!(backoff.next_delay(), std::time::Duration::from_secs(15));
        backoff.reset();
        assert_eq!(backoff.next_delay(), std::time::Duration::from_millis(500));
    }

    #[test]
    fn not_submitted_response_is_an_error() {
        let response = proto::SubmitTurnResponse {
            submission_id: "submission-1".into(),
            turn_id: String::new(),
            disposition: "not_submitted".into(),
            reason: "thread is busy".into(),
        };
        assert_eq!(accepted_turn_id(response).unwrap_err(), "thread is busy");
    }

    #[test]
    fn started_response_returns_turn_id() {
        let response = proto::SubmitTurnResponse {
            submission_id: "submission-1".into(),
            turn_id: "turn-1".into(),
            disposition: "started".into(),
            reason: String::new(),
        };
        assert_eq!(accepted_turn_id(response).unwrap(), "turn-1");
    }
}
