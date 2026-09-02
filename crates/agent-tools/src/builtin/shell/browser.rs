//! 任务绑定的 Chromium 浏览器自动化。
//!
//! 每个聊天会话拥有一个隔离的浏览器工作区，可包含多个标签页。工具返回结构化 JSON，
//! 桌面端渲染可交互预览，同时模型接收同一活动标签页的 DOM 快照和操作结果。

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use base64::Engine;
use futures::{SinkExt, StreamExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

type DevtoolsSocket = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

const DEFAULT_WAIT_MS: u64 = 8_000;
const MAX_WAIT_MS: u64 = 30_000;
const MAX_SNAPSHOT_CHARS: usize = 20_000;
const DEFAULT_BROWSER_HOME_PAGE: &str = "https://example.com/";
const DEFAULT_VIEWPORT_WIDTH: u32 = 1280;
const DEFAULT_VIEWPORT_HEIGHT: u32 = 800;

/// Shared browser runtime preferences. These settings are global to the local
/// Astro installation while browser tabs and profiles remain task-isolated.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, rename_all = "camelCase")]
pub struct BrowserSettings {
    pub home_page: String,
    pub viewport_width: u32,
    pub viewport_height: u32,
    pub allow_loopback: bool,
    pub downloads_enabled: bool,
}

impl Default for BrowserSettings {
    fn default() -> Self {
        Self {
            home_page: DEFAULT_BROWSER_HOME_PAGE.to_string(),
            viewport_width: DEFAULT_VIEWPORT_WIDTH,
            viewport_height: DEFAULT_VIEWPORT_HEIGHT,
            allow_loopback: true,
            downloads_enabled: true,
        }
    }
}

impl BrowserSettings {
    fn normalized(mut self) -> anyhow::Result<Self> {
        self.home_page = normalize_http_url(&self.home_page)?;
        if is_loopback_url(&self.home_page) && !self.allow_loopback {
            anyhow::bail!("the configured home page requires local development access");
        }
        self.viewport_width = self.viewport_width.clamp(320, 1_600);
        self.viewport_height = self.viewport_height.clamp(240, 1_400);
        Ok(self)
    }
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserOpenArgs {
    /// 公开的 http(s) URL，或本地开发服务器的回环地址 URL。
    pub url: String,
    /// 等待 DOM 就绪的最大时间。
    #[serde(default)]
    pub wait_ms: Option<u64>,
    /// 在新标签页打开；默认在当前标签页导航。
    #[serde(default)]
    pub new_tab: bool,
}

#[derive(Debug, Clone, Deserialize, Default)]
struct BrowserDesktopNewTabArgs {
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserTabArgs {
    /// 标签页 id；由 browser_tabs 或任一浏览器结果返回。
    pub tab_id: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct BrowserHistoryArgs {
    /// 导航后等待 DOM 就绪的最大时间。
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserPointArgs {
    /// 截图坐标系中的横坐标。
    pub x: f64,
    /// 截图坐标系中的纵坐标。
    pub y: f64,
    /// 点击后等待多久再获取下一次快照。
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserKeyArgs {
    /// 键值，例如 Enter、Backspace、ArrowLeft，或单个可打印字符。
    pub key: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserViewportArgs {
    /// Browser viewport width in CSS pixels.
    pub width: u32,
    /// Browser viewport height in CSS pixels.
    pub height: u32,
    /// Capture a screenshot after resizing. Desktop live WebViews disable this
    /// because the native surface is already visible.
    #[serde(default)]
    pub screenshot: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct BrowserSnapshotArgs {
    /// 捕获并刷新预览截图（默认 true）。
    #[serde(default)]
    pub screenshot: Option<bool>,
    /// 等待 DOM 就绪的最大时间。
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserClickArgs {
    /// CSS 选择器。优先使用 browser_snapshot 返回的选择器。
    #[serde(default)]
    pub selector: Option<String>,
    /// 当没有稳定选择器时，使用可见标签文本作为回退。
    #[serde(default)]
    pub text: Option<String>,
    /// 声明的操作意图，审批层据此判断是否为状态变更操作。
    #[serde(default)]
    pub intent: BrowserActionIntent,
    /// 点击后等待多久再获取下一次快照。
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserTypeArgs {
    /// 目标 input、textarea、select 或可编辑元素的 CSS 选择器。
    pub selector: String,
    /// 要输入的文本。禁止输入密码、OTP、token 和支付数据。
    pub text: String,
    /// 声明的操作意图，审批层据此判断是否为敏感提交。
    #[serde(default)]
    pub intent: BrowserActionIntent,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct BrowserScrollArgs {
    /// 水平滚动增量，单位为 CSS 像素。
    #[serde(default)]
    pub x: Option<i64>,
    /// 垂直滚动增量，单位为 CSS 像素（默认 600）。
    #[serde(default)]
    pub y: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct BrowserWaitArgs {
    /// 要等待的 CSS 选择器。
    #[serde(default)]
    pub selector: Option<String>,
    /// 未提供选择器时，等待出现的可见文本。
    #[serde(default)]
    pub text: Option<String>,
    /// 最大等待时间（毫秒），默认 8000，上限 30000。
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserActionIntent {
    /// 检查、导航、测试控件等可逆操作。
    #[default]
    ReadOnly,
    /// 表单提交或会改变远程状态的操作。
    StateChanging,
    /// 登录、授权、发布、删除、购买等敏感操作。
    Sensitive,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserApprovalClass {
    StateChanging,
    Sensitive,
}

impl BrowserApprovalClass {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StateChanging => "state_changing",
            Self::Sensitive => "sensitive",
        }
    }
}

impl BrowserActionIntent {
    fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::StateChanging => "state_changing",
            Self::Sensitive => "sensitive",
        }
    }
}

/// 保守地分类需要用户/监护人审查的浏览器调用。
/// 所有点击和输入操作默认为状态变更，除非检测到敏感目标。
pub fn approval_class(name: &str, args: &Value) -> Option<BrowserApprovalClass> {
    if !matches!(name, "browser_click" | "browser_type") {
        return None;
    }
    let declared = args
        .get("intent")
        .and_then(Value::as_str)
        .unwrap_or("read_only")
        .to_ascii_lowercase();
    let haystack = [
        args.get("selector").and_then(Value::as_str).unwrap_or(""),
        args.get("text").and_then(Value::as_str).unwrap_or(""),
    ]
    .join(" ")
    .to_ascii_lowercase();
    let sensitive = [
        "password",
        "passwd",
        "otp",
        "one-time",
        "credit",
        "card",
        "payment",
        "purchase",
        "checkout",
        "delete",
        "remove account",
        "publish",
        "permission",
        "authorize",
        "api-key",
        "api_key",
        "secret",
        "token",
        "credential",
        "account",
        "login",
        "sign in",
        "sign up",
        "密码",
        "验证码",
        "支付",
        "购买",
        "删除",
        "发布",
        "授权",
        "登录",
    ]
    .iter()
    .any(|needle| haystack.contains(needle));
    if declared == "sensitive" || sensitive {
        return Some(BrowserApprovalClass::Sensitive);
    }
    Some(BrowserApprovalClass::StateChanging)
}

/// 在应用已记忆的审批前重新检查实际目标。防止通用选择器掩盖策略
/// 应捕获的敏感按钮或输入框。
pub async fn effective_approval_class(
    session_id: &str,
    name: &str,
    args: &Value,
) -> Option<BrowserApprovalClass> {
    let base = approval_class(name, args)?;
    if base == BrowserApprovalClass::Sensitive {
        return Some(base);
    }
    let (_, session) = browser_session(session_id).await.ok()?;
    let mut session = session.lock().await;
    let selector = serde_json::to_string(&args.get("selector").and_then(Value::as_str))
        .unwrap_or_else(|_| "null".to_string());
    let text = serde_json::to_string(&args.get("text").and_then(Value::as_str))
        .unwrap_or_else(|_| "null".to_string());
    let expression = format!(
        r#"(() => {{
          const selector = {selector}; const wanted = {text};
          let el = selector ? document.querySelector(selector) : null;
          if (!el && wanted) {{
            el = [...document.querySelectorAll('button,a,input,textarea,select,[role="button"],[role="link"],[contenteditable="true"]')]
              .find(node => ((node.innerText || node.getAttribute('aria-label') || '').trim() === wanted));
          }}
          if (!el) return JSON.stringify({{}});
          return JSON.stringify({{
            selector: [el.id, el.getAttribute('name'), el.getAttribute('type'), el.getAttribute('autocomplete'), el.getAttribute('aria-label'), el.getAttribute('placeholder')].filter(Boolean).join(' '),
            text: (el.innerText || el.getAttribute('aria-label') || '').trim().slice(0, 300)
          }});
        }})()"#
    );
    let live = session.evaluate_json(&expression).await.ok()?;
    let enriched = json!({
        "intent": args.get("intent").cloned().unwrap_or(Value::Null),
        "selector": format!(
            "{} {}",
            args.get("selector").and_then(Value::as_str).unwrap_or(""),
            live.get("selector").and_then(Value::as_str).unwrap_or("")
        ),
        "text": format!(
            "{} {}",
            args.get("text").and_then(Value::as_str).unwrap_or(""),
            live.get("text").and_then(Value::as_str).unwrap_or("")
        ),
    });
    approval_class(name, &enriched).or(Some(base))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserApprovalRule {
    pub origin: String,
    pub action_class: String,
}

fn approvals_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("browser-approvals.json")
}

fn approval_rules_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

pub fn load_approval_rules(memory_dir: &Path) -> Vec<BrowserApprovalRule> {
    std::fs::read_to_string(approvals_path(memory_dir))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

pub fn approval_rule_matches(memory_dir: &Path, origin: &str, class: BrowserApprovalClass) -> bool {
    load_approval_rules(memory_dir)
        .iter()
        .any(|rule| rule.origin == origin && rule.action_class == class.as_str())
}

pub fn add_approval_rule(
    memory_dir: &Path,
    origin: &str,
    class: BrowserApprovalClass,
) -> anyhow::Result<()> {
    let _guard = approval_rules_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("browser approval lock is poisoned"))?;
    let path = approvals_path(memory_dir);
    let mut rules = load_approval_rules(memory_dir);
    let rule = BrowserApprovalRule {
        origin: origin.to_string(),
        action_class: class.as_str().to_string(),
    };
    if !rules.contains(&rule) {
        rules.push(rule);
        rules.sort_by(|a, b| (&a.origin, &a.action_class).cmp(&(&b.origin, &b.action_class)));
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(&rules)?)?;
    std::fs::rename(temp, path)?;
    Ok(())
}

pub fn remove_approval_rule(
    memory_dir: &Path,
    origin: &str,
    action_class: &str,
) -> anyhow::Result<()> {
    let _guard = approval_rules_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("browser approval lock is poisoned"))?;
    let path = approvals_path(memory_dir);
    let mut rules = load_approval_rules(memory_dir);
    rules.retain(|rule| rule.origin != origin || rule.action_class != action_class);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(&rules)?)?;
    std::fs::rename(temp, path)?;
    Ok(())
}

fn settings_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("browser").join("settings.json")
}

fn browser_settings_lock() -> &'static std::sync::Mutex<()> {
    static LOCK: OnceLock<std::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| std::sync::Mutex::new(()))
}

pub fn load_browser_settings(memory_dir: &Path) -> BrowserSettings {
    std::fs::read_to_string(settings_path(memory_dir))
        .ok()
        .and_then(|raw| serde_json::from_str::<BrowserSettings>(&raw).ok())
        .and_then(|settings| settings.normalized().ok())
        .unwrap_or_default()
}

pub fn save_browser_settings(
    memory_dir: &Path,
    settings: BrowserSettings,
) -> anyhow::Result<BrowserSettings> {
    let settings = settings.normalized()?;
    let _guard = browser_settings_lock()
        .lock()
        .map_err(|_| anyhow::anyhow!("browser settings lock is poisoned"))?;
    let path = settings_path(memory_dir);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let temp = path.with_extension("json.tmp");
    std::fs::write(&temp, serde_json::to_vec_pretty(&settings)?)?;
    std::fs::rename(temp, path)?;
    Ok(settings)
}

pub fn browser_data_dir(memory_dir: &Path) -> PathBuf {
    memory_dir.join("browser")
}

pub async fn current_origin(session_id: &str) -> Option<String> {
    let (_, session) = browser_session(session_id).await.ok()?;
    let mut session = session.lock().await;
    session
        .evaluate("location.origin")
        .await
        .ok()?
        .as_str()
        .map(str::to_string)
}

pub fn register(registry: &mut ToolRegistry) {
    for (name, description, schema, icon) in [
        (
            "browser_open",
            "Open a URL in the task-bound browser and return a DOM/accessibility snapshot plus screenshot. Use this when the user asks to inspect or operate a web page, and after starting a local development server. A URL merely mentioned for reading should normally use web_fetch instead.",
            schema_for_args::<BrowserOpenArgs>(),
            "panel-top-open",
        ),
        (
            "browser_snapshot",
            "Inspect the current task-bound page. Returns URL, title, visible text, interactive elements with stable selectors, and optionally a refreshed screenshot.",
            schema_for_args::<BrowserSnapshotArgs>(),
            "scan-search",
        ),
        (
            "browser_click",
            "Click an element in the current task-bound page by CSS selector or visible text, then return a refreshed snapshot. Set intent=state_changing or sensitive for submissions, login, permissions, publishing, deletion, purchase, or other remote side effects.",
            schema_for_args::<BrowserClickArgs>(),
            "mouse-pointer-click",
        ),
        (
            "browser_type",
            "Type text into an element in the current task-bound page, then return a refreshed snapshot. Never type passwords, OTPs, tokens, payment data, or other secrets. Set intent=state_changing when the value participates in a remote mutation.",
            schema_for_args::<BrowserTypeArgs>(),
            "text-cursor-input",
        ),
        (
            "browser_scroll",
            "Scroll the current task-bound page by a pixel delta and return a refreshed snapshot.",
            schema_for_args::<BrowserScrollArgs>(),
            "move-vertical",
        ),
        (
            "browser_wait",
            "Wait for a CSS selector or visible text to appear in the current task-bound page, then return a refreshed snapshot.",
            schema_for_args::<BrowserWaitArgs>(),
            "clock-3",
        ),
        (
            "browser_screenshot",
            "Capture the current task-bound page and return its latest screenshot and DOM snapshot.",
            schema_for_args::<BrowserSnapshotArgs>(),
            "camera",
        ),
        (
            "browser_tabs",
            "List the task-bound browser tabs and identify the active tab.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
            "panels-top-left",
        ),
        (
            "browser_tab_open",
            "Open a URL in a new tab in the task-bound browser and make it active.",
            schema_for_args::<BrowserOpenArgs>(),
            "square-plus",
        ),
        (
            "browser_tab_switch",
            "Switch the active task-bound browser tab by tab_id and return a fresh snapshot.",
            schema_for_args::<BrowserTabArgs>(),
            "panel-top",
        ),
        (
            "browser_tab_close",
            "Close a task-bound browser tab by tab_id.",
            schema_for_args::<BrowserTabArgs>(),
            "square-x",
        ),
        (
            "browser_back",
            "Navigate the active browser tab backward and return a fresh snapshot.",
            schema_for_args::<BrowserHistoryArgs>(),
            "arrow-left",
        ),
        (
            "browser_forward",
            "Navigate the active browser tab forward and return a fresh snapshot.",
            schema_for_args::<BrowserHistoryArgs>(),
            "arrow-right",
        ),
        (
            "browser_reload",
            "Reload the active browser tab and return a fresh snapshot.",
            schema_for_args::<BrowserHistoryArgs>(),
            "refresh-cw",
        ),
        (
            "browser_downloads",
            "List files downloaded by the task-bound browser.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
            "download",
        ),
        (
            "browser_close",
            "Close the task-bound browser session and release its isolated profile.",
            json!({"type":"object","properties":{},"additionalProperties":false}),
            "panel-top-close",
        ),
    ] {
        registry.register(ToolEntry {
            name: name.to_string(),
            toolset: "browser".to_string(),
            description: description.to_string(),
            schema,
            check_fn: Some(Box::new(browser_available)),
            icon,
            exclusive_access: true,
            ..ToolEntry::lifecycle_defaults()
        });
    }
}

crate::submit_builtin_tool! {
    register: register,
    names: ["browser_open", "browser_snapshot", "browser_click", "browser_type", "browser_scroll", "browser_wait", "browser_screenshot", "browser_tabs", "browser_tab_open", "browser_tab_switch", "browser_tab_close", "browser_back", "browser_forward", "browser_reload", "browser_downloads", "browser_close"],
    async_named: dispatch,
}

pub async fn dispatch(
    ctx: &mut ToolContext<'_>,
    name: &str,
    args: &Value,
) -> anyhow::Result<String> {
    match name {
        "browser_open" => {
            let parsed: BrowserOpenArgs = serde_json::from_value(args.clone())?;
            open(ctx, parsed).await
        }
        "browser_snapshot" => {
            let parsed: BrowserSnapshotArgs = serde_json::from_value(args.clone())?;
            snapshot(ctx, parsed).await
        }
        "browser_click" => {
            let parsed: BrowserClickArgs = serde_json::from_value(args.clone())?;
            click(ctx, parsed).await
        }
        "browser_type" => {
            let parsed: BrowserTypeArgs = serde_json::from_value(args.clone())?;
            type_text(ctx, parsed).await
        }
        "browser_scroll" => {
            let parsed: BrowserScrollArgs = serde_json::from_value(args.clone())?;
            scroll(ctx, parsed).await
        }
        "browser_wait" => {
            let parsed: BrowserWaitArgs = serde_json::from_value(args.clone())?;
            wait_for(ctx, parsed).await
        }
        "browser_screenshot" => {
            let parsed: BrowserSnapshotArgs = serde_json::from_value(args.clone())?;
            snapshot(
                ctx,
                BrowserSnapshotArgs {
                    screenshot: Some(true),
                    ..parsed
                },
            )
            .await
        }
        "browser_tabs" => tabs(ctx).await,
        "browser_tab_open" => {
            let mut parsed: BrowserOpenArgs = serde_json::from_value(args.clone())?;
            parsed.new_tab = true;
            open(ctx, parsed).await
        }
        "browser_tab_switch" => {
            let parsed: BrowserTabArgs = serde_json::from_value(args.clone())?;
            switch_tab(ctx, parsed).await
        }
        "browser_tab_close" => {
            let parsed: BrowserTabArgs = serde_json::from_value(args.clone())?;
            close_tab(ctx, parsed).await
        }
        "browser_back" => {
            let parsed: BrowserHistoryArgs = serde_json::from_value(args.clone())?;
            history(ctx, -1, parsed.wait_ms).await
        }
        "browser_forward" => {
            let parsed: BrowserHistoryArgs = serde_json::from_value(args.clone())?;
            history(ctx, 1, parsed.wait_ms).await
        }
        "browser_reload" => {
            let parsed: BrowserHistoryArgs = serde_json::from_value(args.clone())?;
            reload(ctx, parsed.wait_ms).await
        }
        "browser_downloads" => downloads(ctx).await,
        "browser_close" => close(ctx).await,
        _ => anyhow::bail!("unsupported browser tool: {name}"),
    }
}

#[derive(Default)]
struct BrowserManager {
    sessions: HashMap<String, BrowserWorkspace>,
}

struct BrowserWorkspace {
    tabs: Vec<BrowserTab>,
    active_tab_id: String,
    output_dir: PathBuf,
    preview_server: Option<PreviewServer>,
}

struct PreviewServer {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for PreviewServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

struct BrowserTab {
    id: String,
    title: String,
    url: String,
    favicon_url: Option<String>,
    session: Arc<Mutex<BrowserSession>>,
}

struct BrowserSession {
    child: Child,
    socket: DevtoolsSocket,
    next_id: u64,
    output_dir: PathBuf,
    page_allows_loopback: bool,
    loopback_enabled: bool,
    download_dir: PathBuf,
    downloads_enabled: bool,
    viewport_width: u32,
    viewport_height: u32,
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

fn manager() -> &'static Mutex<BrowserManager> {
    static MANAGER: OnceLock<Mutex<BrowserManager>> = OnceLock::new();
    MANAGER.get_or_init(|| Mutex::new(BrowserManager::default()))
}

pub async fn update_browser_settings(
    memory_dir: &Path,
    settings: BrowserSettings,
) -> anyhow::Result<BrowserSettings> {
    let settings = save_browser_settings(memory_dir, settings)?;
    let sessions = {
        let manager = manager().lock().await;
        manager
            .sessions
            .values()
            .flat_map(|workspace| workspace.tabs.iter().map(|tab| tab.session.clone()))
            .collect::<Vec<_>>()
    };
    futures::future::join_all(sessions.into_iter().map(|session| {
        let settings = settings.clone();
        async move {
            let _ = session.lock().await.apply_runtime_settings(&settings).await;
        }
    }))
    .await;
    Ok(settings)
}

async fn browser_session(session_id: &str) -> anyhow::Result<(String, Arc<Mutex<BrowserSession>>)> {
    let manager = manager().lock().await;
    let workspace = manager.sessions.get(session_id).ok_or_else(|| {
        anyhow::anyhow!("browser session is disconnected; call browser_open to restore it")
    })?;
    let tab = workspace
        .tabs
        .iter()
        .find(|tab| tab.id == workspace.active_tab_id)
        .ok_or_else(|| anyhow::anyhow!("browser session has no active tab"))?;
    Ok((tab.id.clone(), tab.session.clone()))
}

pub fn browser_available() -> bool {
    find_browser_executable().is_some()
}

fn find_browser_executable() -> Option<PathBuf> {
    if let Some(explicit) = std::env::var_os("ASTRO_BROWSER_EXECUTABLE") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return Some(path);
        }
    }

    #[cfg(target_os = "macos")]
    let candidates = [
        "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
        "/Applications/Chromium.app/Contents/MacOS/Chromium",
        "/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge",
        "/Applications/Brave Browser.app/Contents/MacOS/Brave Browser",
    ];
    #[cfg(target_os = "linux")]
    let candidates = [
        "/usr/bin/google-chrome",
        "/usr/bin/google-chrome-stable",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/microsoft-edge",
    ];
    #[cfg(target_os = "windows")]
    let candidates = [
        r"C:\Program Files\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe",
        r"C:\Program Files\Microsoft\Edge\Application\msedge.exe",
        r"C:\Program Files\BraveSoftware\Brave-Browser\Application\brave.exe",
    ];
    #[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
    let candidates: [&str; 0] = [];

    candidates
        .into_iter()
        .map(PathBuf::from)
        .find(|path| path.is_file())
        .or_else(find_playwright_browser)
}

fn find_playwright_browser() -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from)?;
    #[cfg(target_os = "macos")]
    let roots = [home.join("Library/Caches/ms-playwright")];
    #[cfg(not(target_os = "macos"))]
    let roots = [home.join(".cache/ms-playwright")];

    fn visit(dir: &Path, depth: usize) -> Option<PathBuf> {
        if depth == 0 {
            return None;
        }
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                if let Some(found) = visit(&path, depth - 1) {
                    return Some(found);
                }
            } else if path.is_file() {
                let name = path
                    .file_name()
                    .and_then(|value| value.to_str())
                    .unwrap_or("");
                if matches!(
                    name,
                    "Chromium" | "chrome" | "headless_shell" | "chrome-headless-shell"
                ) {
                    return Some(path);
                }
            }
        }
        None
    }

    roots.iter().find_map(|root| visit(root, 6))
}

