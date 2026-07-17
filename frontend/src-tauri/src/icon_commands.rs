//! 应用图标切换 Tauri 命令：读取/设置变体并实时应用到托盘/程序坞/窗口。

use base64::Engine;
use serde::Serialize;
use tauri::{AppHandle, Runtime};

use crate::app_icon;

/// 单个图标变体的展示态（含 base64 缩略图供选择器预览）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIconOptionDto {
    pub id: String,
    pub data_url: String,
}

/// 图标设置全量。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppIconSettingsDto {
    pub current: String,
    pub options: Vec<AppIconOptionDto>,
}

fn build_options() -> Vec<AppIconOptionDto> {
    app_icon::variant_ids()
        .into_iter()
        .map(|id| {
            let b64 = base64::engine::general_purpose::STANDARD.encode(app_icon::variant_png(id));
            AppIconOptionDto {
                id: id.to_string(),
                data_url: format!("data:image/png;base64,{b64}"),
            }
        })
        .collect()
}

fn build_settings() -> AppIconSettingsDto {
    AppIconSettingsDto {
        current: app_icon::load_variant(),
        options: build_options(),
    }
}

/// 读取当前图标变体与全部可选项。
#[tauri::command]
pub async fn get_app_icon() -> Result<AppIconSettingsDto, String> {
    Ok(build_settings())
}

/// 设置图标变体：持久化并立即应用到托盘/程序坞/窗口。
#[tauri::command]
pub async fn set_app_icon<R: Runtime>(
    app: AppHandle<R>,
    variant: String,
) -> Result<AppIconSettingsDto, String> {
    let normalized = app_icon::normalize_variant(&variant).to_string();
    app_icon::save_variant(&normalized).map_err(|e| e.to_string())?;
    app_icon::apply_app_icon(&app, &normalized);
    Ok(build_settings())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_cover_four_variants_with_data_urls() {
        let opts = build_options();
        assert_eq!(opts.len(), 4);
        assert_eq!(opts[0].id, "blue");
        for o in &opts {
            assert!(o.data_url.starts_with("data:image/png;base64,"));
            assert!(o.data_url.len() > "data:image/png;base64,".len());
        }
    }
}
