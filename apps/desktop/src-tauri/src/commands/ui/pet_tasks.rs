//! Separate native surfaces for tasks/controls; the pet's hit-test area never grows.
use super::pet_task_motion::{Morph, Rect};
use crate::infra::grpc::{default_grpc_address, endpoint_url};
use proto::astro_service_client::AstroServiceClient;
use std::{collections::HashSet, sync::Mutex};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
#[cfg(not(target_os = "macos"))]
use tauri::{PhysicalPosition, PhysicalSize};
use types::pending_interaction::{InteractionResponse, InteractionSnapshot};

const POPUP: &str = "pet-task-popup";
const POPUP_WIDTH: f64 = 392.0;
const POPUP_MAX_HEIGHT: f64 = 560.0;
const POPUP_MIN_HEIGHT: f64 = 180.0;
// Commands and the timer may ask to refresh concurrently. Window construction
// is serialized separately from the request state mutex used by WebView IPC.
static WINDOW_REFRESH: Mutex<()> = Mutex::new(());
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopInteractions {
    pub expanded: bool,
    pub ui_revision: u64,
    pub connected: bool,
    pub snapshot: InteractionSnapshot,
    pub selected: Option<String>,
}
#[derive(Default)]
struct State {
    ui_revision: u64,
    connected: bool,
    snapshot: InteractionSnapshot,
    seen: HashSet<String>,
    main_visible: HashSet<String>,
    selected: Option<String>,
    open: bool,
    initialized: bool,
    retired_epochs: HashSet<String>,
    navigation: Option<serde_json::Value>,
    last_position: Option<(i32, i32)>,
    auto_candidate: Option<(String, std::time::Instant)>,
    popup_height: Option<f64>,
    reduced_motion: bool,
}
#[derive(Default)]
pub struct PetTasks(Mutex<State>, Mutex<RenderState>);
pub(super) fn blocks_roaming(app: &AppHandle) -> bool {
    let Some(tasks) = app.try_state::<PetTasks>() else {
        return true;
    };
    tasks.0.lock().map_or(true, |state| {
        !state.connected
            || state.open
            || !state.snapshot.tasks.is_empty()
            || !state.snapshot.requests.is_empty()
    })
}
#[derive(Clone, Copy)]
struct Geometry {
    pet: (i32, i32, u32, u32),
    area: (i32, i32, u32, u32),
    scale: f64,
}
#[derive(Default)]
struct RenderState {
    geometry: Option<Geometry>,
    checked: Option<std::time::Instant>,
    tick: Option<std::time::Instant>,
    motion: Morph,
    visible: bool,
    last_rect: Option<Rect>,
    last_progress: Option<f64>,
    animating: bool,
    sequence: u64,
    last_goal: Option<(bool, f64, bool)>,
}
fn dto(state: &State) -> DesktopInteractions {
    DesktopInteractions {
        expanded: state.open,
        ui_revision: state.ui_revision,
        connected: state.connected,
        snapshot: state.snapshot.clone(),
        selected: state.selected.clone(),
    }
}

