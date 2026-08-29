//! Codex-compatible Code Mode cell runtime.
//!
//! Each cell runs in a fresh Node process and a fresh `node:vm` context. The
//! model script receives no Node globals; its only host boundary is the
//! line-delimited tool protocol implemented below. Actual nested tool calls are
//! deliberately executed by `streaming::tools_exec`, so they retain Astro's
//! normal approvals, hooks, sandbox, cancellation, and accounting.

use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

const DEFAULT_YIELD_TIME_MS: u64 = 10_000;
const DEFAULT_MAX_OUTPUT_TOKENS: usize = 10_000;
const MAX_SAFE_INTEGER: u64 = (1_u64 << 53) - 1;

const NODE_HARNESS: &str = r#"
const vm = require('node:vm');
const readline = require('node:readline');
const rl = readline.createInterface({ input: process.stdin, crlfDelay: Infinity });
const pending = new Map();
let nextId = 1;
let resumeYield = null;
const emit = (value) => process.stdout.write(JSON.stringify(value) + '\n');
const printable = (value) => {
  if (typeof value === 'string') return value;
  if (value === undefined) return 'undefined';
  try { return JSON.stringify(value); } catch (_) { return String(value); }
};
const firstLine = new Promise((resolve) => rl.once('line', resolve));
(async () => {
  const init = JSON.parse(await firstLine);
  const stores = new Map(Object.entries(init.stores || {}));
  rl.on('line', (line) => {
    let message;
    try { message = JSON.parse(line); } catch (_) { return; }
    if (message.type === 'tool_result') {
      const waiter = pending.get(message.id);
      if (!waiter) return;
      pending.delete(message.id);
      if (message.ok) waiter.resolve(message.value); else waiter.reject(new Error(message.error));
    } else if (message.type === 'resume' && resumeYield) {
      const resume = resumeYield;
      resumeYield = null;
      resume();
    }
  });
  const invoke = (wireName, input) => new Promise((resolve, reject) => {
    const id = String(nextId++);
    pending.set(id, { resolve, reject });
    emit({ type: 'tool_call', id, name: wireName, input: input === undefined ? null : input });
  });
  const tools = Object.create(null);
  for (const tool of init.tools) tools[tool.name] = (input) => invoke(tool.wire_name, input);
  const sandbox = {
    tools,
    ALL_TOOLS: Object.freeze(init.tools.map(({ name, description }) => Object.freeze({ name, description }))),
    text: (value) => emit({ type: 'content', kind: 'text', value: printable(value) }),
    image: (value, detail) => emit({ type: 'content', kind: 'image', value, detail }),
    audio: (value) => emit({ type: 'content', kind: 'audio', value }),
    generatedImage: (value) => emit({ type: 'content', kind: 'generated_image', value }),
    store: (key, value) => {
      const serialized = JSON.parse(JSON.stringify(value));
      stores.set(String(key), serialized);
      emit({ type: 'store', key: String(key), value: serialized });
    },
    load: (key) => stores.get(String(key)),
    notify: (value) => emit({ type: 'content', kind: 'text', value: printable(value) }),
    exit: () => { throw Object.assign(new Error('__ASTRO_CODE_MODE_EXIT__'), { astroExit: true }); },
    yield_control: () => {
      emit({ type: 'yield' });
      return new Promise((resolve) => { resumeYield = resolve; });
    },
    setTimeout,
    clearTimeout,
  };
  const context = vm.createContext(sandbox, {
    name: 'astro-code-mode',
    codeGeneration: { strings: false, wasm: false },
  });
  try {
    const script = new vm.Script(`(async () => {\n${init.source}\n})()`, {
      filename: 'exec-cell.js',
      displayErrors: true,
    });
    await script.runInContext(context);
    emit({ type: 'result' });
  } catch (error) {
    if (error && error.astroExit) emit({ type: 'result' });
    else emit({ type: 'result', error: error && error.stack ? String(error.stack) : String(error) });
  } finally {
    process.exit(0);
  }
})().catch((error) => {
  emit({ type: 'result', error: error && error.stack ? String(error.stack) : String(error) });
  process.exit(1);
});
"#;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct NestedToolMetadata {
    pub(crate) name: String,
    pub(crate) wire_name: String,
    pub(crate) description: String,
}

#[derive(Debug, Clone)]
pub(crate) struct ExecSource {
    pub(crate) code: String,
    pub(crate) yield_time_ms: u64,
    pub(crate) max_output_tokens: usize,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum RuntimeEvent {
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    Content {
        kind: String,
        value: serde_json::Value,
        #[serde(default)]
        detail: Option<String>,
    },
    Store {
        key: String,
        value: serde_json::Value,
    },
    Yield,
    Result {
        #[serde(default)]
        error: Option<String>,
    },
}

#[derive(Debug)]
pub(crate) enum NextEvent {
    Event(RuntimeEvent),
    TimedOut,
    Closed(String),
}

struct CodeCell {
    child: Child,
    stdin: ChildStdin,
    stdout: Lines<BufReader<ChildStdout>>,
}

#[derive(Default)]
pub(crate) struct CodeModeService {
    cells: Mutex<HashMap<String, Arc<Mutex<CodeCell>>>>,
    stores: Mutex<HashMap<String, serde_json::Value>>,
}

#[derive(Serialize)]
struct InitMessage<'a> {
    source: &'a str,
    tools: &'a [NestedToolMetadata],
    stores: &'a HashMap<String, serde_json::Value>,
}