fn session_dir_for(memory_dir: &Path, session_id: &str) -> PathBuf {
    let safe = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    memory_dir.join("browser").join(safe)
}

fn normalize_http_url(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    let mut parsed =
        reqwest::Url::parse(trimmed).map_err(|e| anyhow::anyhow!("invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("browser only supports http(s) URLs");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        anyhow::bail!("browser URLs must not contain embedded credentials");
    }
    if parsed
        .host_str()
        .and_then(|host| host.parse::<IpAddr>().ok())
        .is_some_and(|ip| ip.is_unspecified())
    {
        parsed
            .set_host(Some("127.0.0.1"))
            .map_err(|_| anyhow::anyhow!("failed to normalize local development URL"))?;
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL is missing a host"))?;
    if host.is_empty() {
        anyhow::bail!("URL is missing a host");
    }
    Ok(parsed.to_string())
}

fn validate_url(raw: &str) -> anyhow::Result<String> {
    let normalized = normalize_http_url(raw)?;
    let parsed = reqwest::Url::parse(&normalized)?;
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL is missing a host"))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if !loopback {
        crate::engine::network::assert_browser_http_url(parsed.as_str())?;
    }
    Ok(normalized)
}

fn validate_url_for_settings(raw: &str, settings: &BrowserSettings) -> anyhow::Result<String> {
    let url = validate_url(raw)?;
    if is_loopback_url(&url) && !settings.allow_loopback {
        anyhow::bail!("local development addresses are disabled in Browser settings");
    }
    Ok(url)
}

/// Validate a URL before it is loaded by the desktop's native child WebView.
///
/// Keeping this at the browser-tool boundary makes the visible WebView follow
/// the same public-network and loopback settings as the CDP automation session.
pub fn validate_live_webview_url(memory_dir: &Path, raw: &str) -> anyhow::Result<String> {
    let settings = load_browser_settings(memory_dir);
    validate_url_for_settings(raw, &settings)
}

fn is_loopback_url(raw: &str) -> bool {
    reqwest::Url::parse(raw)
        .ok()
        .and_then(|url| url.host_str().map(str::to_string))
        .is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
        })
}

