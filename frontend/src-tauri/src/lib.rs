//! Astro Agent Tauri 库：菜单、窗口与 `invoke` 命令注册入口。

#![allow(unexpected_cfgs)] // 旧版 objc 的 msg_send!/sel! 使用 cfg(feature = "cargo-clippy")

mod artifacts_commands;
mod auxiliary_commands;
mod auxiliary_resolver;
mod clipboard_files;
mod commands;
mod compaction_commands;
mod config_commands;
mod app_icon;
mod dreaming_commands;
mod env_hydrate;
mod evolution_commands;
mod evolution_run_commands;
mod icon_commands;
mod fs_ops;
mod grpc;
mod ip_location;
mod keystore;
mod litellm_meta;
mod memory_commands;
mod menu_locale;
mod model_meta;
mod notify;
mod providers_commands;
mod session_events;
mod skills_commands;
mod tray;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use menu_locale::AppLocale;
use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID},
    window::Color,
    AppHandle, Emitter, Manager, RunEvent, WebviewWindowBuilder,
};

/// 原生窗全透明；内容由 CSS 铺满。macOS 用系统装饰 + Overlay 标题栏（红绿灯）。
/// 与 light/blue underlay 一致；勿用全透明，否则 zoom 不同步时会露白边（tauri#13898）。
const BG: Color = Color(0xdb, 0xea, 0xfe, 0xff);

/// 原生菜单「偏好设置」项 id。
const MENU_PREFERENCES_ID: &str = "preferences";
/// 原生菜单「关于」项 id（打开应用内 About 对话框）。
const MENU_ABOUT_ID: &str = "about";
/// 前端监听的「打开偏好设置」事件名。
const EVENT_OPEN_PREFERENCES: &str = "open-preferences";
/// 前端监听的「打开关于」事件名。
const EVENT_OPEN_ABOUT: &str = "open-about";

/// 为 true 时允许窗口真正关闭（退出流程）；否则关窗只隐藏到托盘。
static ALLOW_EXIT: AtomicBool = AtomicBool::new(false);

/// 真正退出应用（托盘 / 菜单「退出」）。
pub fn request_app_exit<R: tauri::Runtime>(app: &AppHandle<R>) {
    ALLOW_EXIT.store(true, Ordering::SeqCst);
    app.exit(0);
}

/// 安装应用菜单（关于、偏好设置、窗口与帮助），文案随 [`AppLocale`]。
fn install_app_menu<R: tauri::Runtime>(app: &AppHandle<R>, locale: AppLocale) -> tauri::Result<()> {
    let s = locale.strings();
    let about = MenuItem::with_id(app, MENU_ABOUT_ID, s.about, true, None::<&str>)?;

    let preferences = MenuItem::with_id(
        app,
        MENU_PREFERENCES_ID,
        s.preferences,
        true,
        Some("CmdOrCtrl+,"),
    )?;

    let window_menu = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        s.submenu_window,
        true,
        &[
            &PredefinedMenuItem::minimize(app, Some(s.minimize))?,
            &PredefinedMenuItem::maximize(app, Some(s.maximize))?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, Some(s.close_window))?,
        ],
    )?;

    let help_menu = Submenu::with_id_and_items(
        app,
        HELP_SUBMENU_ID,
        s.submenu_help,
        true,
        &[
            #[cfg(not(target_os = "macos"))]
            &about,
            #[cfg(not(target_os = "macos"))]
            &PredefinedMenuItem::separator(app)?,
            #[cfg(not(target_os = "macos"))]
            &preferences,
        ],
    )?;

    let menu = Menu::with_items(
        app,
        &[
            #[cfg(target_os = "macos")]
            &Submenu::with_items(
                app,
                "Astro",
                true,
                &[
                    &about,
                    &PredefinedMenuItem::separator(app)?,
                    &preferences,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, Some(s.services))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, Some(s.hide))?,
                    &PredefinedMenuItem::hide_others(app, Some(s.hide_others))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, Some(s.quit))?,
                ],
            )?,
            #[cfg(not(any(
                target_os = "linux",
                target_os = "dragonfly",
                target_os = "freebsd",
                target_os = "netbsd",
                target_os = "openbsd"
            )))]
            &Submenu::with_items(
                app,
                s.submenu_file,
                true,
                &[
                    &PredefinedMenuItem::close_window(app, Some(s.close_window))?,
                    #[cfg(not(target_os = "macos"))]
                    &PredefinedMenuItem::quit(app, Some(s.quit))?,
                ],
            )?,
            &Submenu::with_items(
                app,
                s.submenu_edit,
                true,
                &[
                    &PredefinedMenuItem::undo(app, Some(s.undo))?,
                    &PredefinedMenuItem::redo(app, Some(s.redo))?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, Some(s.cut))?,
                    &PredefinedMenuItem::copy(app, Some(s.copy))?,
                    &PredefinedMenuItem::paste(app, Some(s.paste))?,
                    &PredefinedMenuItem::select_all(app, Some(s.select_all))?,
                ],
            )?,
            #[cfg(target_os = "macos")]
            &Submenu::with_items(
                app,
                s.submenu_view,
                true,
                &[&PredefinedMenuItem::fullscreen(app, Some(s.fullscreen))?],
            )?,
            &window_menu,
            &help_menu,
        ],
    )?;

    app.set_menu(menu)?;
    Ok(())
}

