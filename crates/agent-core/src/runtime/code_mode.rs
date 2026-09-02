//! Codex-compatible Code Mode cell runtime.
//!
//! Each cell runs in a fresh embedded QuickJS runtime on a dedicated thread.
//! The model script receives no host globals; its only privileged boundary is
//! the tool proxy implemented below. Actual nested tool calls are deliberately
//! executed by `streaming::tools_exec`, so they retain Astro's normal
//! approvals, hooks, sandbox, cancellation, and accounting.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use rquickjs::prelude::{Async, Func};
use rquickjs::{AsyncContext, AsyncRuntime, CatchResultExt, Promise, Value};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot, Mutex};

const DEFAULT_YIELD_TIME_MS: u64 = 10_000;
const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;
const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;
const QUICKJS_MEMORY_LIMIT_BYTES: usize = 64 * 1024 * 1024;
const QUICKJS_STACK_LIMIT_BYTES: usize = 1024 * 1024;
const EVENT_BUFFER_CAPACITY: usize = 256;
const EXIT_SENTINEL: &str = "__ASTRO_CODE_MODE_EXIT__";

const QUICKJS_BOOTSTRAP: &str = r#"
(() => {
const printable = (value) => {
  if (typeof value === 'string') return value;
  if (value === undefined) return 'undefined';
  try {
    const encoded = JSON.stringify(value);
    return encoded === undefined ? String(value) : encoded;
  } catch (_) {
    return String(value);
  }
};
const encode = (value) => {
  const encoded = JSON.stringify(value);
  return encoded === undefined ? JSON.stringify(String(value)) : encoded;
};
const deepFreeze = (value) => {
  if (value && typeof value === 'object' && !Object.isFrozen(value)) {
    Object.freeze(value);
    for (const child of Object.values(value)) deepFreeze(child);
  }
  return value;
};
const decodeResponse = (encoded) => {
  const response = JSON.parse(encoded);
  if (!response.ok) throw new Error(response.error);
  return response.value;
};

const hostInvoke = globalThis.__astroInvoke;
const hostEmit = globalThis.__astroEmit;
const hostStore = globalThis.__astroStore;
const hostYield = globalThis.__astroYield;
const hostSleep = globalThis.__astroSleep;
const stores = new Map(Object.entries(JSON.parse(globalThis.__astroInitialStoresJson)));
const tools = Object.create(null);
const toolMetadata = JSON.parse(globalThis.__astroToolMetadataJson);
const toolSchemas = new Map();
for (const tool of toolMetadata) {
  tools[tool.name] = async (input) => decodeResponse(
    await hostInvoke(
      tool.wire_name,
      JSON.stringify(input === undefined ? null : input),
    ),
  );
  const definition = tool.format
    ? { type: 'custom', name: tool.name, description: tool.description, format: tool.format }
    : { type: 'function', name: tool.name, description: tool.description, parameters: tool.parameters };
  toolSchemas.set(tool.name, deepFreeze(definition));
}

let nextTimerId = 1;
const timers = new Map();
globalThis.setTimeout = (callback, delay = 0, ...args) => {
  const id = nextTimerId++;
  const timer = { active: true };
  timers.set(id, timer);
  hostSleep(Number(delay)).then(() => {
    if (!timer.active) return;
    timers.delete(id);
    callback(...args);
  });
  return id;
};
globalThis.clearTimeout = (id) => {
  const timer = timers.get(id);
  if (timer) {
    timer.active = false;
    timers.delete(id);
  }
};

globalThis.tools = Object.freeze(tools);
globalThis.ALL_TOOLS = Object.freeze(
  toolMetadata.map(({ name, description }) => Object.freeze({ name, description })),
);
globalThis.getToolSchema = (name) => toolSchemas.get(String(name));
globalThis.text = (value) => hostEmit('text', encode(printable(value)), null);
globalThis.image = (value, detail) => hostEmit(
  'image',
  encode(value),
  detail === undefined || detail === null ? null : String(detail),
);
globalThis.audio = (value) => hostEmit('audio', encode(value), null);
globalThis.generatedImage = (value) => hostEmit('generated_image', encode(value), null);
globalThis.notify = (value) => hostEmit('text', encode(printable(value)), null);
globalThis.store = (key, value) => {
  const encoded = JSON.stringify(value);
  const serialized = JSON.parse(encoded);
  stores.set(String(key), serialized);
  hostStore(String(key), encoded);
};
globalThis.load = (key) => stores.get(String(key));
globalThis.yield_control = () => hostYield();
globalThis.exit = () => { throw new Error('__ASTRO_CODE_MODE_EXIT__'); };

delete globalThis.__astroInitialStoresJson;
delete globalThis.__astroToolMetadataJson;
delete globalThis.__astroInvoke;
delete globalThis.__astroEmit;
delete globalThis.__astroStore;
delete globalThis.__astroYield;
delete globalThis.__astroSleep;
globalThis.eval = undefined;
globalThis.Function = undefined;
})();
"#;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NestedToolMetadata {
    pub(crate) name: String,
    pub(crate) wire_name: String,
    pub(crate) description: String,
    pub(crate) parameters: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) format: Option<types::FreeformToolFormat>,
}

