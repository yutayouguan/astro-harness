//! 桌面端浏览器侧栏与 Agent `browser_*` 工具共享浏览器会话。

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};
use tauri::{
    plugin::{Builder as PluginBuilder, TauriPlugin},
    webview::{NewWindowResponse, PageLoadEvent, WebviewBuilder},
    Emitter, LogicalPosition, LogicalSize, Manager, Runtime, WebviewUrl, Window,
};

pub const LIVE_BROWSER_WEBVIEW_PREFIX: &str = "astro-browser-live-";
pub const LIVE_BROWSER_PAGE_EVENT: &str = "browser-live-page-load";

const LIVE_BROWSER_INTERACTION_SCRIPT: &str = r#"
(() => {
document.addEventListener("click", (event) => {
  if (event.button !== 0) return;
  const anchor = event.composedPath().find((node) => node instanceof HTMLAnchorElement);
  if (
    !anchor ||
    anchor.target.trim().toLowerCase() !== "_blank" ||
    anchor.hasAttribute("download")
  ) return;
  try {
    const next = new URL(anchor.href, window.location.href);
    if (next.protocol !== "http:" && next.protocol !== "https:") return;
    event.preventDefault();
    event.stopImmediatePropagation();
    window.location.assign(next.href);
  } catch {}
}, true);

const originalOpen = window.open.bind(window);
window.open = (url, target, features) => {
  const normalizedTarget = typeof target === "string" ? target.trim().toLowerCase() : "";
  if (!normalizedTarget || normalizedTarget === "_blank") {
    try {
      const next = new URL(String(url || ""), window.location.href);
      if (next.protocol === "http:" || next.protocol === "https:") {
        window.location.assign(next.href);
        return window;
      }
    } catch {}
  }
  return originalOpen(url, target, features);
};
})();
"#;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPanelRequest {
    pub session_id: String,
    pub action: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserPreviewFileRequest {
    pub session_id: String,
    pub project_id: String,
    pub path: String,
    #[serde(default)]
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserLiveWebviewRequest {
    pub label: String,
    pub action: String,
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub width: Option<f64>,
    #[serde(default)]
    pub height: Option<f64>,
    #[serde(default)]
    pub user_agent: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BrowserLivePageEvent {
    label: String,
    url: String,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSettingsState {
    #[serde(flatten)]
    pub settings: tools::builtin::shell::browser::BrowserSettings,
    pub browser_available: bool,
    pub data_directory: String,
    pub approval_rules: Vec<BrowserApprovalRuleDto>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserApprovalRuleDto {
    pub origin: String,
    pub action_class: String,
}

fn is_live_browser_webview(label: &str) -> bool {
    label
        .strip_prefix(LIVE_BROWSER_WEBVIEW_PREFIX)
        .is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'/' | b':')
                })
        })
}

fn emit_live_page_event<R: Runtime>(
    webview: &tauri::Webview<R>,
    url: &str,
    status: &'static str,
    error: Option<String>,
) {
    let _ = webview.emit(
        LIVE_BROWSER_PAGE_EVENT,
        BrowserLivePageEvent {
            label: webview.label().to_string(),
            url: url.to_string(),
            status,
            error,
        },
    );
}