fn publish(app: &AppHandle) {
    let value = {
        let holder = app.state::<PetTasks>();
        let mut state = holder.0.lock().unwrap();
        state.ui_revision = state.ui_revision.saturating_add(1);
        dto(&state)
    };
    for label in ["main", POPUP] {
        let _ = app.emit_to(label, "pending-interactions-changed", &value);
    }
}
/// OS Close/⌘W has the same semantics as snooze, never reject or cancel work.
/// Called on the main thread: update state only; the frame driver owns geometry.
pub fn close_from_native(app: &AppHandle, label: &str) -> bool {
    if label != POPUP {
        return false;
    }
    let Some(holder) = app.try_state::<PetTasks>() else {
        return false;
    };
    holder.0.lock().unwrap().suppress_popup();
    publish(app);
    true
}
pub fn install(app: &AppHandle) {
    app.manage(PetTasks::default());
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            let result = async {
                let mut client = AstroServiceClient::connect(endpoint_url(&default_grpc_address()))
                    .await
                    .map_err(|e| e.to_string())?;
                let mut stream = client
                    .watch_pending_interactions(proto::Empty {})
                    .await
                    .map_err(|e| e.to_string())?
                    .into_inner();
                while let Some(item) = stream.message().await.map_err(|e| e.to_string())? {
                    let snapshot: InteractionSnapshot =
                        serde_json::from_str(&item.json).map_err(|e| e.to_string())?;
                    receive(&handle, snapshot);
                }
                Ok::<(), String>(())
            }
            .await;
            {
                handle.state::<PetTasks>().0.lock().unwrap().connected = false;
            }
            publish(&handle);
            if let Err(error) = result {
                tracing::debug!(%error,"pending interaction stream reconnecting");
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    });
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut tick = tokio::time::interval(std::time::Duration::from_millis(16));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut last_error = None;
        loop {
            tick.tick().await;
            let app = handle.clone();
            let error =
                match tauri::async_runtime::spawn_blocking(move || refresh_windows(&app)).await {
                    Ok(Ok(())) => None,
                    Ok(Err(error)) => Some(error),
                    Err(error) => Some(error.to_string()),
                };
            if error != last_error {
                if let Some(error) = &error {
                    tracing::warn!(%error, "desktop pet task surfaces could not refresh");
                }
                last_error = error;
            }
        }
    });
}
fn pet_visible(app: &AppHandle) -> bool {
    if !app
        .get_webview_window("desktop-pet")
        .is_some_and(|w| w.is_visible().unwrap_or(false))
    {
        return false;
    }
    let Ok(state) = types::read_desktop_pet_state(&home::default_memory_dir()) else {
        return false;
    };
    state.enabled
        && !state.preferences.presentation_mode
        && !(state.preferences.hide_in_fullscreen && super::desktop_pet::fullscreen_now(app))
}
fn receive(app: &AppHandle, snapshot: InteractionSnapshot) {
    let allowed = pet_visible(app);
    let main_front = app
        .get_webview_window("main")
        .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
    {
        let holder = app.state::<PetTasks>();
        let mut state = holder.0.lock().unwrap();
        if !state.apply_snapshot(snapshot, allowed, main_front) {
            return;
        }
    }
    publish(app);
}

impl State {
    fn suppress_popup(&mut self) {
        self.open = false;
        self.auto_candidate = None;
    }
    fn update_anchor(&mut self, visible: bool, position: (i32, i32)) {
        if !visible || self.last_position.is_some_and(|old| old != position) {
            self.suppress_popup();
        }
        self.last_position = Some(position);
    }
    fn apply_snapshot(
        &mut self,
        snapshot: InteractionSnapshot,
        allowed: bool,
        main_front: bool,
    ) -> bool {
        let fresh_epoch = self.snapshot.epoch != snapshot.epoch;
        if self.retired_epochs.contains(&snapshot.epoch) {
            return false;
        }
        if !fresh_epoch && snapshot.revision < self.snapshot.revision {
            return false;
        }
        if fresh_epoch {
            if !self.snapshot.epoch.is_empty() {
                self.retired_epochs.insert(self.snapshot.epoch.clone());
            }
            self.seen.clear();
            self.open = false;
            self.selected = None;
            self.initialized = false;
            self.auto_candidate = None;
        }
        let candidate_gone = self
            .auto_candidate
            .as_ref()
            .is_some_and(|(key, _)| !snapshot.requests.iter().any(|r| &r.key == key));
        if candidate_gone {
            self.auto_candidate = None;
        }
        let new = snapshot
            .requests
            .iter()
            .find(|r| {
                let deferred_for_main_front = main_front && self.main_visible.contains(&r.key);
                !(self.seen.contains(&r.key) || deferred_for_main_front)
            })
            .map(|r| r.key.clone());
        if self.initialized && allowed && !self.open && self.auto_candidate.is_none() {
            if let Some(key) = new {
                self.auto_candidate = Some((key, std::time::Instant::now()));
            }
        }
        self.seen
            .extend(snapshot.requests.iter().map(|r| r.key.clone()));
        if self
            .selected
            .as_ref()
            .is_some_and(|key| !snapshot.requests.iter().any(|r| &r.key == key))
        {
            self.selected = snapshot.requests.first().map(|r| r.key.clone());
            if self.selected.is_none() {
                self.open = false;
            }
        }
        self.snapshot = snapshot;
        self.connected = true;
        self.initialized = true;
        true
    }
}

