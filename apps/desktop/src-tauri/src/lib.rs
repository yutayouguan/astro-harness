//! Astro Agent Tauri 库：菜单、窗口与 `invoke` 命令注册入口。

#![allow(unexpected_cfgs)] // 旧版 objc 的 msg_send!/sel! 使用 cfg(feature = "cargo-clippy")

mod commands;
mod infra;
mod meta;
mod ui;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem, Submenu, HELP_SUBMENU_ID, WINDOW_SUBMENU_ID},
    window::Color,
    AppHandle, Emitter, Manager, RunEvent, WebviewWindowBuilder,
};
use ui::menu_locale::AppLocale;

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
    types::set_notify_locale(match next {
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
    ui::tray::apply_tray_locale(&app, next).map_err(|e| e.to_string())?;
    Ok(())
}

/// macOS：通过 window-vibrancy 插件启用原生毛玻璃效果。
#[cfg(target_os = "macos")]
fn configure_macos_window(win: &tauri::WebviewWindow) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
    let _ = apply_vibrancy(
        win,
        NSVisualEffectMaterial::UnderWindowBackground,
        None,
        None,
    );
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// 启动 Tauri 应用（菜单、窗口与插件）。
pub fn run() {
    if let Err(err) = home::init_logging("agent") {
        eprintln!("frontend logging init failed: {err}");
    }

    // Finder / Dock 启动的 .app 没有终端 shell 环境；补载 ~/.astro/.env 与登录 shell 中的 API Key。
    infra::env_hydrate::hydrate_process_env();

    tauri::Builder::default()
        // 单实例须最先注册：二次启动聚焦已有窗口（Windows/Linux；macOS 另见 Reopen）。
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            ui::tray::show_main_window(app);
        }))
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_clipboard::init())
        .plugin(tauri_plugin_window_state::Builder::new().build())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .on_menu_event(|app, event| {
            if event.id() == MENU_PREFERENCES_ID {
                let _ = app.emit(EVENT_OPEN_PREFERENCES, ());
                ui::tray::show_main_window(app);
            } else if event.id() == MENU_ABOUT_ID {
                let _ = app.emit(EVENT_OPEN_ABOUT, ());
                ui::tray::show_main_window(app);
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
            // — chat —
            commands::chat::start_chat,
            commands::chat::chat_control,
            commands::chat::interrupt_resume,
            commands::chat::generate_image,
            commands::chat::query_memory,
            commands::chat::count_tokens,
            // — agent —
            commands::agent::prepare_multitask_worktree,
            commands::agent::cleanup_multitask_worktree,
            commands::agent::get_config,
            commands::agent::list_agents,
            commands::agent::create_agent,
            commands::agent::set_active_agent,
            commands::agent::set_pending_agent_icon,
            commands::agent::clear_pending_agent_icon,
            commands::agent::list_daily_memory,
            commands::agent::read_daily_memory,
            commands::agent::write_daily_memory,
            // — session —
            commands::session::get_chat_history,
            commands::session::fork_chat_session,
            commands::session::remove_chat_bubbles,
            commands::session::list_recent_sessions,
            commands::session::list_sessions,
            commands::session::rename_session,
            commands::session::regenerate_session_title,
            commands::session::archive_session,
            commands::session::unarchive_session,
            commands::session::pin_session,
            commands::session::unpin_session,
            commands::session::delete_session_permanently,
            // — files —
            commands::files::list_files,
            commands::files::read_file,
            commands::files::open_path_externally,
            commands::files::reveal_in_folder,
            commands::files::trash_paths,
            commands::files::read_file_base64,
            commands::files::read_user_file_base64,
            commands::files::download_file_to_downloads,
            commands::files::download_bytes_to_downloads,
            commands::files::copy_paths_to_clipboard,
            commands::files::list_clipboard_file_paths,
            commands::files::paste_paths_from_clipboard,
            commands::files::write_file,
            commands::files::create_file,
            commands::files::create_directory,
            commands::files::rename_path,
            commands::files::copy_paths,
            commands::files::move_paths,
            commands::files::delete_path,
            // — cron —
            commands::cron::list_cron_jobs,
            commands::cron::extract_cron_job,
            commands::cron::add_cron_job,
            commands::cron::update_cron_job,
            commands::cron::remove_cron_job,
            commands::cron::set_cron_job_enabled,
            commands::cron::run_cron_job_now,
            commands::cron::get_cron_run,
            commands::cron::delete_cron_run,
            commands::cron::list_cron_runs,
            commands::cron::list_cron_job_runs,
            // — batch & model catalog —
            commands::batch::create_batch,
            commands::batch::get_batch,
            commands::batch::list_batches,
            commands::batch::get_batch_results,
            commands::batch::list_model_catalog,
            // — memory —
            commands::memory::refresh_memory,
            commands::memory::list_pending_memory_writes,
            commands::memory::approve_pending_memory_write,
            commands::memory::reject_pending_memory_write,
            commands::memory::approve_all_pending_memory_writes,
            commands::memory::reject_all_pending_memory_writes,
            commands::memory::get_memory_settings,
            commands::memory::set_memory_write_approval,
            commands::memory::set_memory_auto_refresh,
            commands::memory::set_background_review_enabled,
            commands::memory::get_approval_settings,
            commands::memory::set_approval_mode,
            commands::memory::add_command_allowlist,
            commands::memory::remove_command_allowlist,
            // — loops —
            commands::loops::list_loops,
            commands::loops::get_loop,
            commands::loops::create_loop,
            commands::loops::save_loop,
            commands::loops::delete_loop,
            commands::loops::set_loop_enabled,
            commands::loops::set_loop_ai_callable,
            commands::loops::run_loop,
            commands::loops::list_loop_runs,
            commands::loops::get_loop_run,
            commands::loops::delete_loop_run,
            commands::loops::list_loop_step_logs,
            commands::loops::export_loop,
            commands::loops::export_loop_svg,
            commands::loops::import_loop,
            commands::loops::ai_generate_workflow,
            commands::loops::loop_ai_polish,
            // — media —
            commands::media::tts_synthesize,
            commands::media::speech_to_text,
            // — config —
            commands::config::get_tools_enabled,
            commands::config::set_tools_enabled,
            commands::config::get_tool_catalog,
            commands::config::get_mcp_servers,
            commands::config::set_mcp_servers,
            commands::config::refresh_mcp_tools,
            commands::config::get_agent_usage_stats,
            commands::config::get_usage_insights,
            commands::config::get_collaboration_insights,
            commands::config::get_trace_insights,
            commands::config::query_agent_logs,
            // — providers —
            commands::providers::get_providers_state,
            commands::providers::list_providers,
            commands::providers::add_provider,
            commands::providers::save_provider,
            commands::providers::reorder_providers,
            commands::providers::delete_provider,
            commands::providers::set_active_provider,
            commands::providers::set_provider_api_key,
            commands::providers::get_provider_api_key,
            commands::providers::clear_provider_api_key,
            commands::providers::list_provider_models,
            commands::providers::get_cached_provider_models,
            commands::providers::test_provider,
            commands::providers::test_provider_models,
            // — skills —
            commands::skills::list_installed_skills,
            commands::skills::search_store_skills,
            commands::skills::get_store_skill_detail,
            commands::skills::set_skill_enabled,
            commands::skills::link_machine_skill,
            commands::skills::install_store_skill,
            commands::skills::get_skill_content,
            commands::skills::list_skill_bundle,
            commands::skills::get_skill_file,
            commands::skills::open_skill_folder,
            commands::skills::reveal_skill_file,
            commands::skills::open_skill_file,
            commands::skills::list_skill_origins,
            commands::skills::preview_skill_update,
            commands::skills::update_installed_skill,
            commands::skills::check_skill_updates,
            commands::skills::update_all_skills,
            commands::skills::list_skill_backups,
            commands::skills::reveal_skill_backup,
            commands::skills::get_skill_cooldown_remaining,
            commands::skills::list_skill_snapshots,
            commands::skills::restore_skill_snapshot,
            commands::skills::get_skill_signal_summary,
            // — artifacts —
            commands::artifacts::list_artifacts,
            commands::artifacts::find_artifact_by_path,
            commands::artifacts::reconcile_artifacts,
            commands::artifacts::register_artifact,
            commands::artifacts::save_chat_upload,
            commands::artifacts::remove_artifacts_by_paths,
            // — dreaming —
            commands::dreaming::get_dreaming_status,
            commands::dreaming::set_dreaming_enabled_cmd,
            commands::dreaming::run_dreaming,
            // — compaction —
            commands::compaction::compact_chat_session,
            // — compression settings —
            commands::compression_settings::get_compression_settings,
            commands::compression_settings::set_compression_settings,
            commands::compression_settings::reset_compression_settings,
            // — auxiliary —
            commands::auxiliary::get_auxiliary_settings,
            commands::auxiliary::set_auxiliary_route,
            commands::auxiliary::reset_auxiliary_route,
            commands::auxiliary::reset_all_auxiliary_routes,
            // — evolution —
            commands::evolution::get_evolution_settings,
            commands::evolution::set_evolution_enabled,
            commands::evolution::set_evolution_route,
            commands::evolution::reset_evolution_route,
            commands::evolution::set_evolution_gates,
            commands::evolution::set_evolution_search,
            commands::evolution::set_evolution_auto,
            commands::evolution::set_evolution_curator,
            // — evolution_run —
            commands::evolution_run::run_evolution,
            commands::evolution_run::run_evolution_search,
            commands::evolution_run::cancel_evolution_search,
            commands::evolution_run::list_evolution_proposals,
            commands::evolution_run::approve_evolution_proposal,
            commands::evolution_run::approve_evolution_proposal_to_branch,
            commands::evolution_run::reject_evolution_proposal,
            commands::evolution_run::list_eval_examples,
            commands::evolution_run::list_eval_import_candidates,
            commands::evolution_run::import_eval_from_session,
            commands::evolution_run::add_eval_example,
            commands::evolution_run::remove_eval_example,
            commands::evolution_run::run_skill_curator,
            commands::evolution_run::enqueue_curator_proposals,
            commands::evolution_run::get_curator_last,
            commands::evolution_run::curator_status,
            commands::evolution_run::maybe_run_skill_curator,
            commands::evolution_run::evolution_dspy_status,
            commands::evolution_run::setup_evolution_dspy,
            commands::evolution_run::run_evolution_dspy,
            commands::evolution_run::evolution_history,
            commands::evolution_run::evolution_auto_status,
            commands::evolution_run::maybe_run_evolution_auto,
            // — icon —
            commands::icon::get_app_icon,
            commands::icon::set_app_icon,
            // — infra —
            infra::session_events::set_session_events_filter,
            infra::ip_location::infer_ip_location,
        ])
        .setup(|app| {
            if let Err(err) = memory::ensure_default_workspace() {
                tracing::warn!("workspace bootstrap failed: {err}");
            }

            // 默认同进程内嵌 gRPC；ASTRO_EMBED_BACKEND=0 时连外部 backend。
            // 未设 ASTRO_GRPC_ADDR 时 bind 127.0.0.1:0，实际端口经 oneshot + 进程内地址共享。
            if infra::grpc::embed_backend_enabled() {
                let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
                tauri::async_runtime::spawn(async move {
                    if let Err(err) = server::run_embedded(Some(ready_tx)).await {
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
                    infra::grpc::default_grpc_address()
                );
            }

            infra::session_events::start_bridge(app.handle());

            app.manage(Mutex::new(AppLocale::Zh));

            if let Err(err) = install_app_menu(app.handle(), AppLocale::Zh) {
                tracing::warn!("app menu install failed: {err}");
            }

            if let Err(err) = ui::tray::install_tray(app.handle(), AppLocale::Zh) {
                tracing::warn!("system tray install failed: {err}");
            }

            // 重放持久化的应用图标（托盘/程序坞/窗口）
            ui::app_icon::apply_app_icon(app.handle(), &ui::app_icon::load_variant());

            infra::notify::install(app.handle());

            meta::default_skills_seed::spawn_on_startup(app.handle());

            // 策展到期：仅刷新启发式报告，不入队、不调 LLM
            crate::commands::evolution_run::spawn_maybe_curator(app.handle().clone());

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
            // 进程真正退出前：清理仍在运行的后台任务（独立进程组，否则会变孤儿）。
            RunEvent::Exit => {
                let n = tools::shutdown_background_jobs();
                if n > 0 {
                    tracing::info!(killed = n, "terminated background jobs on exit");
                }
            }
            // macOS：点 Dock 图标时若窗口已关进托盘，重新显示。
            RunEvent::Reopen {
                has_visible_windows: false,
                ..
            } => {
                ui::tray::show_main_window(app);
            }
            _ => {}
        });
}