async fn create_live_browser_webview<R: Runtime>(
    window: Window<R>,
    request: &BrowserLiveWebviewRequest,
) -> Result<(), String> {
    let label = request.label.trim().to_string();
    let x = request.x.ok_or("缺少浏览器横坐标")?;
    let y = request.y.ok_or("缺少浏览器纵坐标")?;
    let width = request.width.ok_or("缺少浏览器宽度")?;
    let height = request.height.ok_or("缺少浏览器高度")?;
    if width <= 0.0
        || height <= 0.0
        || !x.is_finite()
        || !y.is_finite()
        || !width.is_finite()
        || !height.is_finite()
    {
        return Err("浏览器显示区尺寸无效".into());
    }
    if window.app_handle().get_webview(&label).is_some() {
        return Ok(());
    }

    let validated_url = tools::builtin::shell::browser::validate_live_webview_url(
        &home::default_memory_dir(),
        request.url.as_deref().unwrap_or_default().trim(),
    )
    .map_err(|error| error.to_string())?;
    let initial_url = url::Url::parse(&validated_url).map_err(|error| error.to_string())?;

    let app = window.app_handle().clone();
    let target_label = label.clone();
    let popup_generation = Arc::new(AtomicU64::new(0));
    let mut builder = WebviewBuilder::new(label, WebviewUrl::External(initial_url))
        .focused(false)
        .accept_first_mouse(true)
        .zoom_hotkeys_enabled(true)
        .initialization_script(LIVE_BROWSER_INTERACTION_SCRIPT)
        .on_new_window(move |url, _features| {
            let generation = popup_generation.fetch_add(1, Ordering::Relaxed) + 1;
            let latest_generation = popup_generation.clone();
            let navigation_app = app.clone();
            let navigation_label = target_label.clone();
            let requested_url = url.to_string();
            // Fire-and-forget: the JoinHandle is dropped, the blocking task keeps
            // validating the popup URL and navigating.
            tauri::async_runtime::spawn_blocking(move || {
                let Ok(validated_url) = tools::builtin::shell::browser::validate_live_webview_url(
                    &home::default_memory_dir(),
                    &requested_url,
                ) else {
                    return;
                };
                if latest_generation.load(Ordering::Relaxed) != generation {
                    return;
                }
                if let Ok(destination) = url::Url::parse(&validated_url) {
                    let app_for_main = navigation_app.clone();
                    let latest_for_main = latest_generation.clone();
                    let destination_text = destination.to_string();
                    let _ = navigation_app.run_on_main_thread(move || {
                        if latest_for_main.load(Ordering::Relaxed) != generation {
                            return;
                        }
                        if let Some(webview) = app_for_main.get_webview(&navigation_label) {
                            if let Err(error) = webview.navigate(destination) {
                                tracing::warn!(
                                    label = webview.label(),
                                    url = %destination_text,
                                    error = %error,
                                    "navigate popup target in live browser failed"
                                );
                            }
                        }
                    });
                }
            });
            NewWindowResponse::Deny
        });
    if let Some(user_agent) = request
        .user_agent
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        builder = builder.user_agent(user_agent);
    }

    window
        .add_child(
            builder,
            LogicalPosition::new(x, y),
            LogicalSize::new(width, height),
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
}

/// Observe only Astro's remote browser surfaces. Other application WebViews
/// keep their existing navigation behavior.
pub fn live_browser_plugin<R: Runtime>() -> TauriPlugin<R> {
    PluginBuilder::new("browser-live")
        .on_navigation(|webview, url| {
            if !is_live_browser_webview(webview.label()) {
                return true;
            }
            match tools::builtin::shell::browser::validate_live_webview_url(
                &home::default_memory_dir(),
                url.as_str(),
            ) {
                Ok(_) => true,
                Err(error) => {
                    emit_live_page_event(webview, url.as_str(), "blocked", Some(error.to_string()));
                    false
                }
            }
        })
        .on_page_load(|webview, payload| {
            if !is_live_browser_webview(webview.label()) {
                return;
            }
            let status = match payload.event() {
                PageLoadEvent::Started => "started",
                PageLoadEvent::Finished => "finished",
            };
            emit_live_page_event(webview, payload.url().as_str(), status, None);
        })
        .build()
}

fn browser_settings_state(
    settings: tools::builtin::shell::browser::BrowserSettings,
) -> BrowserSettingsState {
    let memory_dir = home::default_memory_dir();
    BrowserSettingsState {
        settings,
        browser_available: tools::builtin::shell::browser::browser_available(),
        data_directory: tools::builtin::shell::browser::browser_data_dir(&memory_dir)
            .to_string_lossy()
            .into_owned(),
        approval_rules: tools::builtin::shell::browser::load_approval_rules(&memory_dir)
            .into_iter()
            .map(|rule| BrowserApprovalRuleDto {
                origin: rule.origin,
                action_class: rule.action_class,
            })
            .collect(),
    }
}

#[tauri::command]
pub async fn browser_get_settings() -> Result<BrowserSettingsState, String> {
    let memory_dir = home::default_memory_dir();
    Ok(browser_settings_state(
        tools::builtin::shell::browser::load_browser_settings(&memory_dir),
    ))
}

#[tauri::command]
pub async fn browser_set_settings(
    settings: tools::builtin::shell::browser::BrowserSettings,
) -> Result<BrowserSettingsState, String> {
    let memory_dir = home::default_memory_dir();
    let settings = tools::builtin::shell::browser::update_browser_settings(&memory_dir, settings)
        .await
        .map_err(|error| error.to_string())?;
    Ok(browser_settings_state(settings))
}