fn ensure_window(app: &AppHandle, label: &str) -> Result<WebviewWindow, String> {
    if let Some(window) = app.get_webview_window(label) {
        return Ok(window);
    }
    let window = WebviewWindowBuilder::new(
        app,
        label,
        WebviewUrl::App(format!("index.html?surface={label}").into()),
    )
    .title("Astro Tasks")
    .inner_size(166.0, 38.0)
    .decorations(false)
    .transparent(true)
    .shadow(true)
    .resizable(false)
    .skip_taskbar(true)
    .always_on_top(true)
    .focused(false)
    .focusable(false)
    .accept_first_mouse(true)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;
    #[cfg(target_os = "macos")]
    {
        let effect_window = window.clone();
        // A fixed shared radius is a capsule at 38px high, a rounded rectangle
        // when expanded. The same native glass view survives the whole morph.
        let radius = 19.0;
        // NSVisualEffectView must be installed on the main thread. CSS blur alone
        // cannot sample pixels from the desktop behind a transparent WebView.
        window.run_on_main_thread(move || {
            if let Err(error) = window_vibrancy::apply_vibrancy(
                &effect_window,
                window_vibrancy::NSVisualEffectMaterial::Popover,
                Some(window_vibrancy::NSVisualEffectState::Active),
                Some(radius),
            ) {
                tracing::warn!(%error, "pet task vibrancy unavailable; keeping readable fallback");
            }
        }).map_err(|e| e.to_string())?;
    }
    Ok(window)
}

fn bounded_popup_height(height: f64) -> Result<f64, String> {
    if !height.is_finite() {
        return Err("无效的弹窗高度".into());
    }
    Ok(height.ceil().clamp(POPUP_MIN_HEIGHT, POPUP_MAX_HEIGHT))
}

#[tauri::command]
pub fn resize_pet_task_content(
    window: WebviewWindow,
    app: AppHandle,
    height: f64,
) -> Result<(), String> {
    if window.label() != POPUP {
        return Err("Only the task popup may report its content size".into());
    }
    app.state::<PetTasks>().0.lock().unwrap().popup_height = Some(bounded_popup_height(height)?);
    Ok(())
}

/// Physical-coordinate placement; preserves scale on negative-origin monitors.
fn place(
    pet: (i32, i32, u32, u32),
    area: (i32, i32, u32, u32),
    wanted: (u32, u32),
    gap: i32,
) -> (i32, i32, u32, u32) {
    let width = wanted.0.min(area.2.saturating_sub((gap * 2) as u32)).max(1);
    let height = wanted.1.min(area.3.saturating_sub((gap * 2) as u32)).max(1);
    let max_x = area.0 + area.2 as i32 - width as i32 - gap;
    let right = pet.0 + pet.2 as i32 + gap;
    let x = if right <= max_x {
        right
    } else {
        pet.0 - width as i32 - gap
    };
    let min_x = (area.0 + gap).min(max_x);
    let max_y = area.1 + area.3 as i32 - height as i32 - gap;
    let min_y = (area.1 + gap).min(max_y);
    (
        x.clamp(min_x, max_x),
        (pet.1 + pet.3 as i32 - height as i32).clamp(min_y, max_y),
        width,
        height,
    )
}
fn morph_frames(g: Geometry, height: f64) -> (Rect, Rect, bool) {
    let gap = (8.0 * g.scale) as i32;
    let panel = Rect::from_tuple(place(
        g.pet,
        g.area,
        ((POPUP_WIDTH * g.scale) as u32, (height * g.scale) as u32),
        gap,
    ));
    let mut badge = Rect::from_tuple(place(
        g.pet,
        g.area,
        ((166.0 * g.scale) as u32, (38.0 * g.scale) as u32),
        gap,
    ));
    let left = panel.x < g.pet.0 as f64;
    badge.x = if left {
        panel.x + panel.width - badge.width
    } else {
        panel.x
    };
    (badge, panel, left)
}

