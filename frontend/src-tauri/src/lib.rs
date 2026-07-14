//! Astro Agent Tauri 库：菜单、窗口与 `invoke` 命令注册入口。

#![allow(unexpected_cfgs)] // 旧版 objc 的 msg_send!/sel! 使用 cfg(feature = "cargo-clippy")

mod artifacts_commands;
mod clipboard_files;
mod commands;
mod config_commands;
mod dreaming_commands;
mod env_hydrate;
mod fs_ops;
mod grpc;
mod keystore;
mod litellm_meta;
mod model_meta;
mod providers_commands;
mod skills_commands;

use tauri::{
    AppHandle, Emitter, Manager, WebviewWindowBuilder,
    menu::{
        AboutMetadata, HELP_SUBMENU_ID, Menu, MenuItem, PredefinedMenuItem, Submenu,
        WINDOW_SUBMENU_ID,
    },
    window::Color,
};

/// 原生窗全透明；内容由 CSS 铺满。macOS 用系统装饰 + Overlay 标题栏（红绿灯）。
const BG: Color = Color(0x00, 0x00, 0x00, 0x00);

/// 与偏好设置「关于Astro」卡片一致的应用介绍（macOS 关于面板 credits）。
const ABOUT_CREDITS: &str = "Astro（阿童木）是本地 AI 桌面工作站，名字取自经典动漫《铁臂阿童木》——希望它像阿童木一样，成为你身边可靠、聪明、敢闯敢干的助手。支持智能对话、记忆召回、工作区与文件空间，可接入多家模型，并调用工具与 Skills 完成复杂任务。偏好设置保存在本机。";

/// 原生菜单「偏好设置」项 id。
const MENU_PREFERENCES_ID: &str = "preferences";
/// 前端监听的「打开偏好设置」事件名。
const EVENT_OPEN_PREFERENCES: &str = "open-preferences";

/// 安装应用菜单（关于、偏好设置、窗口与帮助）。
fn install_app_menu<R: tauri::Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let pkg = app.package_info();
    let about = AboutMetadata {
        name: Some("关于Astro".into()),
        version: Some(pkg.version.to_string()),
        credits: Some(ABOUT_CREDITS.into()),
        ..Default::default()
    };

    let preferences = MenuItem::with_id(
        app,
        MENU_PREFERENCES_ID,
        "偏好设置...",
        true,
        Some("CmdOrCtrl+,"),
    )?;

    let window_menu = Submenu::with_id_and_items(
        app,
        WINDOW_SUBMENU_ID,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::close_window(app, None)?,
        ],
    )?;

    let help_menu = Submenu::with_id_and_items(
        app,
        HELP_SUBMENU_ID,
        "Help",
        true,
        &[
            #[cfg(not(target_os = "macos"))]
            &PredefinedMenuItem::about(app, None, Some(about))?,
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
                    &PredefinedMenuItem::about(app, None, Some(about))?,
                    &PredefinedMenuItem::separator(app)?,
                    &preferences,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::services(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::hide(app, None)?,
                    &PredefinedMenuItem::hide_others(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::quit(app, None)?,
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
                "File",
                true,
                &[
                    &PredefinedMenuItem::close_window(app, None)?,
                    #[cfg(not(target_os = "macos"))]
                    &PredefinedMenuItem::quit(app, None)?,
                ],
            )?,
            &Submenu::with_items(
                app,
                "Edit",
                true,
                &[
                    &PredefinedMenuItem::undo(app, None)?,
                    &PredefinedMenuItem::redo(app, None)?,
                    &PredefinedMenuItem::separator(app)?,
                    &PredefinedMenuItem::cut(app, None)?,
                    &PredefinedMenuItem::copy(app, None)?,
                    &PredefinedMenuItem::paste(app, None)?,
                    &PredefinedMenuItem::select_all(app, None)?,
                ],
            )?,
            #[cfg(target_os = "macos")]
            &Submenu::with_items(
                app,
                "View",
                true,
                &[&PredefinedMenuItem::fullscreen(app, None)?],
            )?,
            &window_menu,
            &help_menu,
        ],
    )?;

    app.set_menu(menu)?;
    Ok(())
}