fn download_entries(output_dir: &Path) -> Vec<Value> {
    let directory = output_dir.join("downloads");
    let mut entries = std::fs::read_dir(directory)
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let metadata = entry.metadata().ok()?;
            if !metadata.is_file() {
                return None;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let downloading = name.ends_with(".crdownload");
            let modified_at = metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| duration.as_millis() as u64)
                .unwrap_or(0);
            Some(json!({
                "name": name,
                "path": path.to_string_lossy(),
                "size": metadata.len(),
                "status": if downloading { "downloading" } else { "complete" },
                "updated_at": modified_at,
            }))
        })
        .collect::<Vec<_>>();
    entries.sort_by_key(|entry| std::cmp::Reverse(entry["updated_at"].as_u64().unwrap_or(0)));
    entries
}

async fn decorate_result(
    session_id: &str,
    tab_id: &str,
    mut result: Value,
) -> anyhow::Result<String> {
    let mut manager = manager().lock().await;
    let workspace = manager
        .sessions
        .get_mut(session_id)
        .ok_or_else(|| anyhow::anyhow!("browser session was closed"))?;
    if let Some(tab) = workspace.tabs.iter_mut().find(|tab| tab.id == tab_id) {
        if let Some(url) = result.get("url").and_then(Value::as_str) {
            tab.url = url.to_string();
        }
        if let Some(title) = result.get("title").and_then(Value::as_str) {
            tab.title = title.to_string();
        }
        if let Some(favicon_url) = result.get("favicon_url") {
            tab.favicon_url = favicon_url.as_str().map(str::to_string);
        }
    }
    result["session_id"] = Value::String(session_id.to_string());
    result["active_tab_id"] = Value::String(workspace.active_tab_id.clone());
    result["tabs"] = Value::Array(
        workspace
            .tabs
            .iter()
            .map(|tab| {
                json!({
                    "id": tab.id,
                    "title": tab.title,
                    "url": tab.url,
                    "favicon_url": tab.favicon_url,
                    "active": tab.id == workspace.active_tab_id,
                })
            })
            .collect(),
    );
    result["downloads"] = Value::Array(download_entries(&workspace.output_dir));
    Ok(result.to_string())
}