// One NSWindow frame transaction avoids a resize followed by a visibly separate
// reposition. Use Tauri's current top-left coordinates to recover AppKit's origin.
fn apply_frame(window: &WebviewWindow, rect: Rect, scale: f64) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let origin = window.outer_position().map_err(|e| e.to_string())?;
        let current_scale = window.scale_factor().map_err(|e| e.to_string())?;
        let target = window.clone();
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        window
            .run_on_main_thread(move || {
                let result = (|| {
                    let ptr = target.ns_window().map_err(|e| e.to_string())?;
                    if ptr.is_null() {
                        return Err("Task window is closed".to_string());
                    }
                    // SAFETY: Tauri owns this NSWindow and this closure runs on the
                    // AppKit main thread. The handle is not retained outside it.
                    let native = unsafe { &*(ptr as *const objc2_app_kit::NSWindow) };
                    let mut frame = native.frame();
                    let top = frame.origin.y + frame.size.height + origin.y as f64 / current_scale;
                    frame.origin.x = rect.x / scale;
                    frame.origin.y = top - (rect.y + rect.height) / scale;
                    frame.size.width = rect.width / scale;
                    frame.size.height = rect.height / scale;
                    native.setFrame_display(frame, true);
                    Ok(())
                })();
                let _ = tx.send(result);
            })
            .map_err(|e| e.to_string())?;
        rx.recv_timeout(std::time::Duration::from_millis(250))
            .map_err(|e| e.to_string())?
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = scale;
        window
            .set_size(PhysicalSize::new(
                rect.width.round() as u32,
                rect.height.round() as u32,
            ))
            .map_err(|e| e.to_string())?;
        window
            .set_position(PhysicalPosition::new(
                rect.x.round() as i32,
                rect.y.round() as i32,
            ))
            .map_err(|e| e.to_string())
    }
}
fn refresh_windows(app: &AppHandle) -> Result<(), String> {
    let Ok(_refresh) = WINDOW_REFRESH.try_lock() else {
        return Ok(());
    };
    let holder = app.state::<PetTasks>();
    let mut render = holder.1.lock().unwrap();
    let now = std::time::Instant::now();
    let dt = render
        .tick
        .replace(now)
        .map(|last| now.duration_since(last).as_secs_f64())
        .unwrap_or(0.016);
    // Native visibility/topology polling remains at 4Hz; only the frame driver
    // runs at display cadence. No task content is published on animation ticks.
    let polled = render
        .checked
        .is_none_or(|at| now.duration_since(at) >= std::time::Duration::from_millis(250));
    if polled {
        render.checked = Some(now);
        let mut geometry = None;
        let mut visible = false;
        if let Some(pet) = app.get_webview_window("desktop-pet") {
            visible = pet_visible(app);
            if let (Ok(pos), Ok(size), Ok(Some(monitor))) = (
                pet.outer_position(),
                pet.outer_size(),
                pet.current_monitor(),
            ) {
                let area = monitor.work_area();
                geometry = Some(Geometry {
                    pet: (pos.x, pos.y, size.width, size.height),
                    area: (
                        area.position.x,
                        area.position.y,
                        area.size.width,
                        area.size.height,
                    ),
                    scale: monitor.scale_factor(),
                });
            }
        }
        visible &= geometry.is_some();
        let main_front = app
            .get_webview_window("main")
            .is_some_and(|w| w.is_visible().unwrap_or(false) && w.is_focused().unwrap_or(false));
        let mut state = holder.0.lock().unwrap();
        let was_open = state.open;
        if let Some(g) = geometry {
            state.update_anchor(visible, (g.pet.0, g.pet.1));
            if render
                .geometry
                .is_some_and(|old| old.pet != g.pet || old.area != g.area || old.scale != g.scale)
            {
                state.suppress_popup();
            }
        } else {
            state.suppress_popup();
        }
        if state
            .auto_candidate
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= std::time::Duration::from_millis(200))
        {
            let (key, _) = state.auto_candidate.take().unwrap();
            if visible
                && state.connected
                && !state.open
                && !(main_front && state.main_visible.contains(&key))
                && state.snapshot.requests.iter().any(|r| r.key == key)
            {
                state.selected = Some(key);
                state.open = true;
            }
        }
        let changed = was_open != state.open;
        drop(state);
        if changed {
            publish(app);
        }
        if !visible
            || render.geometry.is_some_and(|old| {
                geometry
                    .is_none_or(|g| old.pet != g.pet || old.area != g.area || old.scale != g.scale)
            })
        {
            render.last_rect = None;
            render.motion = Morph::default();
        }
        render.visible = visible;
        render.geometry = geometry;
    }
    let (expanded, height, reduced, has_tasks) = {
        let state = holder.0.lock().unwrap();
        (
            state.open,
            state.popup_height.unwrap_or(420.0),
            state.reduced_motion,
            !state.snapshot.tasks.is_empty() || !state.snapshot.requests.is_empty(),
        )
    };
    if !polled && !render.animating && render.last_goal == Some((expanded, height, reduced)) {
        return Ok(());
    }
    let was_expanded = render.last_goal.is_some_and(|goal| goal.0);
    render.last_goal = Some((expanded, height, reduced));
    if !render.visible || render.geometry.is_none() {
        if let Some(window) = app.get_webview_window(POPUP) {
            if window.is_visible().unwrap_or(false) {
                let _ = window.hide();
                let _ = window.set_focusable(false);
            }
        }
        return Ok(());
    }
    let g = render.geometry.unwrap();
    let (badge, panel, left) = morph_frames(g, height);
    let window = ensure_window(app, POPUP)?;
    if render.last_rect.is_none() {
        render.motion.snap(badge, false);
    }
    render.animating =
        render
            .motion
            .step(if expanded { panel } else { badge }, expanded, dt, reduced);
    if !expanded && was_expanded {
        let _ = window.set_focusable(false);
    }
    if !has_tasks && !expanded && !render.animating {
        if window.is_visible().unwrap_or(false) {
            let _ = window.hide();
        }
        return Ok(());
    }
    let rect = render.motion.rect;
    let rect_changed = render.last_rect != Some(rect);
    if rect_changed {
        apply_frame(&window, rect, g.scale)?;
        render.last_rect = Some(rect);
    }
    if rect_changed
        || render.last_progress != Some(render.motion.progress)
        || !window.is_visible().unwrap_or(false)
    {
        let progress = render.motion.progress.clamp(0.0, 1.0);
        render.sequence = render.sequence.saturating_add(1);
        let _=window.emit("pet-task-morph-frame",serde_json::json!({"sequence":render.sequence,"progress":progress,"left":left,"width":rect.width/g.scale,"height":rect.height/g.scale,"contentWidth":panel.width/g.scale,"contentHeight":panel.height/g.scale}));
        render.last_progress = Some(render.motion.progress);
    }
    if !window.is_visible().unwrap_or(false) {
        window.set_focusable(false).map_err(|e| e.to_string())?;
        window.show().map_err(|e| e.to_string())?;
    }
    Ok(())
}
#[tauri::command]
pub fn get_pending_interactions(app: AppHandle) -> DesktopInteractions {
    dto(&app.state::<PetTasks>().0.lock().unwrap())
}
#[tauri::command]
pub async fn get_pet_task_morph_frame(app: AppHandle) -> serde_json::Value {
    let holder = app.state::<PetTasks>();
    let height = holder.0.lock().unwrap().popup_height.unwrap_or(420.0);
    let render = holder.1.lock().unwrap();
    let scale = render.geometry.map_or(1.0, |g| g.scale);
    let panel = render
        .geometry
        .map(|g| morph_frames(g, height).1)
        .unwrap_or(Rect {
            width: 392.0,
            height: 420.0,
            ..Rect::default()
        });
    serde_json::json!({"sequence":render.sequence,"progress":render.motion.progress,"left":render.geometry.is_some_and(|g|render.motion.rect.x<g.pet.0 as f64),"width":render.motion.rect.width/scale,"height":render.motion.rect.height/scale,"contentWidth":panel.width/scale,"contentHeight":panel.height/scale})
}
#[tauri::command]
pub async fn respond_pending_interaction(
    window: WebviewWindow,
    app: AppHandle,
    request: InteractionResponse,
) -> Result<DesktopInteractions, String> {
    if !["main", POPUP].contains(&window.label()) {
        return Err("此窗口不能处理交互请求".into());
    }
    if !app.state::<PetTasks>().0.lock().unwrap().connected {
        return Err("连接中断，请等待重新连接".into());
    }
    let mut client = AstroServiceClient::connect(endpoint_url(&default_grpc_address()))
        .await
        .map_err(|e| e.to_string())?;
    let result = match client
        .respond_pending_interaction(proto::PendingInteractionsJson {
            json: serde_json::to_string(&request).map_err(|e| e.to_string())?,
        })
        .await
    {
        Ok(result) => result,
        Err(error) => {
            if let Ok(current) = client.get_pending_interactions(proto::Empty {}).await {
                if let Ok(snapshot) = serde_json::from_str(&current.into_inner().json) {
                    receive(&app, snapshot);
                }
            }
            return Err(error.message().to_string());
        }
    };
    receive(
        &app,
        serde_json::from_str(&result.into_inner().json).map_err(|e| e.to_string())?,
    );
    Ok(get_pending_interactions(app))
}
#[tauri::command]
pub async fn open_pet_tasks(app: AppHandle, request_key: Option<String>) -> Result<(), String> {
    {
        let holder = app.state::<PetTasks>();
        let mut state = holder.0.lock().unwrap();
        state.auto_candidate = None;
        state.open = true;
        state.selected =
            request_key.filter(|key| state.snapshot.requests.iter().any(|r| &r.key == key));
    }
    publish(&app);
    refresh_windows(&app)
}
#[tauri::command]
pub async fn dismiss_pet_tasks(app: AppHandle) {
    let holder = app.state::<PetTasks>();
    {
        let mut state = holder.0.lock().unwrap();
        state.suppress_popup();
    }
    publish(&app);
    let _ = refresh_windows(&app);
}
#[tauri::command]
pub async fn focus_pet_tasks(window: WebviewWindow, app: AppHandle) -> Result<(), String> {
    if window.label() != POPUP {
        return Err("Only the interaction popup may request input focus".into());
    }
    if !app.state::<PetTasks>().0.lock().unwrap().open {
        return Err("Task popup is collapsed".into());
    }
    window.set_focusable(true).map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}