/// macOS：将窗口背景设为全透明，由 WebView CSS 负责视觉。
#[cfg(target_os = "macos")]
fn make_window_transparent(win: &tauri::WebviewWindow) {
    use objc::runtime::{Class, Object, BOOL, NO, YES};
    use objc::{msg_send, sel, sel_impl};

    if let Ok(ns_window) = win.ns_window() {
        unsafe {
            let ns_window = ns_window as *mut Object;
            let clear: *mut Object =
                msg_send![Class::get("NSColor").unwrap(), clearColor];
            let _: () = msg_send![ns_window, setOpaque: NO];
            let _: () = msg_send![ns_window, setBackgroundColor: clear];
            // decorations: true 时保留系统阴影

            let content_view: *mut Object = msg_send![ns_window, contentView];
            if content_view.is_null() {
                return;
            }

            let _: () = msg_send![content_view, setWantsLayer: YES];
            let layer: *mut Object = msg_send![content_view, layer];
            if !layer.is_null() {
                let cg: *mut Object = msg_send![clear, CGColor];
                let _: () = msg_send![layer, setBackgroundColor: cg];
                let _: () = msg_send![layer, setOpaque: NO];
            }

            // 只处理直接子视图（WKWebView），避免闪烁
            let subviews: *mut Object = msg_send![content_view, subviews];
            if subviews.is_null() {
                return;
            }
            let count: usize = msg_send![subviews, count];
            for i in 0..count {
                let child: *mut Object = msg_send![subviews, objectAtIndex: i];
                if child.is_null() {
                    continue;
                }
                let responds: BOOL =
                    msg_send![child, respondsToSelector: sel!(setDrawsBackground:)];
                if responds == YES {
                    let _: () = msg_send![child, setDrawsBackground: NO];
                }
                let responds: BOOL = msg_send![child, respondsToSelector: sel!(setOpaque:)];
                if responds == YES {
                    let _: () = msg_send![child, setOpaque: NO];
                }
                let _: () = msg_send![child, setWantsLayer: YES];
                let child_layer: *mut Object = msg_send![child, layer];
                if !child_layer.is_null() {
                    let cg: *mut Object = msg_send![clear, CGColor];
                    let _: () = msg_send![child_layer, setBackgroundColor: cg];
                    let _: () = msg_send![child_layer, setOpaque: NO];
                }
            }
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
/// 启动 Tauri 应用（菜单、窗口与插件）。
pub fn run() {
    if let Err(err) = memory::init_logging("agent") {
        eprintln!("frontend logging init failed: {err}");
    }

    // Finder / Dock 启动的 .app 没有终端 shell 环境；补载 ~/.astro/.env 与登录 shell 中的 API Key。
    env_hydrate::hydrate_process_env();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .on_menu_event(|app, event| {
            if event.id() == MENU_PREFERENCES_ID {
                let _ = app.emit(EVENT_OPEN_PREFERENCES, ());
                if let Some(win) = app.get_webview_window("main") {
                    let _ = win.unminimize();
                    let _ = win.set_focus();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::start_chat,
            commands::chat_control,
            commands::interrupt_resume,
            commands::generate_image,
            commands::query_memory,
            commands::get_chat_history,
            commands::fork_chat_session,
            commands::list_recent_sessions,
            commands::list_files,
            commands::read_file,
            commands::open_path_externally,
            commands::reveal_in_folder,
            commands::trash_paths,
            commands::read_file_base64,
            commands::copy_paths_to_clipboard,
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
            artifacts_commands::list_artifacts,
            artifacts_commands::reconcile_artifacts,
            artifacts_commands::register_artifact,
            artifacts_commands::save_chat_upload,
            artifacts_commands::remove_artifacts_by_paths,
            dreaming_commands::get_dreaming_status,
            dreaming_commands::set_dreaming_enabled_cmd,
            dreaming_commands::run_dreaming,
        ])
        .setup(|app| {
            if let Err(err) = memory::ensure_default_workspace() {
                tracing::warn!("workspace bootstrap failed: {err}");
            }

            if let Err(err) = install_app_menu(app.handle()) {
                tracing::warn!("app menu install failed: {err}");
            }

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
            make_window_transparent(&window);

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