async fn open_for_session(
    session_id: &str,
    memory_dir: &Path,
    args: BrowserOpenArgs,
) -> anyhow::Result<String> {
    let settings = load_browser_settings(memory_dir);
    let url = validate_url_for_settings(&args.url, &settings)?;
    let existing = {
        let manager = manager().lock().await;
        manager.sessions.get(session_id).and_then(|workspace| {
            workspace
                .tabs
                .iter()
                .find(|tab| tab.id == workspace.active_tab_id)
                .map(|tab| (tab.id.clone(), tab.session.clone()))
        })
    };

    if !args.new_tab {
        if let Some((tab_id, session)) = existing {
            let mut session = session.lock().await;
            session.apply_runtime_settings(&settings).await?;
            session.navigate(&url).await?;
            session.wait_ready(args.wait_ms).await?;
            let result = session.snapshot(true).await?;
            drop(session);
            return decorate_result(session_id, &tab_id, result).await;
        }
    }

    let output_dir = session_dir_for(memory_dir, session_id);
    tokio::fs::create_dir_all(output_dir.join("downloads")).await?;
    let tab_id = uuid::Uuid::new_v4().to_string();
    let tab_dir = output_dir.join("tabs").join(&tab_id);
    let mut session =
        BrowserSession::launch(&url, tab_dir, output_dir.join("downloads"), &settings).await?;
    session.wait_ready(args.wait_ms).await?;
    let result = session.snapshot(true).await?;
    let session = Arc::new(Mutex::new(session));
    {
        let mut manager = manager().lock().await;
        let workspace = manager
            .sessions
            .entry(session_id.to_string())
            .or_insert_with(|| BrowserWorkspace {
                tabs: Vec::new(),
                active_tab_id: tab_id.clone(),
                output_dir: output_dir.clone(),
                preview_server: None,
            });
        workspace.tabs.push(BrowserTab {
            id: tab_id.clone(),
            title: result
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            url: result
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or(&url)
                .to_string(),
            favicon_url: result
                .get("favicon_url")
                .and_then(Value::as_str)
                .map(str::to_string),
            session,
        });
        workspace.active_tab_id = tab_id.clone();
    }
    decorate_result(session_id, &tab_id, result).await
}

async fn open(ctx: &ToolContext<'_>, args: BrowserOpenArgs) -> anyhow::Result<String> {
    open_for_session(&ctx.session_id, &ctx.memory_dir, args).await
}

async fn snapshot(ctx: &ToolContext<'_>, args: BrowserSnapshotArgs) -> anyhow::Result<String> {
    snapshot_for_session(
        &ctx.session_id,
        args.screenshot.unwrap_or(true),
        args.wait_ms,
    )
    .await
}

async fn snapshot_for_session(
    session_id: &str,
    screenshot: bool,
    wait_ms: Option<u64>,
) -> anyhow::Result<String> {
    let (tab_id, session) = browser_session(session_id).await?;
    let mut session = session.lock().await;
    session.wait_ready(wait_ms).await?;
    let result = session.snapshot(screenshot).await?;
    drop(session);
    decorate_result(session_id, &tab_id, result).await
}

async fn click(ctx: &ToolContext<'_>, args: BrowserClickArgs) -> anyhow::Result<String> {
    if args.selector.as_deref().is_none_or(str::is_empty)
        && args.text.as_deref().is_none_or(str::is_empty)
    {
        anyhow::bail!("browser_click requires selector or text");
    }
    let (tab_id, session) = browser_session(&ctx.session_id).await?;
    let mut session = session.lock().await;
    let selector = serde_json::to_string(&args.selector)?;
    let text = serde_json::to_string(&args.text)?;
    let expression = format!(
        r#"(() => {{
          const selector = {selector}; const wanted = {text};
          const visible = node => {{
            const rect = node.getBoundingClientRect(); const style = getComputedStyle(node);
            return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
          }};
          let el = selector ? document.querySelector(selector) : null;
          if (!el && wanted) {{
            el = [...document.querySelectorAll('button,a,input,[role="button"],[role="link"],summary')]
              .find(node => visible(node) && ((node.innerText || node.value || node.getAttribute('aria-label') || '').trim() === wanted));
          }}
          if (!el) return JSON.stringify({{ok:false,error:'element_not_found'}});
          if (!visible(el)) return JSON.stringify({{ok:false,error:'element_not_visible'}});
          if (el.disabled || el.getAttribute('aria-disabled') === 'true') return JSON.stringify({{ok:false,error:'element_disabled'}});
          el.scrollIntoView({{block:'center',inline:'center'}}); el.click();
          return JSON.stringify({{ok:true,tag:el.tagName.toLowerCase(),text:(el.innerText || el.getAttribute('aria-label') || '').trim().slice(0,160)}});
        }})()"#
    );
    let action = session.evaluate_json(&expression).await?;
    if action.get("ok").and_then(Value::as_bool) != Some(true) {
        anyhow::bail!(
            "browser click failed: {}",
            action
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    tokio::time::sleep(Duration::from_millis(
        args.wait_ms.unwrap_or(500).min(5_000),
    ))
    .await;
    session.wait_ready(Some(DEFAULT_WAIT_MS)).await?;
    let mut result = session.snapshot(true).await?;
    result["action"] = json!({"kind":"click","intent":args.intent.as_str(),"result":action});
    drop(session);
    decorate_result(&ctx.session_id, &tab_id, result).await
}

async fn type_text(ctx: &ToolContext<'_>, args: BrowserTypeArgs) -> anyhow::Result<String> {
    if args.text.len() > 10_000 {
        anyhow::bail!("browser_type text exceeds 10000 characters");
    }
    let (tab_id, session) = browser_session(&ctx.session_id).await?;
    let mut session = session.lock().await;
    let selector = serde_json::to_string(&args.selector)?;
    let text = serde_json::to_string(&args.text)?;
    let expression = format!(
        r#"(() => {{
          const el = document.querySelector({selector}); const value = {text};
          if (!el) return JSON.stringify({{ok:false,error:'element_not_found'}});
          const type = (el.getAttribute('type') || '').toLowerCase();
          const autocomplete = (el.getAttribute('autocomplete') || '').toLowerCase();
          const fieldContext = [el.id, el.getAttribute('name'), autocomplete, el.getAttribute('aria-label'), el.getAttribute('placeholder')].filter(Boolean).join(' ').toLowerCase();
          if (type === 'password' || ['one-time-code','cc-number','cc-csc','cc-exp'].includes(autocomplete) || /(password|passwd|otp|token|secret|api.?key|credit|card|验证码|密码|支付)/.test(fieldContext))
            return JSON.stringify({{ok:false,error:'sensitive_input_blocked'}});
          if (el.disabled || el.readOnly) return JSON.stringify({{ok:false,error:'element_not_editable'}});
          el.focus();
          if (el.isContentEditable) {{
            el.textContent = value;
          }} else {{
            const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : el instanceof HTMLSelectElement ? HTMLSelectElement.prototype : HTMLInputElement.prototype;
            const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set;
            if (setter) setter.call(el, value); else el.value = value;
          }}
          el.dispatchEvent(new InputEvent('input', {{bubbles:true,inputType:'insertText',data:value}}));
          el.dispatchEvent(new Event('change', {{bubbles:true}}));
          return JSON.stringify({{ok:true,tag:el.tagName.toLowerCase(),type}});
        }})()"#
    );
    let action = session.evaluate_json(&expression).await?;
    if action.get("ok").and_then(Value::as_bool) != Some(true) {
        anyhow::bail!(
            "browser type failed: {}",
            action
                .get("error")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
        );
    }
    let mut result = session.snapshot(true).await?;
    result["action"] = json!({"kind":"type","intent":args.intent.as_str(),"result":action});
    drop(session);
    decorate_result(&ctx.session_id, &tab_id, result).await
}

async fn scroll(ctx: &ToolContext<'_>, args: BrowserScrollArgs) -> anyhow::Result<String> {
    let (tab_id, session) = browser_session(&ctx.session_id).await?;
    let mut session = session.lock().await;
    let x = args.x.unwrap_or(0).clamp(-10_000, 10_000);
    let y = args.y.unwrap_or(600).clamp(-10_000, 10_000);
    session
        .evaluate(&format!(
            "window.scrollBy({{left:{x},top:{y},behavior:'auto'}}); true"
        ))
        .await?;
    let mut result = session.snapshot(true).await?;
    result["action"] = json!({"kind":"scroll","x":x,"y":y});
    drop(session);
    decorate_result(&ctx.session_id, &tab_id, result).await
}

async fn wait_for(ctx: &ToolContext<'_>, args: BrowserWaitArgs) -> anyhow::Result<String> {
    if args.selector.as_deref().is_none_or(str::is_empty)
        && args.text.as_deref().is_none_or(str::is_empty)
    {
        anyhow::bail!("browser_wait requires selector or text");
    }
    let (tab_id, session) = browser_session(&ctx.session_id).await?;
    let mut session = session.lock().await;
    let selector = serde_json::to_string(&args.selector)?;
    let text = serde_json::to_string(&args.text)?;
    let expression = format!(
        r#"(() => {{ const selector = {selector}; const wanted = {text};
          const visible = el => {{
            if (!el) return false;
            const rect = el.getBoundingClientRect(); const style = getComputedStyle(el);
            return rect.width > 0 && rect.height > 0 && style.visibility !== 'hidden' && style.display !== 'none';
          }};
          if (selector) return visible(document.querySelector(selector));
          return [...document.querySelectorAll('body *')].some(el => visible(el) && (el.innerText || '').includes(wanted));
        }})()"#
    );
    let deadline = tokio::time::Instant::now()
        + Duration::from_millis(
            args.timeout_ms
                .unwrap_or(DEFAULT_WAIT_MS)
                .clamp(100, MAX_WAIT_MS),
        );
    loop {
        if session.evaluate(&expression).await?.as_bool() == Some(true) {
            let mut result = session.snapshot(true).await?;
            result["action"] = json!({"kind":"wait","matched":true});
            drop(session);
            return decorate_result(&ctx.session_id, &tab_id, result).await;
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("browser_wait timed out");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn tabs(ctx: &ToolContext<'_>) -> anyhow::Result<String> {
    snapshot_for_session(&ctx.session_id, false, None).await
}

async fn switch_tab(ctx: &ToolContext<'_>, args: BrowserTabArgs) -> anyhow::Result<String> {
    switch_tab_for_session(&ctx.session_id, &args.tab_id).await
}

async fn switch_tab_for_session(session_id: &str, tab_id: &str) -> anyhow::Result<String> {
    {
        let mut manager = manager().lock().await;
        let workspace = manager
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("browser session is disconnected"))?;
        if !workspace.tabs.iter().any(|tab| tab.id == tab_id) {
            anyhow::bail!("browser tab not found");
        }
        workspace.active_tab_id = tab_id.to_string();
    }
    snapshot_for_session(session_id, true, None).await
}

async fn close_tab(ctx: &ToolContext<'_>, args: BrowserTabArgs) -> anyhow::Result<String> {
    close_tab_for_session(&ctx.session_id, &args.tab_id).await
}

async fn close_tab_for_session(session_id: &str, tab_id: &str) -> anyhow::Result<String> {
    let (removed, next_tab_id) = {
        let mut manager = manager().lock().await;
        let workspace = manager
            .sessions
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("browser session is disconnected"))?;
        let index = workspace
            .tabs
            .iter()
            .position(|tab| tab.id == tab_id)
            .ok_or_else(|| anyhow::anyhow!("browser tab not found"))?;
        let removed = workspace.tabs.remove(index);
        if workspace.tabs.is_empty() {
            manager.sessions.remove(session_id);
            (removed, None)
        } else {
            if workspace.active_tab_id == tab_id {
                workspace.active_tab_id = workspace.tabs[index.saturating_sub(1)].id.clone();
            }
            (removed, Some(workspace.active_tab_id.clone()))
        }
    };
    let mut session = removed.session.lock().await;
    let _ = session.child.kill().await;
    drop(session);
    if let Some(next_tab_id) = next_tab_id {
        switch_tab_for_session(session_id, &next_tab_id).await
    } else {
        Ok(json!({
            "astro_browser": true,
            "session_id": session_id,
            "status": "closed",
            "tabs": [],
            "downloads": [],
        })
        .to_string())
    }
}