#[tauri::command]
pub fn set_pet_task_motion_preference(
    window: WebviewWindow,
    app: AppHandle,
    reduced: bool,
) -> Result<(), String> {
    if window.label() != POPUP {
        return Err("Only the task surface may set motion preference".into());
    }
    app.state::<PetTasks>().0.lock().unwrap().reduced_motion = reduced;
    Ok(())
}
#[tauri::command]
pub fn report_pending_interactions_visible(
    window: WebviewWindow,
    app: AppHandle,
    keys: Vec<String>,
) -> Result<(), String> {
    if window.label() != "main" {
        return Err("Only the main window may report visible requests".into());
    }
    app.state::<PetTasks>().0.lock().unwrap().main_visible = keys.into_iter().collect();
    Ok(())
}
#[tauri::command]
pub async fn open_pet_task_session(
    app: AppHandle,
    session_id: String,
    request_key: Option<String>,
) -> Result<(), String> {
    let navigation = {
        let holder = app.state::<PetTasks>();
        let mut state = holder.0.lock().unwrap();
        if !state
            .snapshot
            .tasks
            .iter()
            .any(|t| t.session_id == session_id)
        {
            return Err("任务已结束，请从会话历史打开".into());
        }
        let item = request_key.and_then(|key| {
            state
                .snapshot
                .requests
                .iter()
                .find(|r| r.key == key && r.session_id == session_id)
                .map(|r| r.tool_call_id.clone())
        });
        let nav = serde_json::json!({"id":uuid::Uuid::new_v4().to_string(),"sessionId":session_id,"itemId":item});
        state.navigation = Some(nav.clone());
        state.suppress_popup();
        nav
    };
    let _ = refresh_windows(&app);
    crate::ui::tray::show_main_window(&app);
    publish(&app);
    app.emit_to("main", "pet-task-open-session", navigation)
        .map_err(|e| e.to_string())
}
#[tauri::command]
pub fn take_pet_task_navigation(
    window: WebviewWindow,
    app: AppHandle,
) -> Option<serde_json::Value> {
    if window.label() != "main" {
        return None;
    }
    app.state::<PetTasks>().0.lock().unwrap().navigation.take()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn snapshot(revision: u64, ids: &[&str]) -> InteractionSnapshot {
        InteractionSnapshot {
            epoch: "test".into(),
            revision,
            tasks: vec![],
            requests: ids
                .iter()
                .map(|id| types::pending_interaction::PendingInteraction {
                    key: (*id).into(),
                    session_id: "session".into(),
                    turn_id: "turn".into(),
                    request_id: (*id).into(),
                    tool_call_id: "call".into(),
                    kind: "question".into(),
                    message: "question".into(),
                    operations: serde_json::Value::Null,
                    response_schema: serde_json::Value::Null,
                    actions: vec![],
                    expires_at: String::new(),
                    server_name: None,
                    generation: None,
                })
                .collect(),
        }
    }
    #[test]
    fn popup_policy_preserves_editing_deduplicates_and_does_not_replay_suppressed_requests() {
        let mut state = State::default();
        state.apply_snapshot(snapshot(0, &[]), true, false);
        state.apply_snapshot(snapshot(1, &["a"]), false, false);
        assert!(state.auto_candidate.is_none());
        state.apply_snapshot(snapshot(1, &["a"]), true, false);
        assert!(state.auto_candidate.is_none());
        state.main_visible.insert("b".into());
        state.apply_snapshot(snapshot(2, &["a", "b"]), true, true);
        assert!(state.auto_candidate.is_none());
        state.apply_snapshot(snapshot(3, &["a", "b", "c"]), true, false);
        assert_eq!(
            state.auto_candidate.as_ref().map(|(id, _)| id.as_str()),
            Some("c")
        );
        state.open = true;
        state.selected = Some("a".into());
        state.auto_candidate = None;
        state.apply_snapshot(snapshot(4, &["a", "b", "c", "d"]), true, false);
        assert_eq!(state.selected.as_deref(), Some("a"));
        assert!(state.auto_candidate.is_none());
        assert!(!state.apply_snapshot(snapshot(3, &[]), true, false));
        state.apply_snapshot(snapshot(5, &["b", "c", "d"]), true, false);
        assert_eq!(state.selected.as_deref(), Some("b"));
    }
    #[test]
    fn moving_or_hiding_clears_a_scheduled_popup_and_surfaces_never_overlap() {
        let mut state = State::default();
        state.apply_snapshot(snapshot(0, &[]), true, false);
        state.update_anchor(true, (10, 10));
        state.apply_snapshot(snapshot(1, &["request"]), true, false);
        assert!(state.auto_candidate.is_some());
        state.update_anchor(true, (11, 10));
        assert!(state.auto_candidate.is_none());
        assert!(!state.open);
        state.open = true;
        assert!(state.open);
        state.update_anchor(false, (11, 10));
        assert!(!state.open);
    }
    #[test]
    fn expiring_a_new_candidate_does_not_replay_a_snoozed_request() {
        let mut state = State::default();
        state.apply_snapshot(snapshot(0, &["snoozed"]), true, false);
        state.apply_snapshot(snapshot(1, &["snoozed", "new"]), true, false);
        assert!(state.auto_candidate.is_some());
        state.apply_snapshot(snapshot(2, &["snoozed"]), true, false);
        assert!(state.auto_candidate.is_none());
        assert!(!state.open);
    }
    #[test]
    fn native_snooze_retains_all_requests_and_selection() {
        let mut state = State::default();
        state.apply_snapshot(snapshot(1, &["a", "b"]), true, false);
        state.open = true;
        state.selected = Some("a".into());
        let requests = state.snapshot.requests.clone();
        state.suppress_popup();
        assert!(!state.open);
        assert_eq!(state.snapshot.requests, requests);
        assert_eq!(state.selected.as_deref(), Some("a"));
    }
    #[test]
    fn capsule_uses_the_panels_side_even_when_only_capsule_fits_on_the_right() {
        let g = Geometry {
            pet: (850, 500, 120, 136),
            area: (0, 0, 1200, 900),
            scale: 1.0,
        };
        let (badge, panel, left) = morph_frames(g, 420.0);
        assert!(left);
        assert_eq!(badge.x + badge.width, panel.x + panel.width);
        assert!(panel.x + panel.width < 850.0);
    }
    #[test]
    fn task_popup_content_size_is_bounded_and_rejects_nonfinite_values() {
        assert_eq!(bounded_popup_height(20.0).unwrap(), 180.0);
        assert_eq!(bounded_popup_height(321.2).unwrap(), 322.0);
        assert_eq!(bounded_popup_height(2000.0).unwrap(), 560.0);
        assert!(bounded_popup_height(f64::NAN).is_err());
        assert!(bounded_popup_height(f64::INFINITY).is_err());
    }
    #[test]
    fn popup_placement_clamps_negative_monitor_origins_and_avoids_pet() {
        let (x, y, w, h) = place((-150, 400, 100, 120), (-1920, 0, 1920, 1080), (440, 560), 8);
        assert!(x + w as i32 <= -150 && x >= -1912 && y >= 8 && y + h as i32 <= 1072);
        let (x, _, _, _) = place((10, 100, 100, 120), (0, 0, 1920, 1080), (166, 38), 8);
        assert_eq!(x, 118);
    }
}