impl CodeModeService {
    pub(crate) async fn execute(
        &self,
        source: &ExecSource,
        tools: &[NestedToolMetadata],
        execution_root: &std::path::Path,
    ) -> anyhow::Result<String> {
        let node = resolve_node_path()
            .ok_or_else(|| anyhow::anyhow!("Code Mode requires Node.js on PATH"))?;
        let readable_root = node
            .parent()
            .ok_or_else(|| anyhow::anyhow!("invalid Node.js executable path"))?
            .to_path_buf();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::ReadOnly,
            execution_root,
            std::iter::empty(),
            false,
        )?
        .with_restricted_read(vec![readable_root]);
        let mut command =
            sandbox::SandboxRunner.tokio_command(&policy, node.to_string_lossy().as_ref())?;
        command
            .arg("-e")
            .arg(NODE_HARNESS)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true)
            .current_dir(execution_root)
            .env_clear();
        for key in [
            "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TMPDIR", "TMP", "TEMP",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let mut child = command.spawn().map_err(|error| {
            anyhow::anyhow!("failed to start sandboxed Code Mode runtime: {error}")
        })?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Code Mode runtime stdin unavailable"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Code Mode runtime stdout unavailable"))?;
        let stores = self.stores.lock().await.clone();
        let init = serde_json::to_string(&InitMessage {
            source: &source.code,
            tools,
            stores: &stores,
        })?;
        stdin.write_all(init.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        let cell_id = uuid::Uuid::new_v4().to_string();
        self.cells.lock().await.insert(
            cell_id.clone(),
            Arc::new(Mutex::new(CodeCell {
                child,
                stdin,
                stdout: BufReader::new(stdout).lines(),
            })),
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
        let mut cell = cell.lock().await;
        match tokio::time::timeout(timeout, cell.stdout.next_line()).await {
            Err(_) => Ok(NextEvent::TimedOut),
            Ok(Err(error)) => Err(error.into()),
            Ok(Ok(Some(line))) => {
                let event = serde_json::from_str(&line)
                    .map_err(|error| anyhow::anyhow!("invalid Code Mode runtime event: {error}"))?;
                Ok(NextEvent::Event(event))
            }
            Ok(Ok(None)) => {
                let status = cell.child.wait().await?;
                Ok(NextEvent::Closed(format!(
                    "Code Mode runtime exited unexpectedly with {status}"
                )))
            }
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
        let message = match result {
            Ok(value) => serde_json::json!({"type":"tool_result","id":id,"ok":true,"value":value}),
            Err(error) => {
                serde_json::json!({"type":"tool_result","id":id,"ok":false,"error":error})
            }
        };
        let mut cell = cell.lock().await;
        cell.stdin
            .write_all(serde_json::to_string(&message)?.as_bytes())
            .await?;
        cell.stdin.write_all(b"\n").await?;
        cell.stdin.flush().await?;
        Ok(())
    }

    pub(crate) async fn resume(&self, cell_id: &str) -> anyhow::Result<()> {
        self.send_control(cell_id, serde_json::json!({"type":"resume"}))
            .await
    }

    async fn send_control(&self, cell_id: &str, value: serde_json::Value) -> anyhow::Result<()> {
        let cell = self
            .cells
            .lock()
            .await
            .get(cell_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Code Mode cell not found: {cell_id}"))?;
        let mut cell = cell.lock().await;
        cell.stdin
            .write_all(serde_json::to_string(&value)?.as_bytes())
            .await?;
        cell.stdin.write_all(b"\n").await?;
        cell.stdin.flush().await?;
        Ok(())
    }

    pub(crate) async fn update_store(&self, key: String, value: serde_json::Value) {
        self.stores.lock().await.insert(key, value);
    }

    pub(crate) async fn terminate(&self, cell_id: &str) -> anyhow::Result<bool> {
        let cell = self.cells.lock().await.remove(cell_id);
        let Some(cell) = cell else { return Ok(false) };
        let mut cell = cell.lock().await;
        cell.child.kill().await?;
        Ok(true)
    }

    pub(crate) async fn close(&self, cell_id: &str) {
        self.cells.lock().await.remove(cell_id);
    }
}

fn resolve_node_path() -> Option<std::path::PathBuf> {
    let path = std::env::var_os("PATH")?;
    let candidates = std::env::split_paths(&path).flat_map(|directory| {
        #[cfg(windows)]
        let names = ["node.exe", "node.cmd"];
        #[cfg(not(windows))]
        let names = ["node", "node"];
        names.into_iter().map(move |name| directory.join(name))
    });
    candidates
        .filter(|candidate| candidate.is_file())
        .find_map(|candidate| candidate.canonicalize().ok())
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

    fn node_available() -> bool {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

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
    async fn v8_cell_runs_text_and_nested_tool_calls() {
        if !node_available() {
            return;
        }
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
    async fn yielded_cell_resumes_and_session_store_is_shared() {
        if !node_available() {
            return;
        }
        let service = CodeModeService::default();
        let source = parse_exec_source(
            "store('answer', 42); text('before'); await yield_control(); text('after');",
        )
        .unwrap();
        let cwd = std::env::current_dir().unwrap();
        let cell_id = service.execute(&source, &[], &cwd).await.unwrap();
        assert!(matches!(
            next(&service, &cell_id).await,
            RuntimeEvent::Store { .. }
        ));
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
}