async fn history(
    ctx: &ToolContext<'_>,
    delta: i64,
    wait_ms: Option<u64>,
) -> anyhow::Result<String> {
    history_for_session(&ctx.session_id, delta, wait_ms).await
}

async fn history_for_session(
    session_id: &str,
    delta: i64,
    wait_ms: Option<u64>,
) -> anyhow::Result<String> {
    let (tab_id, session) = browser_session(session_id).await?;
    let mut session = session.lock().await;
    session.navigate_history(delta).await?;
    tokio::time::sleep(Duration::from_millis(120)).await;
    session.wait_ready(wait_ms).await?;
    let result = session.snapshot(true).await?;
    drop(session);
    decorate_result(session_id, &tab_id, result).await
}

async fn reload(ctx: &ToolContext<'_>, wait_ms: Option<u64>) -> anyhow::Result<String> {
    reload_for_session(&ctx.session_id, wait_ms).await
}

async fn reload_for_session(session_id: &str, wait_ms: Option<u64>) -> anyhow::Result<String> {
    let (tab_id, session) = browser_session(session_id).await?;
    let mut session = session.lock().await;
    session.command("Page.reload", json!({})).await?;
    tokio::time::sleep(Duration::from_millis(120)).await;
    session.wait_ready(wait_ms).await?;
    let result = session.snapshot(true).await?;
    drop(session);
    decorate_result(session_id, &tab_id, result).await
}

async fn downloads(ctx: &ToolContext<'_>) -> anyhow::Result<String> {
    downloads_for_session(&ctx.session_id).await
}

async fn downloads_for_session(session_id: &str) -> anyhow::Result<String> {
    let manager = manager().lock().await;
    let workspace = manager
        .sessions
        .get(session_id)
        .ok_or_else(|| anyhow::anyhow!("browser session is disconnected"))?;
    Ok(json!({
        "astro_browser": true,
        "session_id": session_id,
        "status": "connected",
        "active_tab_id": workspace.active_tab_id,
        "tabs": workspace.tabs.iter().map(|tab| json!({
            "id": tab.id,
            "title": tab.title,
            "url": tab.url,
            "favicon_url": tab.favicon_url,
            "active": tab.id == workspace.active_tab_id,
        })).collect::<Vec<_>>(),
        "downloads": download_entries(&workspace.output_dir),
    })
    .to_string())
}

pub async fn desktop_control(
    session_id: &str,
    memory_dir: &Path,
    action: &str,
    args: Value,
) -> anyhow::Result<String> {
    match action {
        "open" => open_for_session(session_id, memory_dir, serde_json::from_value(args)?).await,
        "snapshot" => {
            let args: BrowserSnapshotArgs = serde_json::from_value(args)?;
            snapshot_for_session(session_id, args.screenshot.unwrap_or(true), args.wait_ms).await
        }
        "new_tab" => {
            let args: BrowserDesktopNewTabArgs = serde_json::from_value(args)?;
            let url = args
                .url
                .filter(|url| !url.trim().is_empty())
                .unwrap_or_else(|| load_browser_settings(memory_dir).home_page);
            open_for_session(
                session_id,
                memory_dir,
                BrowserOpenArgs {
                    url,
                    wait_ms: args.wait_ms,
                    new_tab: true,
                },
            )
            .await
        }
        "switch_tab" => {
            let args: BrowserTabArgs = serde_json::from_value(args)?;
            switch_tab_for_session(session_id, &args.tab_id).await
        }
        "close_tab" => {
            let args: BrowserTabArgs = serde_json::from_value(args)?;
            close_tab_for_session(session_id, &args.tab_id).await
        }
        "back" => history_for_session(session_id, -1, None).await,
        "forward" => history_for_session(session_id, 1, None).await,
        "reload" => reload_for_session(session_id, None).await,
        "downloads" => downloads_for_session(session_id).await,
        "click_point" => {
            let args: BrowserPointArgs = serde_json::from_value(args)?;
            let (tab_id, session) = browser_session(session_id).await?;
            let mut session = session.lock().await;
            session.click_point(args.x, args.y).await?;
            tokio::time::sleep(Duration::from_millis(
                args.wait_ms.unwrap_or(350).min(5_000),
            ))
            .await;
            let result = session.snapshot(true).await?;
            drop(session);
            decorate_result(session_id, &tab_id, result).await
        }
        "key" => {
            let args: BrowserKeyArgs = serde_json::from_value(args)?;
            let (tab_id, session) = browser_session(session_id).await?;
            let mut session = session.lock().await;
            session.press_key(&args.key).await?;
            let result = session.snapshot(true).await?;
            drop(session);
            decorate_result(session_id, &tab_id, result).await
        }
        "scroll" => {
            let args: BrowserScrollArgs = serde_json::from_value(args)?;
            let (tab_id, session) = browser_session(session_id).await?;
            let mut session = session.lock().await;
            let x = args.x.unwrap_or(0).clamp(-10_000, 10_000);
            let y = args.y.unwrap_or(600).clamp(-10_000, 10_000);
            session
                .evaluate(&format!(
                    "window.scrollBy({{left:{x},top:{y},behavior:'auto'}}); true"
                ))
                .await?;
            let result = session.snapshot(true).await?;
            drop(session);
            decorate_result(session_id, &tab_id, result).await
        }
        "resize" => {
            let args: BrowserViewportArgs = serde_json::from_value(args)?;
            let (tab_id, session) = browser_session(session_id).await?;
            let mut session = session.lock().await;
            session.resize_viewport(args.width, args.height).await?;
            let result = session.snapshot(args.screenshot.unwrap_or(true)).await?;
            drop(session);
            decorate_result(session_id, &tab_id, result).await
        }
        "close" => close_for_session(session_id).await,
        _ => anyhow::bail!("unsupported browser panel action: {action}"),
    }
}

/// Serve a trusted project HTML file through an ephemeral loopback server and
/// open it in the same browser workspace used by the Agent browser tools.
/// `html_override` lets the editor preview an unsaved or still-streaming draft
/// while relative assets continue to resolve from the file's directory.
pub async fn desktop_preview_file(
    session_id: &str,
    memory_dir: &Path,
    file_path: &Path,
    html_override: Option<String>,
) -> anyhow::Result<String> {
    let extension = file_path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default();
    if !matches!(extension.to_ascii_lowercase().as_str(), "html" | "htm") {
        anyhow::bail!("browser preview only supports HTML files");
    }
    let root = file_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("HTML file has no parent directory"))?
        .canonicalize()?;
    let file_name = file_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("HTML filename is not valid UTF-8"))?
        .to_string();
    let (server, url) = start_preview_server(root, file_name, html_override).await?;
    let result = open_for_session(
        session_id,
        memory_dir,
        BrowserOpenArgs {
            url,
            wait_ms: Some(DEFAULT_WAIT_MS),
            new_tab: false,
        },
    )
    .await;
    match result {
        Ok(result) => {
            let mut manager = manager().lock().await;
            let workspace = manager
                .sessions
                .get_mut(session_id)
                .ok_or_else(|| anyhow::anyhow!("browser session was closed"))?;
            workspace.preview_server = Some(server);
            Ok(result)
        }
        Err(error) => Err(error),
    }
}

async fn start_preview_server(
    root: PathBuf,
    entry_name: String,
    html_override: Option<String>,
) -> anyhow::Result<(PreviewServer, String)> {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let encoded_entry = urlencoding::encode(&entry_name);
    let url = format!("http://127.0.0.1:{}/{encoded_entry}", address.port());
    let task = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                break;
            };
            let root = root.clone();
            let entry_name = entry_name.clone();
            let html_override = html_override.clone();
            tokio::spawn(async move {
                let _ = serve_preview_request(stream, root, entry_name, html_override).await;
            });
        }
    });
    Ok((PreviewServer { task }, url))
}