#[derive(Debug, Clone)]
pub(crate) struct ExecSource {
    pub(crate) code: String,
    pub(crate) yield_time_ms: u64,
    pub(crate) max_output_tokens: usize,
}

#[derive(Debug, Clone)]
pub(crate) enum RuntimeEvent {
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Content {
        kind: String,
        value: serde_json::Value,
        detail: Option<String>,
    },
    Store {
        key: String,
        value: serde_json::Value,
    },
    Yield,
    Result {
        error: Option<String>,
    },
}

#[derive(Debug)]
pub(crate) enum NextEvent {
    Event(RuntimeEvent),
    TimedOut,
    Closed(String),
}

#[derive(Default)]
struct CellControl {
    pending_tools: Mutex<HashMap<String, oneshot::Sender<Result<serde_json::Value, String>>>>,
    resume: Mutex<Option<oneshot::Sender<()>>>,
    cancelled: AtomicBool,
    cancel_notify: tokio::sync::Notify,
}

impl CellControl {
    fn signal_cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
        self.cancel_notify.notify_waiters();
    }

    async fn cancel(&self) {
        self.signal_cancel();
        if let Some(resume) = self.resume.lock().await.take() {
            let _ = resume.send(());
        }
        let mut pending = self.pending_tools.lock().await;
        for (_, sender) in pending.drain() {
            let _ = sender.send(Err("Code Mode cell was terminated".into()));
        }
    }
}

struct CodeCell {
    events: Mutex<mpsc::Receiver<RuntimeEvent>>,
    control: Arc<CellControl>,
}

#[derive(Default)]
pub(crate) struct CodeModeService {
    cells: Mutex<HashMap<String, Arc<CodeCell>>>,
    stores: Mutex<HashMap<String, serde_json::Value>>,
}

fn json_error(message: impl Into<String>) -> rquickjs::Error {
    rquickjs::Error::new_from_js_message("Astro host", "JavaScript", message.into())
}

fn parse_json(encoded: &str, label: &str) -> rquickjs::Result<serde_json::Value> {
    serde_json::from_str(encoded)
        .map_err(|error| json_error(format!("invalid {label} JSON: {error}")))
}

