//! Ephemeral native locomotion. No frame or resting roam position is persisted.
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex, OnceLock,
};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow};

const PET: &str = "desktop-pet";
static REVISION: AtomicU64 = AtomicU64::new(0);
struct Runtime {
    generation: u64,
    active: bool,
    ready: bool,
    suppress_settle: bool,
    blocked: bool,
    heartbeat_ms: u64,
}
static RUNTIME: Mutex<Runtime> = Mutex::new(Runtime {
    generation: 0,
    active: false,
    ready: false,
    suppress_settle: false,
    blocked: true,
    heartbeat_ms: 0,
});
fn now_ms() -> u64 {
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    EPOCH.get_or_init(Instant::now).elapsed().as_millis() as u64
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RoamingFrame {
    pub generation: u64,
    pub pet_id: Option<String>,
    pub revision: u64,
    pub active: bool,
    pub returning: bool,
    pub clip_name: Option<String>,
    pub clip: Option<types::pet_motion::PetMotionClip>,
    pub elapsed_ms: u64,
}
fn stopped(generation: u64) -> RoamingFrame {
    RoamingFrame {
        generation,
        pet_id: None,
        revision: 0,
        active: false,
        returning: false,
        clip_name: None,
        clip: None,
        elapsed_ms: 0,
    }
}
pub(super) fn state_changed(state: &types::DesktopPetState) {
    REVISION.fetch_max(state.revision, Ordering::Relaxed);
}
pub(super) fn suppress_settle() -> bool {
    RUNTIME.lock().map_or(true, |r| r.suppress_settle)
}
pub(super) fn manual_drag(app: &AppHandle) {
    let frame = {
        let mut r = RUNTIME.lock().unwrap();
        r.generation += 1;
        r.active = false;
        r.suppress_settle = false;
        stopped(r.generation)
    };
    let _ = app.emit_to(PET, "desktop-pet-roaming", frame);
}
fn finish(app: &AppHandle, generation: u64, returning: Option<RoamingFrame>) {
    let frame = {
        let mut r = RUNTIME.lock().unwrap();
        if r.generation != generation {
            return;
        }
        r.active = false;
        r.generation += 1;
        // Remains suppressed until an explicit drag or placement; delayed onMoved must not save roaming.
        if let Some(frame) = returning {
            RoamingFrame {
                generation: r.generation,
                active: false,
                returning: true,
                ..frame
            }
        } else {
            stopped(r.generation)
        }
    };
    let _ = app.emit_to(PET, "desktop-pet-roaming", frame);
}

#[tauri::command]
pub fn report_desktop_pet_roaming_guard(
    window: WebviewWindow,
    blocked: bool,
) -> Result<bool, String> {
    if window.label() != PET {
        return Err("Only the pet may report its roaming guard".into());
    }
    let mut r = RUNTIME.lock().map_err(|_| "Roaming lock unavailable")?;
    r.blocked = blocked;
    r.heartbeat_ms = now_ms();
    Ok(r.suppress_settle)
}

#[tauri::command]
pub fn ready_desktop_pet_roaming(window: WebviewWindow, generation: u64) -> Result<(), String> {
    if window.label() != PET {
        return Err("Only the pet may acknowledge decoded frames".into());
    }
    let mut runtime = RUNTIME.lock().map_err(|_| "Roaming lock unavailable")?;
    if runtime.generation == generation && runtime.active {
        runtime.ready = true;
    }
    Ok(())
}

#[tauri::command]
pub async fn place_desktop_pet_on_ground(
    app: AppHandle,
    window: WebviewWindow,
    pet_id: String,
) -> Result<types::DesktopPetState, String> {
    if !matches!(window.label(), "main" | PET) {
        return Err("Unsupported placement caller".into());
    }
    let base = home::default_memory_dir();
    let before = types::read_desktop_pet_state(&base).map_err(|e| e.to_string())?;
    if before.active_pet_id.as_deref() != Some(&pet_id)
        || before.preferences.position_locked
        || before.sprite_version_number != Some(3)
    {
        return Err("当前宠物已变化或位置已锁定".into());
    }
    for name in ["running-left", "running-right"] {
        let clip = before
            .motion_clips
            .get(name)
            .ok_or("此宠物没有已验收的APNG步态")?;
        super::pet_roaming_plan::validate_walk(clip)?;
    }
    let pet = app.get_webview_window(PET).ok_or("请先显示桌宠")?;
    if !before.enabled || !pet.is_visible().unwrap_or(false) {
        return Err("请先显示桌宠".into());
    }
    let at = pet.outer_position().map_err(|e| e.to_string())?;
    let size = pet.outer_size().map_err(|e| e.to_string())?;
    let logical = super::desktop_pet::window_size_for(&before);
    let (position, _) = super::pet_roaming_plan::ground(
        (at.x, at.y),
        (size.width, size.height),
        &super::desktop_pet::pet_screens(&app),
        (logical.width, logical.height),
    )
    .ok_or("没有可用屏幕")?;
    manual_drag(&app);
    let saved = types::update_desktop_pet_state(&base, |state| {
        anyhow::ensure!(state.revision == before.revision, "宠物设置已变化，请重试");
        state.preferences.position = Some(position);
        state.preferences.roaming_enabled = true;
        Ok(())
    })
    .map_err(|e| e.to_string())?;
    super::desktop_pet::present_committed_state(&app, saved)
}

#[tauri::command]
pub async fn start_desktop_pet_roaming(
    app: AppHandle,
    window: WebviewWindow,
    pet_id: String,
    right: bool,
    cycles: u32,
) -> Result<RoamingFrame, String> {
    if window.label() != PET {
        return Err("Only the pet may start roaming".into());
    }
    let state =
        types::read_desktop_pet_state(&home::default_memory_dir()).map_err(|e| e.to_string())?;
    if state.active_pet_id.as_deref() != Some(&pet_id)
        || state.sprite_version_number != Some(3)
        || !state.enabled
        || !state.preferences.roaming_enabled
        || state.preferences.position_locked
        || state.preferences.quiet_mode
        || state.animation_paused
        || state.preferences.presentation_mode
        || !window.is_visible().unwrap_or(false)
        || super::pet_tasks::blocks_roaming(&app)
        || super::pet_platform::primary_button_down()
        || (state.preferences.hide_in_fullscreen && super::desktop_pet::fullscreen_now(&app))
    {
        return Err("当前状态不允许漫游".into());
    }
    let name = if right {
        "running-right"
    } else {
        "running-left"
    };
    let clip = state.motion_clips.get(name).ok_or("缺少步行动作")?;
    let at = window.outer_position().map_err(|e| e.to_string())?;
    let size = window.outer_size().map_err(|e| e.to_string())?;
    let logical = super::desktop_pet::window_size_for(&state);
    let plan = super::pet_roaming_plan::plan(
        (at.x, at.y),
        (size.width, size.height),
        &super::desktop_pet::pet_screens(&app),
        (logical.width, logical.height),
        clip,
        right,
        cycles,
    )?;
    let generation = {
        let mut r = RUNTIME.lock().map_err(|_| "Roaming lock unavailable")?;
        if r.active || r.blocked || now_ms().saturating_sub(r.heartbeat_ms) > 2000 {
            return Err("漫游已暂停或界面未就绪".into());
        }
        r.generation += 1;
        r.active = true;
        r.ready = false;
        r.suppress_settle = true;
        r.generation
    };
    let first = RoamingFrame {
        generation,
        pet_id: state.active_pet_id.clone(),
        revision: state.revision,
        active: true,
        returning: false,
        clip_name: Some(name.into()),
        clip: Some(plan.clip.clone()),
        elapsed_ms: 0,
    };
    let event = first.clone();
    let _ = app.emit_to(PET, "desktop-pet-roaming", &first);
    tauri::async_runtime::spawn(async move {
        let requested = Instant::now();
        let mut start: Option<Instant> = None;
        let mut last_point = (at.x, at.y);
        let mut completed = false;
        loop {
            let allowed = RUNTIME.lock().is_ok_and(|r| {
                r.generation == generation
                    && r.active
                    && !r.blocked
                    && now_ms().saturating_sub(r.heartbeat_ms) <= 2000
            });
            if !allowed
                || REVISION.load(Ordering::Relaxed) != state.revision
                || !window.is_visible().unwrap_or(false)
                || super::pet_platform::primary_button_down()
                || super::pet_tasks::blocks_roaming(&app)
                || format!("{:?}", super::desktop_pet::pet_screens(&app)) != plan.topology
            {
                break;
            }
            if !window
                .outer_position()
                .is_ok_and(|position| (position.x, position.y) == last_point)
            {
                break;
            }
            if !RUNTIME.lock().is_ok_and(|r| r.ready) {
                if requested.elapsed().as_secs() >= 5 {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(33)).await;
                continue;
            }
            let elapsed = (start.get_or_insert_with(Instant::now).elapsed().as_millis() as u64)
                .min(plan.duration_ms);
            let point = plan.point(elapsed);
            if window.cursor_position().is_ok_and(|cursor| {
                let padding = 12.0 * window.scale_factor().unwrap_or(1.0);
                cursor.x >= f64::from(point.0) - padding
                    && cursor.x <= f64::from(point.0) + f64::from(size.width) + padding
                    && cursor.y >= f64::from(point.1) - padding
                    && cursor.y <= f64::from(point.1) + f64::from(size.height) + padding
            }) {
                break;
            }
            // Do not hold any state mutex across platform calls/IPC.
            if point != last_point {
                if window
                    .set_position(PhysicalPosition::new(point.0, point.1))
                    .is_err()
                {
                    break;
                }
                last_point = point;
            }
            let _ = app.emit_to(
                PET,
                "desktop-pet-roaming",
                RoamingFrame {
                    elapsed_ms: elapsed,
                    ..event.clone()
                },
            );
            if elapsed >= plan.duration_ms {
                completed = true;
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(33)).await;
        }
        let returning = (!completed
            && start.is_some()
            && REVISION.load(Ordering::Relaxed) == state.revision
            && window.is_visible().unwrap_or(false)
            && !super::pet_platform::primary_button_down()
            && format!("{:?}", super::desktop_pet::pet_screens(&app)) == plan.topology)
            .then(|| RoamingFrame {
                elapsed_ms: plan.entry_ms + plan.moving_ms,
                ..event
            });
        finish(&app, generation, returning);
    });
    Ok(first)
}