async fn serve_preview_request(
    mut stream: TcpStream,
    root: PathBuf,
    entry_name: String,
    html_override: Option<String>,
) -> anyhow::Result<()> {
    let mut request = vec![0_u8; 16 * 1024];
    let read = tokio::time::timeout(Duration::from_secs(3), stream.read(&mut request)).await??;
    let first_line = String::from_utf8_lossy(&request[..read])
        .lines()
        .next()
        .unwrap_or_default()
        .to_string();
    let mut parts = first_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or("/");
    if !matches!(method, "GET" | "HEAD") {
        return write_preview_response(
            &mut stream,
            405,
            "text/plain",
            b"method not allowed",
            method,
        )
        .await;
    }

    let raw_path = target.split(['?', '#']).next().unwrap_or("/");
    let decoded = urlencoding::decode(raw_path.trim_start_matches('/'))?.into_owned();
    let relative = if decoded.is_empty() {
        PathBuf::from(&entry_name)
    } else {
        PathBuf::from(&decoded)
    };
    if relative.is_absolute()
        || relative.components().any(|component| {
            !matches!(
                component,
                std::path::Component::Normal(_) | std::path::Component::CurDir
            )
        })
    {
        return write_preview_response(&mut stream, 403, "text/plain", b"forbidden", method).await;
    }

    if relative == Path::new(&entry_name) {
        if let Some(html) = html_override.as_deref() {
            return write_preview_response(
                &mut stream,
                200,
                "text/html; charset=utf-8",
                html.as_bytes(),
                method,
            )
            .await;
        }
    }

    let requested = root.join(relative);
    let canonical = match requested.canonicalize() {
        Ok(path) if path.starts_with(&root) && path.is_file() => path,
        _ => {
            if requested.extension().is_none() {
                let fallback = if let Some(html) = html_override.as_deref() {
                    html.as_bytes().to_vec()
                } else {
                    tokio::fs::read(root.join(&entry_name)).await?
                };
                return write_preview_response(
                    &mut stream,
                    200,
                    "text/html; charset=utf-8",
                    &fallback,
                    method,
                )
                .await;
            }
            return write_preview_response(&mut stream, 404, "text/plain", b"not found", method)
                .await;
        }
    };
    let content = tokio::fs::read(&canonical).await?;
    write_preview_response(
        &mut stream,
        200,
        preview_content_type(&canonical),
        &content,
        method,
    )
    .await
}

async fn write_preview_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
    method: &str,
) -> anyhow::Result<()> {
    let reason = match status {
        200 => "OK",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        _ => "Error",
    };
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await?;
    if method != "HEAD" {
        stream.write_all(body).await?;
    }
    stream.shutdown().await?;
    Ok(())
}

fn preview_content_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "html" | "htm" => "text/html; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "json" => "application/json; charset=utf-8",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "avif" => "image/avif",
        "ico" => "image/x-icon",
        "txt" => "text/plain; charset=utf-8",
        "xml" => "application/xml; charset=utf-8",
        "webmanifest" => "application/manifest+json",
        "wasm" => "application/wasm",
        "pdf" => "application/pdf",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mp3" => "audio/mpeg",
        "wav" => "audio/wav",
        "ogg" => "audio/ogg",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "ttf" => "font/ttf",
        "otf" => "font/otf",
        _ => "application/octet-stream",
    }
}

async fn close(ctx: &ToolContext<'_>) -> anyhow::Result<String> {
    close_for_session(&ctx.session_id).await
}

async fn close_for_session(session_id: &str) -> anyhow::Result<String> {
    let workspace = manager().lock().await.sessions.remove(session_id);
    if let Some(workspace) = workspace {
        for tab in workspace.tabs {
            let mut session = tab.session.lock().await;
            let _ = session.child.kill().await;
        }
    }
    Ok(json!({
        "astro_browser": true,
        "session_id": session_id,
        "status": "closed"
    })
    .to_string())
}

impl BrowserSession {
    async fn launch(
        url: &str,
        output_dir: PathBuf,
        download_dir: PathBuf,
        settings: &BrowserSettings,
    ) -> anyhow::Result<Self> {
        let executable = find_browser_executable().ok_or_else(|| {
            anyhow::anyhow!(
                "no Chromium browser found; install Chrome, Chromium, Edge, Brave, or set ASTRO_BROWSER_EXECUTABLE"
            )
        })?;
        let profile_dir = output_dir.join("profile");
        tokio::fs::create_dir_all(&profile_dir).await?;
        tokio::fs::create_dir_all(&download_dir).await?;
        let mut child = Command::new(executable)
            .arg("--headless=new")
            .arg("--remote-debugging-address=127.0.0.1")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile_dir.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-component-update")
            .arg("--disable-sync")
            .arg("--metrics-recording-only")
            .arg(format!(
                "--window-size={},{}",
                settings.viewport_width, settings.viewport_height
            ))
            .arg("about:blank")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;

        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("browser stderr unavailable"))?;
        let mut lines = BufReader::new(stderr).lines();
        let browser_ws = tokio::time::timeout(Duration::from_secs(12), async {
            while let Some(line) = lines.next_line().await? {
                if let Some((_, ws)) = line.split_once("DevTools listening on ") {
                    return Ok::<_, std::io::Error>(ws.trim().to_string());
                }
            }
            Err(std::io::Error::other(
                "browser exited before DevTools became ready",
            ))
        })
        .await
        .map_err(|_| anyhow::anyhow!("timed out starting Chromium DevTools"))??;
        tokio::spawn(async move { while matches!(lines.next_line().await, Ok(Some(_))) {} });