async fn run_quickjs_cell(
    source: String,
    tools: Vec<NestedToolMetadata>,
    stores: HashMap<String, serde_json::Value>,
    events: mpsc::Sender<RuntimeEvent>,
    control: Arc<CellControl>,
) -> Result<(), String> {
    let runtime = AsyncRuntime::new().map_err(|error| error.to_string())?;
    runtime.set_memory_limit(QUICKJS_MEMORY_LIMIT_BYTES).await;
    runtime.set_max_stack_size(QUICKJS_STACK_LIMIT_BYTES).await;
    let cancelled = Arc::clone(&control);
    runtime
        .set_interrupt_handler(Some(Box::new(move || {
            cancelled.cancelled.load(Ordering::Acquire)
        })))
        .await;
    let context = AsyncContext::full(&runtime)
        .await
        .map_err(|error| error.to_string())?;

    context
        .async_with(async |ctx| {
            let globals = ctx.globals();
            globals.set(
                "__astroInitialStoresJson",
                serde_json::to_string(&stores).map_err(|error| json_error(error.to_string()))?,
            )?;
            globals.set(
                "__astroToolMetadataJson",
                serde_json::to_string(&tools).map_err(|error| json_error(error.to_string()))?,
            )?;

            let invoke_events = events.clone();
            let invoke_control = Arc::clone(&control);
            globals.set(
                "__astroInvoke",
                Func::from(Async(move |name: String, encoded: String| {
                    let events = invoke_events.clone();
                    let control = Arc::clone(&invoke_control);
                    async move {
                        let input = match serde_json::from_str(&encoded) {
                            Ok(input) => input,
                            Err(error) => {
                                return serde_json::json!({
                                    "ok": false,
                                    "error": format!("invalid tool input JSON: {error}"),
                                })
                                .to_string();
                            }
                        };
                        if control.cancelled.load(Ordering::Acquire) {
                            return serde_json::json!({
                                "ok": false,
                                "error": "Code Mode cell was terminated",
                            })
                            .to_string();
                        }
                        let id = uuid::Uuid::new_v4().to_string();
                        let (sender, receiver) = oneshot::channel();
                        control
                            .pending_tools
                            .lock()
                            .await
                            .insert(id.clone(), sender);
                        if events
                            .send(RuntimeEvent::ToolCall {
                                id: id.clone(),
                                name,
                                input,
                            })
                            .await
                            .is_err()
                        {
                            control.pending_tools.lock().await.remove(&id);
                            return serde_json::json!({
                                "ok": false,
                                "error": "Code Mode event receiver closed",
                            })
                            .to_string();
                        }
                        let cancelled = control.cancel_notify.notified();
                        tokio::pin!(cancelled);
                        let result = if control.cancelled.load(Ordering::Acquire) {
                            Err("Code Mode cell was terminated".into())
                        } else {
                            tokio::select! {
                                result = receiver => result.unwrap_or_else(|_| {
                                    Err("Code Mode tool response channel closed".into())
                                }),
                                _ = &mut cancelled => Err("Code Mode cell was terminated".into()),
                            }
                        };
                        control.pending_tools.lock().await.remove(&id);
                        match result {
                            Ok(value) => {
                                serde_json::json!({"ok": true, "value": value}).to_string()
                            }
                            Err(error) => {
                                serde_json::json!({"ok": false, "error": error}).to_string()
                            }
                        }
                    }
                })),
            )?;

            let emit_events = events.clone();
            globals.set(
                "__astroEmit",
                Func::from(
                    move |kind: String, encoded: String, detail: Option<String>| {
                        let value = parse_json(&encoded, "content")?;
                        emit_events
                            .try_send(RuntimeEvent::Content {
                                kind,
                                value,
                                detail,
                            })
                            .map_err(|error| {
                                json_error(format!("Code Mode event delivery failed: {error}"))
                            })
                    },
                ),
            )?;

            let store_events = events.clone();
            globals.set(
                "__astroStore",
                Func::from(move |key: String, encoded: String| {
                    let value = parse_json(&encoded, "store value")?;
                    store_events
                        .try_send(RuntimeEvent::Store { key, value })
                        .map_err(|error| {
                            json_error(format!("Code Mode event delivery failed: {error}"))
                        })
                }),
            )?;

            let yield_events = events.clone();
            let yield_control = Arc::clone(&control);
            globals.set(
                "__astroYield",
                Func::from(Async(move || {
                    let events = yield_events.clone();
                    let control = Arc::clone(&yield_control);
                    async move {
                        if control.cancelled.load(Ordering::Acquire) {
                            return;
                        }
                        let (sender, receiver) = oneshot::channel();
                        *control.resume.lock().await = Some(sender);
                        if events.send(RuntimeEvent::Yield).await.is_err() {
                            control.resume.lock().await.take();
                            return;
                        }
                        let cancelled = control.cancel_notify.notified();
                        tokio::pin!(cancelled);
                        if !control.cancelled.load(Ordering::Acquire) {
                            tokio::select! {
                                _ = receiver => {}
                                _ = &mut cancelled => {}
                            }
                        }
                    }
                })),
            )?;

            let sleep_control = Arc::clone(&control);
            globals.set(
                "__astroSleep",
                Func::from(Async(move |delay_ms: f64| {
                    let control = Arc::clone(&sleep_control);
                    async move {
                        let delay_ms = if delay_ms.is_finite() && delay_ms > 0.0 {
                            delay_ms.min(MAX_SAFE_INTEGER as f64) as u64
                        } else {
                            0
                        };
                        let cancelled = control.cancel_notify.notified();
                        tokio::pin!(cancelled);
                        if !control.cancelled.load(Ordering::Acquire) {
                            tokio::select! {
                                _ = tokio::time::sleep(Duration::from_millis(delay_ms)) => {}
                                _ = &mut cancelled => {}
                            }
                        }
                    }
                })),
            )?;

            ctx.eval::<(), _>(QUICKJS_BOOTSTRAP)
                .catch(&ctx)
                .map_err(|error| json_error(error.to_string()))?;
            let wrapped = format!("(async () => {{\n{source}\n}})()");
            let promise: Promise = ctx
                .eval(wrapped)
                .catch(&ctx)
                .map_err(|error| json_error(error.to_string()))?;
            promise
                .into_future::<Value>()
                .await
                .catch(&ctx)
                .map_err(|error| json_error(error.to_string()))?;
            Ok::<(), rquickjs::Error>(())
        })
        .await
        .map_err(|error| error.to_string())
}

