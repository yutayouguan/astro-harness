//! 应用自更新：从构建时注入的公开 GitHub Releases 清单检查并安装签名更新。

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Update, UpdaterExt};
use url::Url;

const UPDATE_ENDPOINT_ENV: &str = "ASTRO_UPDATE_ENDPOINT";
const DEFAULT_UPDATE_ENDPOINT: &str =
    "https://github.com/yutayouguan/astro-agent-releases/releases/latest/download/latest.json";

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AppUpdateInfo {
    configured: bool,
    available: bool,
    current_version: String,
    version: Option<String>,
    date: Option<String>,
    notes: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUpdateProgress {
    phase: &'static str,
    downloaded: u64,
    total: Option<u64>,
}

fn parse_update_endpoint(raw: Option<String>) -> Result<Option<Url>, String> {
    let Some(raw) = raw
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    let endpoint = Url::parse(&raw).map_err(|error| format!("invalid update endpoint: {error}"))?;
    if endpoint.scheme() != "https" {
        return Err("update endpoint must use HTTPS".to_owned());
    }
    Ok(Some(endpoint))
}

fn configured_update_endpoint() -> Result<Option<Url>, String> {
    let runtime = std::env::var(UPDATE_ENDPOINT_ENV).ok();
    let compiled = option_env!("ASTRO_UPDATE_ENDPOINT").map(str::to_owned);
    parse_update_endpoint(
        runtime
            .or(compiled)
            .or_else(|| Some(DEFAULT_UPDATE_ENDPOINT.to_owned())),
    )
}

async fn find_update(app: &AppHandle) -> Result<Option<Update>, String> {
    let endpoint = configured_update_endpoint()?
        .ok_or_else(|| "application updater is not configured".to_owned())?;
    app.updater_builder()
        .endpoints(vec![endpoint])
        .map_err(|error| error.to_string())?
        .build()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn check_app_update(app: AppHandle) -> Result<AppUpdateInfo, String> {
    let current_version = app.package_info().version.to_string();
    if configured_update_endpoint()?.is_none() {
        return Ok(AppUpdateInfo {
            configured: false,
            available: false,
            current_version,
            version: None,
            date: None,
            notes: None,
        });
    }

    let update = find_update(&app).await?;
    Ok(match update {
        Some(update) => AppUpdateInfo {
            configured: true,
            available: true,
            current_version,
            version: Some(update.version),
            date: update.date.map(|date| date.to_string()),
            notes: update.body,
        },
        None => AppUpdateInfo {
            configured: true,
            available: false,
            current_version,
            version: None,
            date: None,
            notes: None,
        },
    })
}

#[tauri::command]
pub(crate) async fn install_app_update(app: AppHandle) -> Result<(), String> {
    let update = find_update(&app)
        .await?
        .ok_or_else(|| "no application update is available".to_owned())?;
    let progress_app = app.clone();
    let finished_app = app.clone();
    let mut downloaded = 0_u64;
    update
        .download_and_install(
            move |chunk_size, total| {
                downloaded = downloaded.saturating_add(chunk_size as u64);
                let _ = progress_app.emit(
                    "app-update-progress",
                    AppUpdateProgress {
                        phase: "downloading",
                        downloaded,
                        total,
                    },
                );
            },
            move || {
                let _ = finished_app.emit(
                    "app-update-progress",
                    AppUpdateProgress {
                        phase: "installing",
                        downloaded: 0,
                        total: None,
                    },
                );
            },
        )
        .await
        .map_err(|error| error.to_string())?;

    #[cfg(not(target_os = "windows"))]
    app.restart();

    #[cfg(target_os = "windows")]
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{configured_update_endpoint, parse_update_endpoint, DEFAULT_UPDATE_ENDPOINT};

    #[test]
    fn updater_endpoint_must_be_https() {
        assert!(parse_update_endpoint(None).unwrap().is_none());
        assert!(parse_update_endpoint(Some(" ".to_owned()))
            .unwrap()
            .is_none());
        assert!(
            parse_update_endpoint(Some("http://updates.example/latest.json".to_owned())).is_err()
        );
        assert!(
            parse_update_endpoint(Some("https://updates.example/latest.json".to_owned()))
                .unwrap()
                .is_some()
        );
        assert_eq!(
            configured_update_endpoint().unwrap().unwrap().as_str(),
            DEFAULT_UPDATE_ENDPOINT
        );
    }
}