        let port = reqwest::Url::parse(&browser_ws)?
            .port_or_known_default()
            .ok_or_else(|| anyhow::anyhow!("DevTools URL has no port"))?;
        let target = reqwest::Client::new()
            .put(format!("http://127.0.0.1:{port}/json/new?about:blank"))
            .send()
            .await?
            .error_for_status()?
            .json::<Value>()
            .await?;
        let target_ws = target
            .get("webSocketDebuggerUrl")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Chromium did not return a page debugger URL"))?;
        let (socket, _) = connect_async(target_ws).await?;
        let mut session = Self {
            child,
            socket,
            next_id: 0,
            output_dir,
            page_allows_loopback: is_loopback_url(url),
            loopback_enabled: settings.allow_loopback,
            download_dir,
            downloads_enabled: settings.downloads_enabled,
            viewport_width: settings.viewport_width,
            viewport_height: settings.viewport_height,
        };
        session.command("Page.enable", json!({})).await?;
        session.command("Runtime.enable", json!({})).await?;
        session.command("DOM.enable", json!({})).await?;
        let download_behavior = if settings.downloads_enabled {
            json!({
                "behavior":"allow",
                "downloadPath":session.download_dir.to_string_lossy(),
                "eventsEnabled":true
            })
        } else {
            json!({"behavior":"deny","eventsEnabled":true})
        };
        session
            .command("Browser.setDownloadBehavior", download_behavior)
            .await?;
        session
            .command(
                "Emulation.setDeviceMetricsOverride",
                json!({
                    "width":settings.viewport_width,
                    "height":settings.viewport_height,
                    "deviceScaleFactor":1,
                    "mobile":false
                }),
            )
            .await?;
        session
            .command(
                "Fetch.enable",
                json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]}),
            )
            .await?;
        let navigation = session.command("Page.navigate", json!({"url":url})).await?;
        if let Some(error) = navigation.get("errorText").and_then(Value::as_str) {
            if !error.is_empty() {
                anyhow::bail!("browser navigation failed: {error}");
            }
        }
        Ok(session)
    }

    async fn navigate(&mut self, url: &str) -> anyhow::Result<()> {
        let url = validate_url(url)?;
        if is_loopback_url(&url) && !self.loopback_enabled {
            anyhow::bail!("local development addresses are disabled in Browser settings");
        }
        self.page_allows_loopback = is_loopback_url(&url);
        let navigation = self.command("Page.navigate", json!({"url":url})).await?;
        if let Some(error) = navigation.get("errorText").and_then(Value::as_str) {
            if !error.is_empty() {
                anyhow::bail!("browser navigation failed: {error}");
            }
        }
        Ok(())
    }

    async fn apply_runtime_settings(&mut self, settings: &BrowserSettings) -> anyhow::Result<()> {
        self.loopback_enabled = settings.allow_loopback;
        if self.downloads_enabled != settings.downloads_enabled {
            let params = if settings.downloads_enabled {
                json!({
                    "behavior":"allow",
                    "downloadPath":self.download_dir.to_string_lossy(),
                    "eventsEnabled":true
                })
            } else {
                json!({"behavior":"deny","eventsEnabled":true})
            };
            self.command("Browser.setDownloadBehavior", params).await?;
            self.downloads_enabled = settings.downloads_enabled;
        }
        Ok(())
    }

    async fn navigate_history(&mut self, delta: i64) -> anyhow::Result<()> {
        let history = self.command("Page.getNavigationHistory", json!({})).await?;
        let current = history
            .get("currentIndex")
            .and_then(Value::as_i64)
            .unwrap_or(0);
        let entries = history
            .get("entries")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("browser navigation history unavailable"))?;
        let target_index = current + delta;
        if target_index < 0 || target_index >= entries.len() as i64 {
            return Ok(());
        }
        let entry_id = entries[target_index as usize]
            .get("id")
            .and_then(Value::as_i64)
            .ok_or_else(|| anyhow::anyhow!("browser history entry has no id"))?;
        self.command("Page.navigateToHistoryEntry", json!({"entryId":entry_id}))
            .await?;
        Ok(())
    }

    async fn click_point(&mut self, x: f64, y: f64) -> anyhow::Result<()> {
        let x = x.clamp(0.0, f64::from(self.viewport_width));
        let y = y.clamp(0.0, f64::from(self.viewport_height));
        self.command(
            "Input.dispatchMouseEvent",
            json!({"type":"mousePressed","x":x,"y":y,"button":"left","clickCount":1}),
        )
        .await?;
        self.command(
            "Input.dispatchMouseEvent",
            json!({"type":"mouseReleased","x":x,"y":y,"button":"left","clickCount":1}),
        )
        .await?;
        Ok(())
    }

    async fn resize_viewport(&mut self, width: u32, height: u32) -> anyhow::Result<()> {
        let width = width.clamp(320, 1_600);
        let height = height.clamp(240, 1_400);
        self.command(
            "Emulation.setDeviceMetricsOverride",
            json!({"width":width,"height":height,"deviceScaleFactor":1,"mobile":false}),
        )
        .await?;
        self.viewport_width = width;
        self.viewport_height = height;
        Ok(())
    }

    async fn press_key(&mut self, key: &str) -> anyhow::Result<()> {
        if key.chars().count() == 1 {
            self.command("Input.insertText", json!({"text":key}))
                .await?;
            return Ok(());
        }
        let code = match key {
            "Enter" => 13,
            "Backspace" => 8,
            "Tab" => 9,
            "Escape" => 27,
            "ArrowLeft" => 37,
            "ArrowUp" => 38,
            "ArrowRight" => 39,
            "ArrowDown" => 40,
            _ => anyhow::bail!("unsupported browser key"),
        };
        self.command(
            "Input.dispatchKeyEvent",
            json!({"type":"keyDown","key":key,"windowsVirtualKeyCode":code,"nativeVirtualKeyCode":code}),
        )
        .await?;
        self.command(
            "Input.dispatchKeyEvent",
            json!({"type":"keyUp","key":key,"windowsVirtualKeyCode":code,"nativeVirtualKeyCode":code}),
        )
        .await?;
        Ok(())
    }

    async fn command(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
        self.next_id += 1;
        let id = self.next_id;
        self.socket
            .send(Message::Text(
                json!({"id":id,"method":method,"params":params})
                    .to_string()
                    .into(),
            ))
            .await?;
        loop {
            let message = tokio::time::timeout(Duration::from_secs(30), self.socket.next())
                .await
                .map_err(|_| anyhow::anyhow!("browser command timed out: {method}"))?
                .ok_or_else(|| anyhow::anyhow!("browser connection closed"))??;
            let Message::Text(text) = message else {
                continue;
            };
            let payload: Value = serde_json::from_str(text.as_str())?;
            if payload.get("method").and_then(Value::as_str) == Some("Fetch.requestPaused") {
                self.handle_request_paused(&payload).await?;
                continue;
            }
            if payload.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = payload.get("error") {
                anyhow::bail!("browser command {method} failed: {error}");
            }
            return Ok(payload.get("result").cloned().unwrap_or(Value::Null));
        }
    }

    async fn handle_request_paused(&mut self, event: &Value) -> anyhow::Result<()> {
        let request_id = event
            .pointer("/params/requestId")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("Fetch.requestPaused missing requestId"))?;
        let url = event
            .pointer("/params/request/url")
            .and_then(Value::as_str)
            .unwrap_or("");
        let allowed = self.request_allowed(url);
        self.next_id += 1;
        let payload = if allowed {
            json!({"id":self.next_id,"method":"Fetch.continueRequest","params":{"requestId":request_id}})
        } else {
            json!({"id":self.next_id,"method":"Fetch.failRequest","params":{"requestId":request_id,"errorReason":"BlockedByClient"}})
        };
        self.socket
            .send(Message::Text(payload.to_string().into()))
            .await?;
        Ok(())
    }

    fn request_allowed(&mut self, raw: &str) -> bool {
        let Ok(url) = reqwest::Url::parse(raw) else {
            return matches!(raw, "about:blank")
                || raw.starts_with("data:")
                || raw.starts_with("blob:");
        };
        if !matches!(url.scheme(), "http" | "https") {
            return matches!(url.scheme(), "data" | "blob" | "about");
        }
        if !url.username().is_empty() || url.password().is_some() {
            return false;
        }
        let Some(host) = url.host_str() else {
            return false;
        };
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
        if loopback {
            self.page_allows_loopback && self.loopback_enabled
        } else {
            crate::engine::network::assert_browser_http_url(raw).is_ok()
        }
    }

    async fn evaluate(&mut self, expression: &str) -> anyhow::Result<Value> {
        let result = self
            .command(
                "Runtime.evaluate",
                json!({
                    "expression": expression,
                    "returnByValue": true,
                    "awaitPromise": true,
                    "userGesture": true
                }),
            )
            .await?;
        if let Some(exception) = result.get("exceptionDetails") {
            anyhow::bail!("page script failed: {exception}");
        }
        Ok(result
            .pointer("/result/value")
            .cloned()
            .unwrap_or(Value::Null))
    }

    async fn evaluate_json(&mut self, expression: &str) -> anyhow::Result<Value> {
        let raw = self
            .evaluate(expression)
            .await?
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("page script returned a non-string value"))?
            .to_string();
        Ok(serde_json::from_str(&raw)?)
    }

    async fn wait_ready(&mut self, wait_ms: Option<u64>) -> anyhow::Result<()> {
        let deadline = tokio::time::Instant::now()
            + Duration::from_millis(wait_ms.unwrap_or(DEFAULT_WAIT_MS).clamp(100, MAX_WAIT_MS));
        loop {
            let ready = self
                .evaluate("document.readyState")
                .await?
                .as_str()
                .is_some_and(|state| matches!(state, "interactive" | "complete"));
            if ready || tokio::time::Instant::now() >= deadline {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    async fn snapshot(&mut self, with_screenshot: bool) -> anyhow::Result<Value> {
        let expression = r#"(() => {
          const visible = el => { const r = el.getBoundingClientRect(); const s = getComputedStyle(el); return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none'; };
          const sensitive = el => {
            const type = (el.getAttribute('type') || '').toLowerCase();
            const context = [el.id, el.getAttribute('name'), el.getAttribute('autocomplete'), el.getAttribute('aria-label'), el.getAttribute('placeholder')].filter(Boolean).join(' ').toLowerCase();
            return type === 'password' || /(password|passwd|otp|token|secret|api.?key|credit|card|验证码|密码|支付)/.test(context);
          };
          const selector = el => {
            if (el.id) return '#' + CSS.escape(el.id);
            const testId = el.getAttribute('data-testid');
            if (testId) return `[data-testid="${CSS.escape(testId)}"]`;
            const name = el.getAttribute('name');
            if (name) return `${el.tagName.toLowerCase()}[name="${CSS.escape(name)}"]`;
            const parts = []; let node = el;
            while (node && node !== document.body && parts.length < 4) {
              let part = node.tagName.toLowerCase();
              const siblings = node.parentElement ? [...node.parentElement.children].filter(x => x.tagName === node.tagName) : [];
              if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
              parts.unshift(part); node = node.parentElement;
            }
            return parts.join(' > ');
          };
          const nodes = [...document.querySelectorAll('a,button,input,textarea,select,summary,[role="button"],[role="link"],[contenteditable="true"]')]
            .filter(visible).slice(0, 60).map(el => ({
              selector: selector(el), tag: el.tagName.toLowerCase(), role: el.getAttribute('role'),
              type: el.getAttribute('type'), text: (el.innerText || (sensitive(el) ? '' : el.value) || el.getAttribute('aria-label') || el.getAttribute('placeholder') || '').trim().slice(0, 180),
              disabled: Boolean(el.disabled || el.getAttribute('aria-disabled') === 'true')
            }));
          const iconLinks = [...document.querySelectorAll('link[rel]')].filter(el => el.getAttribute('href')?.trim());
          const faviconCandidate = iconLinks.find(el => el.rel.toLowerCase().split(/\s+/).includes('icon'))?.href
            || iconLinks.find(el => el.rel.toLowerCase().includes('apple-touch-icon'))?.href
            || '';
          const favicon = faviconCandidate.length <= 8192 ? faviconCandidate : '';
          return JSON.stringify({url:location.href,title:document.title,faviconUrl:favicon,text:(document.body?.innerText || '').slice(0,20000),interactive:nodes});
        })()"#;
        let mut page = self.evaluate_json(expression).await?;
        if let Some(text) = page.get("text").and_then(Value::as_str).map(str::to_string) {
            if text.len() > MAX_SNAPSHOT_CHARS {
                page["text"] =
                    Value::String(types::truncate_utf8(&text, MAX_SNAPSHOT_CHARS).to_string());
            }
        }
        let screenshot_path = if with_screenshot {
            Some(self.capture_screenshot().await?)
        } else {
            None
        };
        Ok(json!({
            "astro_browser": true,
            "status": "connected",
            "url": page.get("url").cloned().unwrap_or(Value::Null),
            "title": page.get("title").cloned().unwrap_or(Value::Null),
            "favicon_url": page.get("faviconUrl").cloned().unwrap_or(Value::Null),
            "snapshot": page,
            "screenshot_path": screenshot_path.map(|path| path.to_string_lossy().into_owned())
        }))
    }

    async fn capture_screenshot(&mut self) -> anyhow::Result<PathBuf> {
        let result = self
            .command(
                "Page.captureScreenshot",
                json!({"format":"png","fromSurface":true,"captureBeyondViewport":false}),
            )
            .await?;
        let encoded = result
            .get("data")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("browser screenshot returned no data"))?;
        let bytes = base64::engine::general_purpose::STANDARD.decode(encoded)?;
        let path = self.output_dir.join("latest.png");
        tokio::fs::write(&path, bytes).await?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_and_loopback_urls() {
        assert!(validate_url("https://1.1.1.1/a").is_ok());
        assert!(validate_url("http://127.0.0.1:1420").is_ok());
        assert!(validate_url("http://localhost:5173").is_ok());
        assert_eq!(
            validate_url("http://0.0.0.0:3000/app").unwrap(),
            "http://127.0.0.1:3000/app"
        );
    }

    #[test]
    fn rejects_non_http_and_private_lan_urls() {
        assert!(validate_url("file:///tmp/a.html").is_err());
        assert!(validate_url("http://192.168.1.2").is_err());
        assert!(validate_url("https://user:secret@example.com").is_err());
    }

    #[test]
    fn session_path_is_sanitized() {
        let value = "a/b:c"
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect::<String>();
        assert_eq!(value, "a_b_c");
    }

    #[test]
    fn approval_class_is_conservative_for_remote_side_effects() {
        assert_eq!(
            approval_class("browser_click", &json!({"text":"Save changes"})),
            Some(BrowserApprovalClass::StateChanging)
        );
        assert_eq!(
            approval_class("browser_click", &json!({"text":"Delete account"})),
            Some(BrowserApprovalClass::Sensitive)
        );
        assert_eq!(
            approval_class(
                "browser_type",
                &json!({"selector":"#search","text":"Astro"})
            ),
            Some(BrowserApprovalClass::StateChanging)
        );
        assert_eq!(
            approval_class("browser_click", &json!({"selector":"#api-token"})),
            Some(BrowserApprovalClass::Sensitive)
        );
        assert_eq!(approval_class("browser_snapshot", &json!({})), None);
    }

    #[test]
    fn browser_approval_rules_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        add_approval_rule(
            dir.path(),
            "https://example.com",
            BrowserApprovalClass::StateChanging,
        )
        .unwrap();
        assert!(approval_rule_matches(
            dir.path(),
            "https://example.com",
            BrowserApprovalClass::StateChanging
        ));
        assert!(!approval_rule_matches(
            dir.path(),
            "https://example.com",
            BrowserApprovalClass::Sensitive
        ));
        remove_approval_rule(dir.path(), "https://example.com", "state_changing").unwrap();
        assert!(load_approval_rules(dir.path()).is_empty());
    }

    #[test]
    fn browser_settings_round_trip_normalizes_values() {
        let dir = tempfile::tempdir().unwrap();
        let saved = save_browser_settings(
            dir.path(),
            BrowserSettings {
                home_page: "https://1.1.1.1".into(),
                viewport_width: 10_000,
                viewport_height: 100,
                allow_loopback: false,
                downloads_enabled: false,
            },
        )
        .unwrap();
        assert_eq!(saved.home_page, "https://1.1.1.1/");
        assert_eq!(saved.viewport_width, 1_600);
        assert_eq!(saved.viewport_height, 240);
        assert_eq!(load_browser_settings(dir.path()), saved);
    }

    #[test]
    fn browser_settings_reject_invalid_home_page_and_gate_loopback() {
        let dir = tempfile::tempdir().unwrap();
        assert!(save_browser_settings(
            dir.path(),
            BrowserSettings {
                home_page: "file:///tmp/index.html".into(),
                ..BrowserSettings::default()
            },
        )
        .is_err());
        assert!(save_browser_settings(
            dir.path(),
            BrowserSettings {
                home_page: "http://localhost:5173".into(),
                allow_loopback: false,
                ..BrowserSettings::default()
            },
        )
        .is_err());
        let settings = BrowserSettings {
            allow_loopback: false,
            ..BrowserSettings::default()
        };
        assert!(validate_url_for_settings("http://localhost:5173", &settings).is_err());
        assert!(validate_url_for_settings("https://1.1.1.1", &settings).is_ok());
    }

    #[test]
    fn desktop_resize_can_skip_an_unneeded_screenshot() {
        let default_args: BrowserViewportArgs =
            serde_json::from_value(json!({"width": 680, "height": 720})).unwrap();
        assert_eq!(default_args.screenshot, None);

        let live_args: BrowserViewportArgs =
            serde_json::from_value(json!({"width": 680, "height": 720, "screenshot": false}))
                .unwrap();
        assert_eq!(live_args.screenshot, Some(false));
    }

    #[tokio::test]
    async fn preview_server_serves_draft_and_relative_asset() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("index.html"), "stale").unwrap();
        std::fs::write(temp.path().join("app.css"), "body{color:red}").unwrap();
        let (server, url) = start_preview_server(
            temp.path().canonicalize().unwrap(),
            "index.html".into(),
            Some("<!doctype html><link rel='stylesheet' href='app.css'><h1>draft</h1>".into()),
        )
        .await
        .unwrap();
        let html = reqwest::get(&url).await.unwrap().text().await.unwrap();
        assert!(html.contains("<h1>draft</h1>"));
        let css = reqwest::get(format!("{}/app.css", url.trim_end_matches("index.html")))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(css, "body{color:red}");
        let route = reqwest::get(format!("{}/settings", url.trim_end_matches("index.html")))
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert!(route.contains("<h1>draft</h1>"));
        drop(server);
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires an installed Chromium-family browser"]
    async fn browser_live_round_trip() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        if !browser_available() {
            return;
        }
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            for _ in 0..3 {
                let Ok(Ok((mut socket, _))) =
                    tokio::time::timeout(Duration::from_secs(8), listener.accept()).await
                else {
                    break;
                };
                let mut request = [0_u8; 2048];
                let _ = socket.read(&mut request).await;
                let body = "<!doctype html><link rel='icon' href='data:image/png;base64,AA=='><title>Astro Browser Test</title><input id='api-token' value='secret-token'><button id='danger'>Delete account</button>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        let temp = tempfile::tempdir().unwrap();
        let mut session = BrowserSession::launch(
            &format!("http://{addr}"),
            temp.path().join("tab"),
            temp.path().join("downloads"),
            &BrowserSettings::default(),
        )
        .await
        .unwrap();
        session.wait_ready(Some(8_000)).await.unwrap();
        session.resize_viewport(640, 720).await.unwrap();
        let viewport = session
            .evaluate_json("JSON.stringify({width:innerWidth,height:innerHeight})")
            .await
            .unwrap();
        assert_eq!(viewport, json!({"width":640,"height":720}));
        let snapshot = session.snapshot(true).await.unwrap();
        assert_eq!(snapshot["title"], "Astro Browser Test");
        assert_eq!(snapshot["favicon_url"], "data:image/png;base64,AA==");
        assert!(snapshot["screenshot_path"]
            .as_str()
            .is_some_and(|path| Path::new(path).exists()));
        assert!(!snapshot.to_string().contains("secret-token"));

        let session_id = format!("browser-live-{}", std::process::id());
        let tab_id = "live-tab".to_string();
        manager().lock().await.sessions.insert(
            session_id.clone(),
            BrowserWorkspace {
                tabs: vec![BrowserTab {
                    id: tab_id.clone(),
                    title: "Astro Browser Test".into(),
                    url: format!("http://{addr}"),
                    favicon_url: Some("data:image/png;base64,AA==".into()),
                    session: Arc::new(Mutex::new(session)),
                }],
                active_tab_id: tab_id,
                output_dir: temp.path().to_path_buf(),
                preview_server: None,
            },
        );
        let decorated = snapshot_for_session(&session_id, false, None)
            .await
            .unwrap();
        let decorated: Value = serde_json::from_str(&decorated).unwrap();
        assert_eq!(
            decorated["tabs"][0]["favicon_url"],
            "data:image/png;base64,AA=="
        );
        assert_eq!(
            effective_approval_class(
                &session_id,
                "browser_click",
                &json!({"selector":"#danger","intent":"read_only"}),
            )
            .await,
            Some(BrowserApprovalClass::Sensitive)
        );
        close_for_session(&session_id).await.unwrap();
    }

    #[tokio::test(flavor = "current_thread")]
    #[ignore = "requires an installed Chromium-family browser"]
    async fn browser_workspace_multi_tab_round_trip() {
        if !browser_available() {
            return;
        }
        let temp = tempfile::tempdir().unwrap();
        let file = temp.path().join("index.html");
        std::fs::write(
            &file,
            "<!doctype html><title>Local Preview</title><h1>draft</h1>",
        )
        .unwrap();
        let session_id = format!("browser-tabs-{}", std::process::id());
        let first = desktop_preview_file(&session_id, temp.path(), &file, None)
            .await
            .unwrap();
        let first: Value = serde_json::from_str(&first).unwrap();
        let first_tab = first["active_tab_id"].as_str().unwrap().to_string();
        assert_eq!(first["tabs"].as_array().unwrap().len(), 1);

        let second = desktop_control(
            &session_id,
            temp.path(),
            "new_tab",
            json!({"url":first["url"],"new_tab":true}),
        )
        .await
        .unwrap();
        let second: Value = serde_json::from_str(&second).unwrap();
        let second_tab = second["active_tab_id"].as_str().unwrap().to_string();
        assert_ne!(first_tab, second_tab);
        assert_eq!(second["tabs"].as_array().unwrap().len(), 2);

        let switched = switch_tab_for_session(&session_id, &first_tab)
            .await
            .unwrap();
        let switched: Value = serde_json::from_str(&switched).unwrap();
        assert_eq!(switched["active_tab_id"], first_tab);
        let closed = close_tab_for_session(&session_id, &second_tab)
            .await
            .unwrap();
        let closed: Value = serde_json::from_str(&closed).unwrap();
        assert_eq!(closed["tabs"].as_array().unwrap().len(), 1);
        close_for_session(&session_id).await.unwrap();
    }
}
