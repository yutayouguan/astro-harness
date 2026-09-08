//! Shell Hooks：`config.toml` 的 `hooks:` 映射，异步执行命令。

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tracing::{debug, warn};

use crate::outcome::HookPayload;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const RESERVED_HOOK_ENV: [&str; 11] = [
    "ASTRO_HOOK_EVENT",
    "ASTRO_HOOK_SESSION",
    "ASTRO_HOOK_DETAIL",
    "ASTRO_HOOK_TURN",
    "ASTRO_HOOK_TOOL",
    "ASTRO_HOOK_MESSAGE",
    "ASTRO_HOOK_PROVIDER",
    "ASTRO_HOOK_MODEL",
    "ASTRO_HOOK_ATTEMPT",
    "ASTRO_HOOK_DURATION_MS",
    "ASTRO_HOOK_STATUS",
];

#[derive(Debug, Clone, Default)]
pub struct ShellHookRunner {
    /// event / hook name → shell command
    commands: HashMap<String, String>,
    timeout: Duration,
    #[cfg(test)]
    scheduled: std::sync::Arc<std::sync::Mutex<Vec<(String, HookPayload)>>>,
}

impl ShellHookRunner {
    pub fn new(commands: HashMap<String, String>) -> Self {
        Self {
            commands,
            timeout: DEFAULT_TIMEOUT,
            #[cfg(test)]
            scheduled: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    pub fn from_map(commands: HashMap<String, String>) -> Self {
        Self::new(commands)
    }

    pub fn is_empty(&self) -> bool {
        self.commands.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn has_event(&self, event: &str) -> bool {
        self.commands.contains_key(event)
    }

    #[cfg(test)]
    pub(crate) fn scheduled(&self) -> Vec<(String, HookPayload)> {
        self.scheduled
            .lock()
            .map(|events| events.clone())
            .unwrap_or_default()
    }

    /// Fire-and-forget：在后台跑命令，不阻塞调用方。
    pub fn fire_async(&self, event: &str, payload: &HookPayload) {
        let Some(cmd) = self.commands.get(event).cloned() else {
            return;
        };
        #[cfg(test)]
        if let Ok(mut scheduled) = self.scheduled.lock() {
            scheduled.push((event.to_string(), payload.clone()));
        }
        let env = env_from_payload(event, payload);
        let event = event.to_owned();
        let timeout = self.timeout;
        tokio::spawn(async move {
            if let Err(err) = run_shell(&cmd, &env, timeout).await {
                warn!(%event, %err, "shell hook failed");
            } else {
                debug!(%event, "shell hook completed");
            }
        });
    }

    /// 同步等待（测试用）。
    pub async fn fire_await(&self, event: &str, payload: &HookPayload) -> anyhow::Result<()> {
        let Some(cmd) = self.commands.get(event) else {
            return Ok(());
        };
        let env = env_from_payload(event, payload);
        run_shell(cmd, &env, self.timeout).await
    }
}

pub(crate) fn env_from_payload(event: &str, payload: &HookPayload) -> Vec<(String, String)> {
    let mut env = vec![
        ("ASTRO_HOOK_EVENT".into(), event.to_owned()),
        ("ASTRO_HOOK_SESSION".into(), payload.session_id.clone()),
        ("ASTRO_HOOK_DETAIL".into(), payload.detail.clone()),
    ];
    if let Some(tid) = payload.turn_id.as_ref().filter(|s| !s.is_empty()) {
        env.push(("ASTRO_HOOK_TURN".into(), tid.clone()));
    }
    if let Some(t) = &payload.tool_name {
        env.push(("ASTRO_HOOK_TOOL".into(), t.clone()));
    }
    let message = payload
        .prompt
        .as_ref()
        .or(payload.last_assistant_message.as_ref());
    if let Some(message) = message {
        env.push(("ASTRO_HOOK_MESSAGE".into(), message.clone()));
    }
    if let Some(provider) = payload.provider.as_ref().filter(|value| !value.is_empty()) {
        env.push(("ASTRO_HOOK_PROVIDER".into(), provider.clone()));
    }
    if !payload.model.is_empty() {
        env.push(("ASTRO_HOOK_MODEL".into(), payload.model.clone()));
    }
    if let Some(attempt) = payload.attempt {
        env.push(("ASTRO_HOOK_ATTEMPT".into(), attempt.to_string()));
    }
    if let Some(duration_ms) = payload.duration_ms {
        env.push(("ASTRO_HOOK_DURATION_MS".into(), duration_ms.to_string()));
    }
    if let Some(status) = payload.status.as_ref().filter(|value| !value.is_empty()) {
        env.push(("ASTRO_HOOK_STATUS".into(), status.clone()));
    }
    env
}

async fn run_shell(cmd: &str, env: &[(String, String)], timeout: Duration) -> anyhow::Result<()> {
    let mut child = Command::new("sh");
    child
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    for key in RESERVED_HOOK_ENV {
        child.env_remove(key);
    }
    for (k, v) in env {
        child.env(k, v);
    }
    let mut child = child.spawn()?;
    match tokio::time::timeout(timeout, child.wait()).await {
        Ok(Ok(status)) if status.success() => Ok(()),
        Ok(Ok(status)) => anyhow::bail!("exit {status}"),
        Ok(Err(err)) => Err(err.into()),
        Err(_) => {
            let _ = child.kill().await;
            anyhow::bail!("timeout after {}s", timeout.as_secs())
        }
    }
}

/// 从已解析的 hooks map 与可选根目录构造（根目录目前仅占位，便于扩展）。
pub fn load_shell_runner(_root: &Path, hooks: HashMap<String, String>) -> ShellHookRunner {
    ShellHookRunner::new(hooks)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;

    use super::*;
    use crate::outcome::HookPayload;

    static ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    struct EnvRestore {
        previous: Vec<(&'static str, Option<OsString>)>,
    }

    impl EnvRestore {
        fn preset(entries: &[(&'static str, &'static str)]) -> Self {
            let previous = entries
                .iter()
                .map(|(key, _)| (*key, std::env::var_os(key)))
                .collect();
            for (key, value) in entries {
                std::env::set_var(key, value);
            }
            Self { previous }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            for (key, value) in self.previous.drain(..) {
                match value {
                    Some(value) => std::env::set_var(key, value),
                    None => std::env::remove_var(key),
                }
            }
        }
    }

    fn env_value<'a>(env: &'a [(String, String)], key: &str) -> Option<&'a str> {
        env.iter()
            .find_map(|(candidate, value)| (candidate == key).then_some(value.as_str()))
    }

    #[test]
    fn legacy_yaml_key_does_not_match_canonical_name() {
        let runner = ShellHookRunner::new(HashMap::from([(
            "post_tool_call".to_string(),
            "true".to_string(),
        )]));

        assert!(!runner.has_event(crate::names::POST_TOOL_USE));
        assert!(runner.has_event("post_tool_call"));
        assert_eq!(runner.commands.len(), 1);
        assert_eq!(
            runner.commands.get("post_tool_call").map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn legacy_and_canonical_keys_remain_distinct() {
        let runner = ShellHookRunner::new(HashMap::from([
            ("post_tool_call".to_string(), "false".to_string()),
            (crate::names::POST_TOOL_USE.to_string(), "true".to_string()),
        ]));

        assert!(runner.has_event(crate::names::POST_TOOL_USE));
        assert_eq!(runner.commands.len(), 2);
        assert_eq!(
            runner
                .commands
                .get(crate::names::POST_TOOL_USE)
                .map(String::as_str),
            Some("true"),
            "canonical dispatch uses only the canonical slot"
        );
    }

    #[test]
    fn legacy_env_is_derived_from_canonical_input() {
        let env = env_from_payload(
            crate::names::PRE_TOOL_USE,
            &HookPayload {
                session_id: "session-1".into(),
                turn_id: Some("turn-1".into()),
                detail: "running terminal".into(),
                prompt: Some("canonical prompt".into()),
                tool_name: Some("exec_command".into()),
                tool_input: Some(serde_json::json!({"message": "do not infer this"})),
                ..Default::default()
            },
        );

        assert_eq!(
            env_value(&env, "ASTRO_HOOK_EVENT"),
            Some(crate::names::PRE_TOOL_USE)
        );
        assert_eq!(env_value(&env, "ASTRO_HOOK_SESSION"), Some("session-1"));
        assert_eq!(env_value(&env, "ASTRO_HOOK_TURN"), Some("turn-1"));
        assert_eq!(
            env_value(&env, "ASTRO_HOOK_DETAIL"),
            Some("running terminal")
        );
        assert_eq!(env_value(&env, "ASTRO_HOOK_TOOL"), Some("exec_command"));
        assert_eq!(
            env_value(&env, "ASTRO_HOOK_MESSAGE"),
            Some("canonical prompt")
        );
    }

    #[test]
    fn telemetry_fields_are_forwarded_to_shell_env() {
        let env = env_from_payload(
            crate::names::POST_LLM_CALL,
            &HookPayload {
                model: "gpt-5.6-sol".into(),
                provider: Some("openai".into()),
                attempt: Some(3),
                duration_ms: Some(987),
                status: Some("succeeded".into()),
                ..Default::default()
            },
        );

        assert_eq!(env_value(&env, "ASTRO_HOOK_PROVIDER"), Some("openai"));
        assert_eq!(env_value(&env, "ASTRO_HOOK_MODEL"), Some("gpt-5.6-sol"));
        assert_eq!(env_value(&env, "ASTRO_HOOK_ATTEMPT"), Some("3"));
        assert_eq!(env_value(&env, "ASTRO_HOOK_DURATION_MS"), Some("987"));
        assert_eq!(env_value(&env, "ASTRO_HOOK_STATUS"), Some("succeeded"));
    }

    #[test]
    fn event_label_is_forwarded_without_rewrite() {
        let env = env_from_payload("pre_tool_call", &HookPayload::default());

        assert_eq!(env_value(&env, "ASTRO_HOOK_EVENT"), Some("pre_tool_call"));
    }

    #[test]
    fn legacy_message_falls_back_to_last_assistant_message() {
        let env = env_from_payload(
            crate::names::STOP,
            &HookPayload {
                last_assistant_message: Some("assistant fallback".into()),
                ..Default::default()
            },
        );

        assert_eq!(
            env_value(&env, "ASTRO_HOOK_MESSAGE"),
            Some("assistant fallback")
        );
    }

    #[test]
    fn legacy_message_prefers_prompt_over_last_assistant_message() {
        let env = env_from_payload(
            crate::names::USER_PROMPT_SUBMIT,
            &HookPayload {
                prompt: Some("user prompt".into()),
                last_assistant_message: Some("assistant message".into()),
                ..Default::default()
            },
        );

        assert_eq!(env_value(&env, "ASTRO_HOOK_MESSAGE"), Some("user prompt"));
    }

    #[test]
    fn legacy_message_does_not_infer_text_from_tool_input() {
        let env = env_from_payload(
            crate::names::PRE_TOOL_USE,
            &HookPayload {
                tool_input: Some(serde_json::json!({"message": "not a prompt"})),
                ..Default::default()
            },
        );

        assert_eq!(env_value(&env, "ASTRO_HOOK_MESSAGE"), None);
    }

    #[test]
    fn env_includes_turn_when_set() {
        let env = env_from_payload(
            crate::names::POST_TOOL_USE,
            &HookPayload {
                session_id: "s1".into(),
                turn_id: Some("turn-abc".into()),
                detail: "d".into(),
                ..Default::default()
            },
        );
        assert!(env
            .iter()
            .any(|(k, v)| k == "ASTRO_HOOK_TURN" && v == "turn-abc"));
        assert!(env
            .iter()
            .any(|(k, v)| k == "ASTRO_HOOK_SESSION" && v == "s1"));
    }

    #[test]
    fn env_omits_turn_when_empty() {
        let env = env_from_payload(
            crate::names::POST_TOOL_USE,
            &HookPayload {
                session_id: "s1".into(),
                turn_id: Some(String::new()),
                detail: "d".into(),
                ..Default::default()
            },
        );
        assert!(!env.iter().any(|(k, _)| k == "ASTRO_HOOK_TURN"));
    }

    #[test]
    fn env_omits_turn_when_none() {
        let env = env_from_payload(
            crate::names::POST_TOOL_USE,
            &HookPayload {
                session_id: "s1".into(),
                turn_id: None,
                detail: "d".into(),
                ..Default::default()
            },
        );
        assert!(!env.iter().any(|(k, _)| k == "ASTRO_HOOK_TURN"));
    }

    #[tokio::test]
    async fn child_does_not_inherit_reserved_hook_env_for_missing_payload_fields() {
        let _lock = ENV_LOCK.lock().await;
        let _restore = EnvRestore::preset(&[
            ("ASTRO_HOOK_EVENT", "parent-event"),
            ("ASTRO_HOOK_SESSION", "parent-session"),
            ("ASTRO_HOOK_DETAIL", "parent-detail"),
            ("ASTRO_HOOK_TURN", "parent-turn"),
            ("ASTRO_HOOK_TOOL", "parent-tool"),
            ("ASTRO_HOOK_MESSAGE", "parent-message"),
            ("ASTRO_HOOK_PROVIDER", "parent-provider"),
            ("ASTRO_HOOK_MODEL", "parent-model"),
            ("ASTRO_HOOK_ATTEMPT", "parent-attempt"),
            ("ASTRO_HOOK_DURATION_MS", "parent-duration"),
            ("ASTRO_HOOK_STATUS", "parent-status"),
            ("HOOK_TEST_PASSTHROUGH", "parent-visible"),
        ]);
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("child-env.txt");
        let mut env = env_from_payload(
            crate::names::POST_TOOL_USE,
            &HookPayload {
                session_id: "child-session".into(),
                detail: "child-detail".into(),
                ..Default::default()
            },
        );
        env.push((
            "HOOK_TEST_OUTPUT".into(),
            output.to_string_lossy().into_owned(),
        ));

        run_shell(
            r#"printf '%s\n' "$ASTRO_HOOK_EVENT" "$ASTRO_HOOK_SESSION" "$ASTRO_HOOK_DETAIL" "${ASTRO_HOOK_TURN-unset}" "${ASTRO_HOOK_TOOL-unset}" "${ASTRO_HOOK_MESSAGE-unset}" "${ASTRO_HOOK_PROVIDER-unset}" "${ASTRO_HOOK_MODEL-unset}" "${ASTRO_HOOK_ATTEMPT-unset}" "${ASTRO_HOOK_DURATION_MS-unset}" "${ASTRO_HOOK_STATUS-unset}" "$HOOK_TEST_PASSTHROUGH" > "$HOOK_TEST_OUTPUT""#,
            &env,
            DEFAULT_TIMEOUT,
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(output).unwrap(),
            "PostToolUse\nchild-session\nchild-detail\nunset\nunset\nunset\nunset\nunset\nunset\nunset\nunset\nparent-visible\n"
        );
    }

    #[tokio::test]
    async fn runs_echo() {
        let mut map = HashMap::new();
        map.insert(crate::names::POST_TOOL_USE.into(), "true".into());
        let runner = ShellHookRunner::new(map);
        runner
            .fire_await(
                crate::names::POST_TOOL_USE,
                &HookPayload {
                    tool_name: Some("echo".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
}