/// 前端语言切换时重建菜单栏与托盘文案。
#[tauri::command]
fn set_app_menu_locale(app: AppHandle, locale: String) -> Result<(), String> {
    let next = AppLocale::parse(&locale);
    common::set_notify_locale(match next {
        AppLocale::En => "en",
        AppLocale::Zh => "zh",
    });
    if let Some(state) = app.try_state::<Mutex<AppLocale>>() {
        let mut cur = state.lock().map_err(|e| e.to_string())?;
        if *cur == next {
            return Ok(());
        }
        *cur = next;
    } else {
        app.manage(Mutex::new(next));
    }
    install_app_menu(&app, next).map_err(|e| e.to_string())?;
    tray::apply_tray_locale(&app, next).map_err(|e| e.to_string())?;
    Ok(())
}

/// macOS：吞掉原生 `zoom:`（标题栏双击白边根因，见 tauri#13898 / tao#1207）。
/// 底色保持不透明 underlay，勿清成 clearColor（露白边）。
#[cfg(target_os = "macos")]
fn configure_macos_window(win: &tauri::WebviewWindow) {
    use objc::runtime::{Class, Object, Sel};
    use objc::{msg_send, sel, sel_impl};
    use std::os::raw::c_void;
    use std::sync::Once;

    type Imp = unsafe extern "C" fn(*mut Object, Sel, *mut Object);

    #[link(name = "objc")]
    extern "C" {
        fn class_getInstanceMethod(cls: *const Class, name: Sel) -> *mut c_void;
        fn method_setImplementation(method: *mut c_void, imp: Imp) -> Imp;
    }

    unsafe extern "C" fn ns_window_zoom_noop(_this: *mut Object, _cmd: Sel, _sender: *mut Object) {}

    static PATCH_ZOOM: Once = Once::new();
    PATCH_ZOOM.call_once(|| unsafe {
        if let Some(cls) = Class::get("NSWindow") {
            let method = class_getInstanceMethod(cls, sel!(zoom:));
            if !method.is_null() {
                let _ = method_setImplementation(method, ns_window_zoom_noop);
            }
        }
    });

    if let Ok(ns_window) = win.ns_window() {
        unsafe {
            let ns_window = ns_window as *mut Object;
            // NSWindowAnimationBehaviorNone = 0：禁止系统缩放动画（WebView 跟不上）
            let _: () = msg_send![ns_window, setAnimationBehavior: 0i64];

            let content_view: *mut Object = msg_send![ns_window, contentView];
            if content_view.is_null() {
                return;
            }
            let subviews: *mut Object = msg_send![content_view, subviews];
            if subviews.is_null() {
                return;
            }
            let flexible: usize = 2 | 16; // WidthSizable | HeightSizable
            let count: usize = msg_send![subviews, count];
            for i in 0..count {
                let child: *mut Object = msg_send![subviews, objectAtIndex: i];
                if child.is_null() {
                    continue;
                }
                let _: () = msg_send![child, setAutoresizingMask: flexible];
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// 启动 Tauri 应用（菜单、窗口与插件）。
pub fn run() {
    if let Err(err) = home::init_logging("agent") {
        eprintln!("frontend logging init failed: {err}");
    }

    // Finder / Dock 启动的 .app 没有终端 shell 环境；补载 ~/.astro/.env 与登录 shell 中的 API Key。
    env_hydrate::hydrate_process_env();

    tauri::Builder::default()
        // 单实例须最先注册：二次启动聚焦已有窗口（Windows/Linux；macOS 另见 Reopen）。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .on_menu_event(|app, event| {
            if event.id() == MENU_PREFERENCES_ID {
                let _ = app.emit(EVENT_OPEN_PREFERENCES, ());
                tray::show_main_window(app);
            } else if event.id() == MENU_ABOUT_ID {
                let _ = app.emit(EVENT_OPEN_ABOUT, ());
                tray::show_main_window(app);
            }
        })
        // 关窗 → 进托盘；真正退出见 [`request_app_exit`] / ExitRequested。
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if !ALLOW_EXIT.load(Ordering::SeqCst) {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            set_app_menu_locale,
            commands::start_chat,
            commands::chat_control,
            commands::interrupt_resume,
            commands::generate_image,
            commands::query_memory,
            memory_commands::refresh_memory,
            memory_commands::list_pending_memory_writes,
            memory_commands::approve_pending_memory_write,
            memory_commands::reject_pending_memory_write,
            memory_commands::approve_all_pending_memory_writes,
            memory_commands::reject_all_pending_memory_writes,
            memory_commands::get_memory_settings,
            memory_commands::set_memory_write_approval,
            memory_commands::set_memory_auto_refresh,
            memory_commands::set_background_review_enabled,
            session_events::set_session_events_filter,
            commands::get_chat_history,
            commands::fork_chat_session,
            commands::remove_chat_bubbles,
            commands::list_recent_sessions,
            commands::list_sessions,
            commands::rename_session,
            commands::regenerate_session_title,
            commands::archive_session,
            commands::unarchive_session,
            commands::pin_session,
            commands::unpin_session,
            commands::delete_session_permanently,
            commands::list_files,
            commands::read_file,
            commands::open_path_externally,
            commands::reveal_in_folder,
            commands::trash_paths,
            commands::read_file_base64,
            commands::read_user_file_base64,
            commands::download_file_to_downloads,
            commands::download_bytes_to_downloads,
            commands::copy_paths_to_clipboard,
            commands::list_clipboard_file_paths,
            commands::paste_paths_from_clipboard,
            commands::write_file,
            commands::create_file,
            commands::create_directory,
            commands::rename_path,
            commands::copy_paths,
            commands::move_paths,
            commands::delete_path,
            commands::get_config,
            commands::list_agents,
            commands::create_agent,
            commands::set_active_agent,
            commands::set_pending_agent_icon,
            commands::clear_pending_agent_icon,
            commands::list_daily_memory,
            commands::read_daily_memory,
            commands::write_daily_memory,
            commands::list_cron_jobs,
            commands::extract_cron_job,
            commands::add_cron_job,
            commands::update_cron_job,
            commands::remove_cron_job,
            commands::set_cron_job_enabled,
            commands::run_cron_job_now,
            commands::list_cron_runs,
            commands::list_cron_job_runs,
            config_commands::get_tools_enabled,
            config_commands::set_tools_enabled,
            config_commands::get_tool_catalog,
            config_commands::get_mcp_servers,
            config_commands::set_mcp_servers,
            config_commands::refresh_mcp_tools,
            config_commands::get_agent_usage_stats,
            config_commands::get_usage_insights,
            config_commands::get_collaboration_insights,
            config_commands::get_trace_insights,
            config_commands::query_agent_logs,
            providers_commands::get_providers_state,
            providers_commands::list_providers,
            providers_commands::add_provider,
            providers_commands::save_provider,
            providers_commands::reorder_providers,
            providers_commands::delete_provider,
            providers_commands::set_active_provider,
            providers_commands::set_provider_api_key,
            providers_commands::get_provider_api_key,
            providers_commands::clear_provider_api_key,
            providers_commands::list_provider_models,
            providers_commands::get_cached_provider_models,
            providers_commands::test_provider,
            providers_commands::test_provider_models,
            skills_commands::list_installed_skills,
            skills_commands::search_store_skills,
            skills_commands::get_store_skill_detail,
            skills_commands::set_skill_enabled,
            skills_commands::link_machine_skill,
            skills_commands::install_store_skill,
            skills_commands::get_skill_content,
            skills_commands::list_skill_bundle,
            skills_commands::get_skill_file,
            skills_commands::open_skill_folder,
            skills_commands::reveal_skill_file,
            skills_commands::open_skill_file,
            skills_commands::list_skill_origins,
            skills_commands::preview_skill_update,
            skills_commands::update_installed_skill,
            skills_commands::check_skill_updates,
            skills_commands::update_all_skills,
            skills_commands::list_skill_backups,
            skills_commands::reveal_skill_backup,
            artifacts_commands::list_artifacts,
            artifacts_commands::reconcile_artifacts,
            artifacts_commands::register_artifact,
            artifacts_commands::save_chat_upload,
            artifacts_commands::remove_artifacts_by_paths,
            dreaming_commands::get_dreaming_status,
            dreaming_commands::set_dreaming_enabled_cmd,
            dreaming_commands::run_dreaming,
            compaction_commands::compact_chat_session,
            ip_location::infer_ip_location,
            auxiliary_commands::get_auxiliary_settings,
            auxiliary_commands::set_auxiliary_route,
            auxiliary_commands::reset_auxiliary_route,
            auxiliary_commands::reset_all_auxiliary_routes,
            evolution_commands::get_evolution_settings,
            evolution_commands::set_evolution_enabled,
            evolution_commands::set_evolution_route,
            evolution_commands::reset_evolution_route,
            evolution_commands::set_evolution_gates,
            evolution_run_commands::run_evolution,
            evolution_run_commands::list_evolution_proposals,
            evolution_run_commands::approve_evolution_proposal,
            evolution_run_commands::reject_evolution_proposal,
            icon_commands::get_app_icon,
            icon_commands::set_app_icon,
        ])
        .setup(|app| {
            if let Err(err) = memory::ensure_default_workspace() {
                tracing::warn!("workspace bootstrap failed: {err}");
            }

            // 默认同进程内嵌 gRPC；ASTRO_EMBED_BACKEND=0 时连外部 backend。
            // 未设 ASTRO_GRPC_ADDR 时 bind 127.0.0.1:0，实际端口经 oneshot + 进程内地址共享。
            if grpc::embed_backend_enabled() {
                let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = backend::run_embedded(Some(ready_tx)).await {
                        tracing::error!(
                            error = %err,
                            "embedded backend exited (port in use? set ASTRO_GRPC_ADDR or ASTRO_EMBED_BACKEND=0)"
                        );
                    }
                });
                match tauri::async_runtime::block_on(async {
                    tokio::time::timeout(std::time::Duration::from_secs(5), ready_rx).await
                }) {
                    Ok(Ok(addr)) => {
                        tracing::info!("embedded gRPC listening on {addr}");
                    }
                    Ok(Err(_)) => {
                        tracing::error!(
                            "embedded backend failed before listen; chat may be unavailable"
                        );
                    }
                    Err(_) => {
                        tracing::warn!(
                            "embedded backend bind not confirmed within 5s; session bridge will keep retrying"
                        );
                    }
                }
            } else {
                tracing::info!(
                    "ASTRO_EMBED_BACKEND disabled; expecting external backend at {}",
                    grpc::default_grpc_address()
                );
            }

            session_events::start_bridge(app.handle());

            app.manage(Mutex::new(AppLocale::Zh));

            if let Err(err) = install_app_menu(app.handle(), AppLocale::Zh) {
                tracing::warn!("app menu install failed: {err}");
            }

            if let Err(err) = tray::install_tray(app.handle(), AppLocale::Zh) {
                tracing::warn!("system tray install failed: {err}");
            }

            // 重放持久化的应用图标（托盘/程序坞/窗口）
            app_icon::apply_app_icon(app.handle(), &app_icon::load_variant());

            notify::install(app.handle());

            let config = app
                .config()
                .app
                .windows
                .first()
                .cloned()
                .expect("missing window config");

            let window = WebviewWindowBuilder::from_config(app.handle(), &config)?
                .background_color(BG)
                .auto_resize()
                .build()?;

            let _ = window.set_title("Astro");
            let _ = window.set_background_color(Some(BG));
            let _ = window.as_ref().set_auto_resize(true);

            #[cfg(target_os = "macos")]
            configure_macos_window(&window);

            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app, event| match event {
            // 菜单「退出」等会走 ExitRequested：允许后续关窗真正销毁。
            RunEvent::ExitRequested { .. } => {
                ALLOW_EXIT.store(true, Ordering::SeqCst);
            }
            // macOS：点 Dock 图标时若窗口已关进托盘，重新显示。
            RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => {
                tray::show_main_window(app);
            }
            _ => {}
        });
}
