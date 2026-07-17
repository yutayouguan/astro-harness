//! 系统托盘：关窗进托盘常驻，点击恢复，菜单「退出」才真正结束进程。

use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

use crate::menu_locale::AppLocale;

const TRAY_ID: &str = "main-tray";
const TRAY_SHOW_ID: &str = "tray-show";
const TRAY_QUIT_ID: &str = "tray-quit";

/// 显示并聚焦主窗口。
pub fn show_main_window<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window("main") {
        let _ = win.unminimize();
        let _ = win.show();
        let _ = win.set_focus();
    }
}

fn build_tray_menu<R: Runtime>(app: &AppHandle<R>, locale: AppLocale) -> tauri::Result<Menu<R>> {
    let s = locale.strings();
    let show = MenuItem::with_id(app, TRAY_SHOW_ID, s.tray_show, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, TRAY_QUIT_ID, s.tray_quit, true, None::<&str>)?;
    Menu::with_items(app, &[&show, &PredefinedMenuItem::separator(app)?, &quit])
}

/// 安装托盘图标与菜单（按语言）。
pub fn install_tray<R: Runtime>(app: &AppHandle<R>, locale: AppLocale) -> tauri::Result<()> {
    let s = locale.strings();
    let menu = build_tray_menu(app, locale)?;

    // 托盘图标按持久化的应用图标变体加载；失败回退到内置窗口图标。
    let icon = match crate::app_icon::icon_image(&crate::app_icon::load_variant()) {
        Ok(img) => img,
        Err(_) => app
            .default_window_icon()
            .cloned()
            .ok_or_else(|| tauri::Error::AssetNotFound("default window icon".into()))?,
    };

    let _tray = TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon)
        .tooltip(s.tray_tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            TRAY_SHOW_ID => show_main_window(app),
            TRAY_QUIT_ID => crate::request_app_exit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// 语言切换时更新托盘菜单与 tooltip（保留同一 tray id）。
pub fn apply_tray_locale<R: Runtime>(app: &AppHandle<R>, locale: AppLocale) -> tauri::Result<()> {
    let s = locale.strings();
    let menu = build_tray_menu(app, locale)?;
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        tray.set_menu(Some(menu))?;
        tray.set_tooltip(Some(s.tray_tooltip))?;
        Ok(())
    } else {
        install_tray(app, locale)
    }
}
