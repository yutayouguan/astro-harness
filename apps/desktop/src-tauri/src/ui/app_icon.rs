//! 运行时应用图标切换：托盘 + macOS 程序坞 + Win/Linux 窗口图标。
//!
//! 4 个变体在编译期 `include_bytes!` 嵌入二进制，运行时无磁盘依赖；
//! 选择持久化到 `{memory_dir}/app-icon.json`，启动时重放。
//! 注意：已安装 `.app` 在访达的图标为构建期烘焙，运行时无法更改。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::image::Image;
use tauri::{AppHandle, Runtime};

/// 与 [`tray::install_tray`](crate::ui::tray) 使用的 tray id 保持一致。
const TRAY_ID: &str = "main-tray";

/// 默认变体（与构建期烘焙的 `icon.png` 来源一致）。
pub const DEFAULT_VARIANT: &str = "blue";

/// 变体 id 与其嵌入 PNG 字节。
const VARIANTS: &[(&str, &[u8])] = &[
    ("blue", include_bytes!("../../icons/blue.png")),
    ("deep_blue", include_bytes!("../../icons/deep_blue.png")),
    ("black", include_bytes!("../../icons/black.png")),
    ("white", include_bytes!("../../icons/white.png")),
    ("white_logo", include_bytes!("../../icons/white_logo.png")),
];

/// 全部可选变体 id（保持声明顺序）。
pub fn variant_ids() -> Vec<&'static str> {
    VARIANTS.iter().map(|(id, _)| *id).collect()
}

/// 校验变体：未知回退 [`DEFAULT_VARIANT`]。
pub fn normalize_variant(variant: &str) -> &'static str {
    VARIANTS
        .iter()
        .find(|(id, _)| *id == variant)
        .map(|(id, _)| *id)
        .unwrap_or(DEFAULT_VARIANT)
}

/// 取某变体的原始 PNG 字节。
pub fn variant_png(variant: &str) -> &'static [u8] {
    let v = normalize_variant(variant);
    VARIANTS
        .iter()
        .find(|(id, _)| *id == v)
        .map(|(_, bytes)| *bytes)
        .unwrap_or(VARIANTS[0].1)
}

/// 解码变体 PNG 为 Tauri [`Image`]（需 tauri `image-png` feature）。
pub fn icon_image(variant: &str) -> tauri::Result<Image<'static>> {
    Image::from_bytes(variant_png(variant))
}

/// 持久化文件路径：`{memory_dir}/app-icon.json`。
fn app_icon_path() -> PathBuf {
    home::default_memory_dir().join("app-icon.json")
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct AppIconState {
    variant: String,
}

/// 读取持久化变体（缺省 / 非法回退 [`DEFAULT_VARIANT`]）。
pub fn load_variant() -> String {
    let path = app_icon_path();
    let raw = match std::fs::read_to_string(&path) {
        Ok(r) => r,
        Err(_) => return DEFAULT_VARIANT.to_string(),
    };
    let state: AppIconState = serde_json::from_str(&raw).unwrap_or_default();
    normalize_variant(&state.variant).to_string()
}

/// 写入持久化变体（原子替换）。
pub fn save_variant(variant: &str) -> anyhow::Result<()> {
    let v = normalize_variant(variant);
    let path = app_icon_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let state = AppIconState {
        variant: v.to_string(),
    };
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(&state)?)?;
    std::fs::rename(&tmp, &path)?;
    Ok(())
}

/// 将选定变体应用到托盘 / 程序坞 / 窗口（best-effort，各表面独立容错）。
pub fn apply_app_icon<R: Runtime>(app: &AppHandle<R>, variant: &str) {
    let v = normalize_variant(variant);

    // 托盘（全平台）
    match icon_image(v) {
        Ok(img) => {
            if let Some(tray) = app.tray_by_id(TRAY_ID) {
                if let Err(e) = tray.set_icon(Some(img)) {
                    tracing::warn!(error = %e, "tray set_icon failed");
                }
            }
        }
        Err(e) => tracing::warn!(error = %e, "decode tray icon failed"),
    }

    // 窗口图标：Win/Linux 生效；macOS 无标题栏图标，跳过
    #[cfg(not(target_os = "macos"))]
    {
        if let (Some(win), Ok(img)) = (app.get_webview_window("main"), icon_image(v)) {
            if let Err(e) = win.set_icon(img) {
                tracing::warn!(error = %e, "window set_icon failed");
            }
        }
    }

    // macOS 程序坞图标
    #[cfg(target_os = "macos")]
    set_macos_dock_icon(variant_png(v));
}

/// 通过 AppKit 设置程序坞图标：`NSApplication.setApplicationIconImage:`。
#[cfg(target_os = "macos")]
fn set_macos_dock_icon(png: &[u8]) {
    use objc::runtime::{Class, Object};
    use objc::{msg_send, sel, sel_impl};

    unsafe {
        let Some(data_cls) = Class::get("NSData") else {
            return;
        };
        let Some(image_cls) = Class::get("NSImage") else {
            return;
        };
        let Some(app_cls) = Class::get("NSApplication") else {
            return;
        };

        // NSData dataWithBytes:length:
        let data: *mut Object = msg_send![
            data_cls,
            dataWithBytes: png.as_ptr() as *const std::os::raw::c_void
            length: png.len()
        ];
        if data.is_null() {
            return;
        }

        // [[NSImage alloc] initWithData:data]
        let image: *mut Object = msg_send![image_cls, alloc];
        let image: *mut Object = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }

        let ns_app: *mut Object = msg_send![app_cls, sharedApplication];
        if ns_app.is_null() {
            return;
        }
        let _: () = msg_send![ns_app, setApplicationIconImage: image];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn variants_cover_all_and_decode() {
        let ids = variant_ids();
        assert_eq!(ids, ["blue", "deep_blue", "black", "white", "white_logo"]);
        for id in ids {
            assert!(!variant_png(id).is_empty(), "{id} png empty");
            assert!(icon_image(id).is_ok(), "{id} decode failed");
        }
    }

    #[test]
    fn unknown_variant_falls_back_to_default() {
        assert_eq!(normalize_variant("nope"), DEFAULT_VARIANT);
        assert_eq!(normalize_variant("blue"), "blue");
        assert_eq!(normalize_variant("white_logo"), "white_logo");
    }
}