fn spawn_quickjs_cell(
    source: String,
    tools: Vec<NestedToolMetadata>,
    stores: HashMap<String, serde_json::Value>,
    events: mpsc::Sender<RuntimeEvent>,
    control: Arc<CellControl>,
) -> anyhow::Result<()> {
    std::thread::Builder::new()
        .name("astro-code-mode-quickjs".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_time()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    let _ = events.blocking_send(RuntimeEvent::Result {
                        error: Some(format!("failed to start Code Mode runtime: {error}")),
                    });
                    return;
                }
            };
            let result = runtime.block_on(run_quickjs_cell(
                source,
                tools,
                stores,
                events.clone(),
                control,
            ));
            let error = result.err().filter(|error| !error.contains(EXIT_SENTINEL));
            let _ = runtime.block_on(events.send(RuntimeEvent::Result { error }));
        })
        .map(|_| ())
        .map_err(|error| anyhow::anyhow!("failed to spawn Code Mode runtime thread: {error}"))
}

impl CodeModeService {
    pub(crate) async fn execute(
        &self,
        source: &ExecSource,
        tools: &[NestedToolMetadata],
        _execution_root: &std::path::Path,
    ) -> anyhow::Result<String> {
        let cell_id = uuid::Uuid::new_v4().to_string();
        let stores = self.stores.lock().await.clone();
        let (event_sender, event_receiver) = mpsc::channel(EVENT_BUFFER_CAPACITY);
        let control = Arc::new(CellControl::default());
        spawn_quickjs_cell(
            source.code.clone(),
            tools.to_vec(),
            stores,
            event_sender,
            Arc::clone(&control),
        )?;
        self.cells.lock().await.insert(
            cell_id.clone(),
            Arc::new(CodeCell {
                events: Mutex::new(event_receiver),
                control,
            }),
        );
        Ok(cell_id)
    }

