//! Task-bound Chromium browser automation.
//!
//! Each chat session owns at most one isolated headless Chromium process. Tool
//! results are structured JSON so the desktop can render a live preview while
//! the model receives the same DOM snapshot and action result.

use std::collections::HashMap;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::OnceLock;
use std::time::Duration;

use base64::Engine;
use futures::{SinkExt, StreamExt};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, BufReader};
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

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserOpenArgs {
    /// Public http(s) URL, or a loopback URL for a local development server.
    pub url: String,
    /// Maximum time to wait for DOM readiness.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct BrowserSnapshotArgs {
    /// Capture and refresh the preview screenshot (default true).
    #[serde(default)]
    pub screenshot: Option<bool>,
    /// Maximum time to wait for DOM readiness.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserClickArgs {
    /// CSS selector. Prefer selectors returned by browser_snapshot.
    #[serde(default)]
    pub selector: Option<String>,
    /// Visible label fallback when no stable selector is available.
    #[serde(default)]
    pub text: Option<String>,
    /// Declared intent, used by the approval layer for state-changing actions.
    #[serde(default)]
    pub intent: BrowserActionIntent,
    /// Wait after the click before taking the next snapshot.
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct BrowserTypeArgs {
    /// CSS selector for the input, textarea, select, or editable element.
    pub selector: String,
    /// Text to enter. Passwords, OTPs, tokens, and payment data must never be supplied.
    pub text: String,
    /// Declared intent, used by the approval layer for sensitive submissions.
    #[serde(default)]
    pub intent: BrowserActionIntent,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct BrowserScrollArgs {
    /// Horizontal scroll delta in CSS pixels.
    #[serde(default)]
    pub x: Option<i64>,
    /// Vertical scroll delta in CSS pixels (default 600).
    #[serde(default)]
    pub y: Option<i64>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema, Default)]
pub struct BrowserWaitArgs {
    /// CSS selector to wait for.
    #[serde(default)]
    pub selector: Option<String>,
    /// Visible text to wait for when no selector is supplied.
    #[serde(default)]
    pub text: Option<String>,
    /// Maximum wait in milliseconds (default 8000, max 30000).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BrowserActionIntent {
    /// Inspection, navigation, test controls, and other reversible actions.
    #[default]
    ReadOnly,
    /// A form submission or action that changes remote state.
    StateChanging,
    /// Login, permission, publishing, deleting, purchasing, or other sensitive action.
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

/// Conservatively classify browser calls that need a user/guardian review.
/// The declaration is only a hint: risky labels/selectors always win.
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
        "login",
        "sign in",
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
    let mutating = [
        "submit", "save", "send", "confirm", "create", "update", "apply", "提交", "保存", "发送",
        "确认", "创建", "更新",
    ]
    .iter()
    .any(|needle| haystack.contains(needle));
    if declared == "state_changing" || mutating {
        Some(BrowserApprovalClass::StateChanging)
    } else {
        None
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BrowserApprovalRule {
    pub origin: String,
    pub action_class: String,
}

fn approvals_path(memory_dir: &Path) -> PathBuf {
    memory_dir.join("browser-approvals.json")
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

pub async fn current_origin(session_id: &str) -> Option<String> {
    let mut manager = manager().lock().await;
    let session = manager.sessions.get_mut(session_id)?;
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
    names: ["browser_open", "browser_snapshot", "browser_click", "browser_type", "browser_scroll", "browser_wait", "browser_screenshot", "browser_close"],
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
        "browser_close" => close(ctx).await,
        _ => anyhow::bail!("unsupported browser tool: {name}"),
    }
}

#[derive(Default)]
struct BrowserManager {
    sessions: HashMap<String, BrowserSession>,
}

struct BrowserSession {
    child: Child,
    socket: DevtoolsSocket,
    next_id: u64,
    output_dir: PathBuf,
    allow_loopback: bool,
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

fn browser_available() -> bool {
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

fn session_dir(ctx: &ToolContext<'_>) -> PathBuf {
    let safe = ctx
        .session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    ctx.memory_dir.join("browser").join(safe)
}

fn validate_url(raw: &str) -> anyhow::Result<String> {
    let trimmed = raw.trim();
    let parsed = reqwest::Url::parse(trimmed).map_err(|e| anyhow::anyhow!("invalid URL: {e}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        anyhow::bail!("browser only supports http(s) URLs");
    }
    let host = parsed
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("URL is missing a host"))?;
    let loopback = host.eq_ignore_ascii_case("localhost")
        || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
    if !loopback {
        crate::engine::network::assert_public_http_url(trimmed)?;
    }
    Ok(parsed.to_string())
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

async fn open(ctx: &ToolContext<'_>, args: BrowserOpenArgs) -> anyhow::Result<String> {
    let url = validate_url(&args.url)?;
    let output_dir = session_dir(ctx);
    tokio::fs::create_dir_all(&output_dir).await?;

    let mut manager = manager().lock().await;
    if let Some(mut old) = manager.sessions.remove(&ctx.session_id) {
        let _ = old.child.kill().await;
    }
    let mut session = BrowserSession::launch(&url, output_dir).await?;
    session.wait_ready(args.wait_ms).await?;
    let result = session.snapshot(true).await?;
    manager.sessions.insert(ctx.session_id.clone(), session);
    Ok(result.to_string())
}

async fn snapshot(ctx: &ToolContext<'_>, args: BrowserSnapshotArgs) -> anyhow::Result<String> {
    let mut manager = manager().lock().await;
    let session = manager.sessions.get_mut(&ctx.session_id).ok_or_else(|| {
        anyhow::anyhow!("browser session is disconnected; call browser_open to restore it")
    })?;
    session.wait_ready(args.wait_ms).await?;
    Ok(session
        .snapshot(args.screenshot.unwrap_or(true))
        .await?
        .to_string())
}

async fn click(ctx: &ToolContext<'_>, args: BrowserClickArgs) -> anyhow::Result<String> {
    if args.selector.as_deref().is_none_or(str::is_empty)
        && args.text.as_deref().is_none_or(str::is_empty)
    {
        anyhow::bail!("browser_click requires selector or text");
    }
    let mut manager = manager().lock().await;
    let session = manager.sessions.get_mut(&ctx.session_id).ok_or_else(|| {
        anyhow::anyhow!("browser session is disconnected; call browser_open to restore it")
    })?;
    let selector = serde_json::to_string(&args.selector)?;
    let text = serde_json::to_string(&args.text)?;
    let expression = format!(
        r#"(() => {{
          const selector = {selector}; const wanted = {text};
          let el = selector ? document.querySelector(selector) : null;
          if (!el && wanted) {{
            el = [...document.querySelectorAll('button,a,input,[role="button"],[role="link"],summary')]
              .find(node => ((node.innerText || node.value || node.getAttribute('aria-label') || '').trim() === wanted));
          }}
          if (!el) return JSON.stringify({{ok:false,error:'element_not_found'}});
          el.scrollIntoView({{block:'center',inline:'center'}}); el.click();
          return JSON.stringify({{ok:true,tag:el.tagName.toLowerCase(),text:(el.innerText || el.value || el.getAttribute('aria-label') || '').trim().slice(0,160)}});
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
    Ok(result.to_string())
}

async fn type_text(ctx: &ToolContext<'_>, args: BrowserTypeArgs) -> anyhow::Result<String> {
    if args.text.len() > 10_000 {
        anyhow::bail!("browser_type text exceeds 10000 characters");
    }
    let mut manager = manager().lock().await;
    let session = manager.sessions.get_mut(&ctx.session_id).ok_or_else(|| {
        anyhow::anyhow!("browser session is disconnected; call browser_open to restore it")
    })?;
    let selector = serde_json::to_string(&args.selector)?;
    let text = serde_json::to_string(&args.text)?;
    let expression = format!(
        r#"(() => {{
          const el = document.querySelector({selector}); const value = {text};
          if (!el) return JSON.stringify({{ok:false,error:'element_not_found'}});
          const type = (el.getAttribute('type') || '').toLowerCase();
          const autocomplete = (el.getAttribute('autocomplete') || '').toLowerCase();
          if (type === 'password' || ['one-time-code','cc-number','cc-csc'].includes(autocomplete))
            return JSON.stringify({{ok:false,error:'sensitive_input_blocked'}});
          el.focus();
          if (el.isContentEditable) {{
            el.textContent = value;
          }} else {{
            const proto = el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype;
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
    Ok(result.to_string())
}

async fn scroll(ctx: &ToolContext<'_>, args: BrowserScrollArgs) -> anyhow::Result<String> {
    let mut manager = manager().lock().await;
    let session = manager.sessions.get_mut(&ctx.session_id).ok_or_else(|| {
        anyhow::anyhow!("browser session is disconnected; call browser_open to restore it")
    })?;
    let x = args.x.unwrap_or(0).clamp(-10_000, 10_000);
    let y = args.y.unwrap_or(600).clamp(-10_000, 10_000);
    session
        .evaluate(&format!(
            "window.scrollBy({{left:{x},top:{y},behavior:'auto'}}); true"
        ))
        .await?;
    let mut result = session.snapshot(true).await?;
    result["action"] = json!({"kind":"scroll","x":x,"y":y});
    Ok(result.to_string())
}

async fn wait_for(ctx: &ToolContext<'_>, args: BrowserWaitArgs) -> anyhow::Result<String> {
    if args.selector.as_deref().is_none_or(str::is_empty)
        && args.text.as_deref().is_none_or(str::is_empty)
    {
        anyhow::bail!("browser_wait requires selector or text");
    }
    let mut manager = manager().lock().await;
    let session = manager.sessions.get_mut(&ctx.session_id).ok_or_else(|| {
        anyhow::anyhow!("browser session is disconnected; call browser_open to restore it")
    })?;
    let selector = serde_json::to_string(&args.selector)?;
    let text = serde_json::to_string(&args.text)?;
    let expression = format!(
        r#"(() => {{ const selector = {selector}; const wanted = {text};
          if (selector) return Boolean(document.querySelector(selector));
          return [...document.querySelectorAll('body *')].some(el => (el.innerText || '').includes(wanted));
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
            return Ok(result.to_string());
        }
        if tokio::time::Instant::now() >= deadline {
            anyhow::bail!("browser_wait timed out");
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn close(ctx: &ToolContext<'_>) -> anyhow::Result<String> {
    let mut manager = manager().lock().await;
    if let Some(mut session) = manager.sessions.remove(&ctx.session_id) {
        let _ = session.child.kill().await;
    }
    Ok(json!({
        "astro_browser": true,
        "session_id": ctx.session_id,
        "status": "closed"
    })
    .to_string())
}

impl BrowserSession {
    async fn launch(url: &str, output_dir: PathBuf) -> anyhow::Result<Self> {
        let executable = find_browser_executable().ok_or_else(|| {
            anyhow::anyhow!(
                "no Chromium browser found; install Chrome, Chromium, Edge, Brave, or set ASTRO_BROWSER_EXECUTABLE"
            )
        })?;
        let profile_dir = output_dir.join("profile");
        tokio::fs::create_dir_all(&profile_dir).await?;
        let mut child = Command::new(executable)
            .arg("--headless=new")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile_dir.display()))
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-background-networking")
            .arg("--disable-component-update")
            .arg("--disable-sync")
            .arg("--metrics-recording-only")
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
            allow_loopback: is_loopback_url(url),
        };
        session.command("Page.enable", json!({})).await?;
        session.command("Runtime.enable", json!({})).await?;
        session.command("DOM.enable", json!({})).await?;
        session
            .command(
                "Fetch.enable",
                json!({"patterns":[{"urlPattern":"*","requestStage":"Request"}]}),
            )
            .await?;
        session.command("Page.navigate", json!({"url":url})).await?;
        Ok(session)
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
        let Some(host) = url.host_str() else {
            return false;
        };
        let loopback = host.eq_ignore_ascii_case("localhost")
            || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback());
        if loopback {
            self.allow_loopback
        } else {
            crate::engine::network::assert_public_http_url(raw).is_ok()
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
              type: el.getAttribute('type'), text: (el.innerText || el.value || el.getAttribute('aria-label') || el.getAttribute('placeholder') || '').trim().slice(0, 180),
              disabled: Boolean(el.disabled || el.getAttribute('aria-disabled') === 'true')
            }));
          return JSON.stringify({url:location.href,title:document.title,text:(document.body?.innerText || '').slice(0,20000),interactive:nodes});
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
    }

    #[test]
    fn rejects_non_http_and_private_lan_urls() {
        assert!(validate_url("file:///tmp/a.html").is_err());
        assert!(validate_url("http://192.168.1.2").is_err());
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
            None
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
                let body = "<!doctype html><title>Astro Browser Test</title><button id='hello'>Hello</button>";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(), body
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        let temp = tempfile::tempdir().unwrap();
        let mut session =
            BrowserSession::launch(&format!("http://{addr}"), temp.path().to_path_buf())
                .await
                .unwrap();
        session.wait_ready(Some(8_000)).await.unwrap();
        let snapshot = session.snapshot(true).await.unwrap();
        assert_eq!(snapshot["title"], "Astro Browser Test");
        assert!(snapshot["screenshot_path"]
            .as_str()
            .is_some_and(|path| Path::new(path).exists()));
    }
}