#[tauri::command]
pub async fn browser_revoke_approval(
    origin: String,
    action_class: String,
) -> Result<BrowserSettingsState, String> {
    let memory_dir = home::default_memory_dir();
    tools::builtin::shell::browser::remove_approval_rule(
        &memory_dir,
        origin.trim(),
        action_class.trim(),
    )
    .map_err(|error| error.to_string())?;
    Ok(browser_settings_state(
        tools::builtin::shell::browser::load_browser_settings(&memory_dir),
    ))
}

#[tauri::command]
pub async fn browser_panel_control(request: BrowserPanelRequest) -> Result<Value, String> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return Err("浏览器会话 ID 不能为空".into());
    }
    let raw = tools::builtin::shell::browser::desktop_control(
        session_id,
        &home::default_memory_dir(),
        request.action.trim(),
        request.args,
    )
    .await
    .map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn browser_live_webview_control<R: Runtime>(
    window: Window<R>,
    request: BrowserLiveWebviewRequest,
) -> Result<(), String> {
    let label = request.label.trim();
    if !is_live_browser_webview(label) {
        return Err("非法的实时浏览器 WebView 标识".into());
    }
    if request.action.trim() == "create" {
        return create_live_browser_webview(window, &request).await;
    }
    let webview = window
        .app_handle()
        .get_webview(label)
        .ok_or_else(|| "实时浏览器 WebView 不存在".to_string())?;
    match request.action.trim() {
        "navigate" => {
            let raw = request.url.as_deref().unwrap_or_default();
            let url = tools::builtin::shell::browser::validate_live_webview_url(
                &home::default_memory_dir(),
                raw,
            )
            .map_err(|error| error.to_string())?;
            webview
                .navigate(url::Url::parse(&url).map_err(|error| error.to_string())?)
                .map_err(|error| error.to_string())
        }
        "reload" => webview.reload().map_err(|error| error.to_string()),
        _ => Err("不支持的实时浏览器操作".into()),
    }
}

#[tauri::command]
pub async fn browser_preview_project_file(
    request: BrowserPreviewFileRequest,
) -> Result<Value, String> {
    let session_id = request.session_id.trim();
    if session_id.is_empty() {
        return Err("需要先开始一个对话，才能预览网页".into());
    }
    let roots = crate::commands::files::project_roots(&request.project_id).await?;
    let path = crate::commands::files::resolve_project_path(&roots, &request.path)?;
    if request
        .content
        .as_ref()
        .is_some_and(|content| content.len() > 16 * 1024 * 1024)
    {
        return Err("HTML 预览内容不能超过 16 MB".into());
    }
    let raw = tools::builtin::shell::browser::desktop_preview_file(
        session_id,
        &home::default_memory_dir(),
        &path,
        request.content,
    )
    .await
    .map_err(|error| error.to_string())?;
    serde_json::from_str(&raw).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{
        is_live_browser_webview, LIVE_BROWSER_INTERACTION_SCRIPT, LIVE_BROWSER_WEBVIEW_PREFIX,
    };

    #[test]
    fn live_webview_labels_are_scoped_and_safe() {
        assert!(is_live_browser_webview(&format!(
            "{LIVE_BROWSER_WEBVIEW_PREFIX}session_tab-1"
        )));
        assert!(!is_live_browser_webview("main"));
        assert!(!is_live_browser_webview(LIVE_BROWSER_WEBVIEW_PREFIX));
        assert!(!is_live_browser_webview(&format!(
            "{LIVE_BROWSER_WEBVIEW_PREFIX}bad label"
        )));
    }

    #[test]
    fn live_webview_intercepts_blank_anchors_at_document_start() {
        assert!(LIVE_BROWSER_INTERACTION_SCRIPT.contains("target.trim().toLowerCase()"));
        assert!(LIVE_BROWSER_INTERACTION_SCRIPT.contains("window.open ="));
        assert!(LIVE_BROWSER_INTERACTION_SCRIPT.contains("window.location.assign"));
        assert!(LIVE_BROWSER_INTERACTION_SCRIPT.contains("event.stopImmediatePropagation()"));
    }
}