    pub(crate) async fn next_event(
        &self,
        cell_id: &str,
        timeout: Duration,
    ) -> anyhow::Result<NextEvent> {
        let cell = self
            .cells
            .lock()
            .await
            .get(cell_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Code Mode cell not found: {cell_id}"))?;
        let mut events = cell.events.lock().await;
        match tokio::time::timeout(timeout, events.recv()).await {
            Err(_) => Ok(NextEvent::TimedOut),
            Ok(Some(event)) => Ok(NextEvent::Event(event)),
            Ok(None) => Ok(NextEvent::Closed(
                "Code Mode runtime exited unexpectedly".into(),
            )),
        }
    }

    pub(crate) async fn send_tool_result(
        &self,
        cell_id: &str,
        id: &str,
        result: Result<serde_json::Value, String>,
    ) -> anyhow::Result<()> {
        let cell = self
            .cells
            .lock()
            .await
            .get(cell_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Code Mode cell not found: {cell_id}"))?;
        let sender = cell.control.pending_tools.lock().await.remove(id);
        let sender = sender.ok_or_else(|| {
            anyhow::anyhow!("Code Mode tool call not found in cell {cell_id}: {id}")
        })?;
        sender
            .send(result)
            .map_err(|_| anyhow::anyhow!("Code Mode cell closed before receiving tool result"))
    }

    pub(crate) async fn resume(&self, cell_id: &str) -> anyhow::Result<()> {
        let cell = self
            .cells
            .lock()
            .await
            .get(cell_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Code Mode cell not found: {cell_id}"))?;
        if let Some(resume) = cell.control.resume.lock().await.take() {
            let _ = resume.send(());
        }
        Ok(())
    }

    pub(crate) async fn update_store(&self, key: String, value: serde_json::Value) {
        self.stores.lock().await.insert(key, value);
    }

    pub(crate) async fn terminate(&self, cell_id: &str) -> anyhow::Result<bool> {
        let cell = self.cells.lock().await.remove(cell_id);
        let Some(cell) = cell else { return Ok(false) };
        cell.control.cancel().await;
        Ok(true)
    }

    pub(crate) async fn close(&self, cell_id: &str) {
        let cell = self.cells.lock().await.remove(cell_id);
        if let Some(cell) = cell {
            cell.control.cancel().await;
        }
    }
}

impl Drop for CodeModeService {
    fn drop(&mut self) {
        for cell in self.cells.get_mut().values() {
            cell.control.signal_cancel();
        }
    }
}

pub(crate) fn parse_exec_source(input: &str) -> Result<ExecSource, String> {
    if input.trim().is_empty() {
        return Err("exec expects raw JavaScript source text (non-empty)".into());
    }
    let mut code = input.to_string();
    let mut yield_time_ms = DEFAULT_YIELD_TIME_MS;
    let mut max_output_tokens = DEFAULT_MAX_OUTPUT_TOKENS;
    let first = input.lines().next().unwrap_or_default().trim_start();
    if let Some(raw) = first.strip_prefix("// @exec:") {
        let (_, rest) = input
            .split_once('\n')
            .ok_or_else(|| "exec pragma must be followed by JavaScript source".to_string())?;
        if rest.trim().is_empty() {
            return Err("exec pragma must be followed by JavaScript source".into());
        }
        let value: serde_json::Value = serde_json::from_str(raw.trim())
            .map_err(|error| format!("exec pragma must be valid JSON: {error}"))?;
        let object = value
            .as_object()
            .ok_or_else(|| "exec pragma must be a JSON object".to_string())?;
        for key in object.keys() {
            if !matches!(key.as_str(), "yield_time_ms" | "max_output_tokens") {
                return Err(format!("exec pragma does not support `{key}`"));
            }
        }
        if let Some(value) = object.get("yield_time_ms") {
            yield_time_ms = value
                .as_u64()
                .filter(|value| *value <= MAX_SAFE_INTEGER)
                .ok_or_else(|| "yield_time_ms must be a non-negative safe integer".to_string())?;
        }
        if let Some(value) = object.get("max_output_tokens") {
            let parsed = value
                .as_u64()
                .filter(|value| *value <= MAX_SAFE_INTEGER)
                .ok_or_else(|| {
                    "max_output_tokens must be a non-negative safe integer".to_string()
                })?;
            max_output_tokens = usize::try_from(parsed)
                .map_err(|_| "max_output_tokens is too large".to_string())?;
        }
        code = rest.to_string();
    }
    Ok(ExecSource {
        code,
        yield_time_ms,
        max_output_tokens,
    })
}

pub(crate) fn normalize_identifier(name: &str) -> String {
    let mut output = String::new();
    for (index, ch) in name.chars().enumerate() {
        let valid = if index == 0 {
            ch == '_' || ch == '$' || ch.is_ascii_alphabetic()
        } else {
            ch == '_' || ch == '$' || ch.is_ascii_alphanumeric()
        };
        output.push(if valid { ch } else { '_' });
    }
    if output.is_empty() {
        "_".into()
    } else {
        output
    }
}

pub(crate) fn truncate_output(text: String, max_tokens: usize) -> String {
    let max_chars = max_tokens.saturating_mul(4);
    if text.chars().count() <= max_chars {
        return text;
    }
    let truncated: String = text.chars().take(max_chars).collect();
    format!("{truncated}\n[output truncated to {max_tokens} tokens]")
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn next(service: &CodeModeService, cell_id: &str) -> RuntimeEvent {
        match service
            .next_event(cell_id, Duration::from_secs(2))
            .await
            .unwrap()
        {
            NextEvent::Event(event) => event,
            other => panic!("expected runtime event, got {other:?}"),
        }
    }

    #[test]
    fn parses_exec_pragma_and_rejects_unknown_fields() {
        let parsed = parse_exec_source(
            "// @exec: {\"yield_time_ms\":25,\"max_output_tokens\":7}\ntext('ok')",
        )
        .unwrap();
        assert_eq!(parsed.code, "text('ok')");
        assert_eq!(parsed.yield_time_ms, 25);
        assert_eq!(parsed.max_output_tokens, 7);
        assert!(parse_exec_source("// @exec: {\"other\":1}\ntext('x')").is_err());
    }

    #[test]
    fn normalizes_tool_names_like_codex() {
        assert_eq!(
            normalize_identifier("mcp__server__tool"),
            "mcp__server__tool"
        );
        assert_eq!(normalize_identifier("cron.list"), "cron_list");
        assert_eq!(normalize_identifier("bad-name"), "bad_name");
    }

    #[tokio::test]
    async fn quickjs_cell_runs_text_and_nested_tool_calls() {
        let service = CodeModeService::default();
        let source =
            parse_exec_source("const value = await tools.echo({answer: 42}); text(value.answer);")
                .unwrap();
        let cell_id = service
            .execute(
                &source,
                &[NestedToolMetadata {
                    name: "echo".into(),
                    wire_name: "echo".into(),
                    description: "echo input".into(),
                    parameters: serde_json::json!({
                        "type": "object",
                        "properties": {"answer": {"type": "number"}},
                        "required": ["answer"]
                    }),
                    format: None,
                }],
                std::env::current_dir().unwrap().as_path(),
            )
            .await
            .unwrap();
        let RuntimeEvent::ToolCall { id, name, input } = next(&service, &cell_id).await else {
            panic!("expected tool call")
        };
        assert_eq!(name, "echo");
        assert_eq!(input["answer"], 42);
        service
            .send_tool_result(&cell_id, &id, Ok(serde_json::json!({"answer":42})))
            .await
            .unwrap();
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Content { value, .. } if value == "42"
        ));
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Result { error: None }
        ));
    }

    #[tokio::test]
    async fn tool_schema_is_available_on_demand_but_catalog_stays_compact() {
        let service = CodeModeService::default();
        let source = parse_exec_source(
            "text([typeof toolMetadata, typeof toolSchemas].join(','));\
             text(Object.hasOwn(ALL_TOOLS[0], 'parameters'));\
             text(getToolSchema('echo').parameters.properties.answer.type);\
             text(getToolSchema('patch').format.syntax);\
             text(Object.isFrozen(getToolSchema('echo').parameters));\
             text(getToolSchema('missing') === undefined);",
        )
        .unwrap();
        let tools = vec![
            NestedToolMetadata {
                name: "echo".into(),
                wire_name: "echo".into(),
                description: "echo input".into(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {"answer": {"type": "number"}},
                    "required": ["answer"]
                }),
                format: None,
            },
            NestedToolMetadata {
                name: "patch".into(),
                wire_name: "patch".into(),
                description: "apply a patch".into(),
                parameters: serde_json::json!({"type": "object", "properties": {}}),
                format: Some(types::FreeformToolFormat {
                    r#type: "grammar".into(),
                    syntax: "lark".into(),
                    definition: "start: /.+/".into(),
                }),
            },
        ];
        let cwd = std::env::current_dir().unwrap();
        let cell_id = service.execute(&source, &tools, &cwd).await.unwrap();
        for expected in [
            "undefined,undefined",
            "false",
            "number",
            "lark",
            "true",
            "true",
        ] {
            assert!(matches!(
                next(&service, &cell_id).await,
                RuntimeEvent::Content { value, .. } if value == expected
            ));
        }
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Result { error: None }
        ));
    }

    #[tokio::test]
    async fn yielded_cell_resumes_and_session_store_is_shared() {
        let service = CodeModeService::default();
        let source = parse_exec_source(
            "store('answer', 42); text('before'); await yield_control(); text('after');",
        )
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        let cell_id = service.execute(&source, &[], &cwd).await.unwrap();
        let RuntimeEvent::Store { key, value } = next(&service, &cell_id).await else {
            panic!("expected store event")
        };
        service.update_store(key, value).await;
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Content { value, .. } if value == "before"
        ));
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Yield
        ));
        service.resume(&cell_id).await.unwrap();
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Content { value, .. } if value == "after"
        ));
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Result { error: None }
        ));
        service.close(&cell_id).await;

        let load = parse_exec_source("text(load('answer'));").unwrap();
        let load_id = service.execute(&load, &[], &cwd).await.unwrap();
        assert!(matches!(
            next(&service, &load_id).await,
            RuntimeEvent::Content { value, .. } if value == "42"
        ));
    }

    #[tokio::test]
    async fn resume_is_a_noop_when_cell_is_running_without_yielding() {
        let service = CodeModeService::default();
        let source = parse_exec_source(
            "await new Promise(resolve => setTimeout(resolve, 5)); text('still-running');",
        )
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        let cell_id = service.execute(&source, &[], &cwd).await.unwrap();
        service.resume(&cell_id).await.unwrap();
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Content { value, .. } if value == "still-running"
        ));
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Result { error: None }
        ));
    }

    #[tokio::test]
    async fn quickjs_cell_supports_timers_without_node() {
        let service = CodeModeService::default();
        let source = parse_exec_source(
            "await new Promise(resolve => setTimeout(resolve, 5)); text('timer-fired');",
        )
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        let cell_id = service.execute(&source, &[], &cwd).await.unwrap();
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Content { value, .. } if value == "timer-fired"
        ));
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Result { error: None }
        ));
    }

    #[tokio::test]
    async fn quickjs_cell_hides_host_bridge_globals() {
        let service = CodeModeService::default();
        let source = parse_exec_source(
            "text([typeof __astroInvoke, typeof process, typeof require].join(','));",
        )
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        let cell_id = service.execute(&source, &[], &cwd).await.unwrap();
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Content { value, .. } if value == "undefined,undefined,undefined"
        ));
    }

    #[tokio::test]
    async fn terminate_interrupts_a_busy_quickjs_cell() {
        let service = CodeModeService::default();
        let source = parse_exec_source("while (true) {}").unwrap();
        let cwd = std::env::current_dir().unwrap();
        let cell_id = service.execute(&source, &[], &cwd).await.unwrap();
        assert!(service.terminate(&cell_id).await.unwrap());
        assert!(!service.terminate(&cell_id).await.unwrap());
    }
}
