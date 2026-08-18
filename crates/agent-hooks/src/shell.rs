//! Shell Hooks：`config.yaml` 的 `hooks:` 映射，异步执行命令。

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tracing::{debug, warn};

use crate::outcome::HookPayload;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);
const RESERVED_HOOK_ENV: [&str; 6] = [
    "ASTRO_HOOK_EVENT",
    "ASTRO_HOOK_SESSION",
    "ASTRO_HOOK_DETAIL",
    "ASTRO_HOOK_TURN",
    "ASTRO_HOOK_TOOL",
    "ASTRO_HOOK_MESSAGE",
];

#[derive(Debug, Clone, Default)]
pub struct ShellHookRunner {
    /// event / hook name → shell command
    commands: HashMap<String, String>,
    timeout: Duration,
}

impl ShellHookRunner {
    pub fn new(commands: HashMap<String, String>) -> Self {
        let mut entries = commands
            .into_iter()
            .map(|(event, command)| {
                let canonical = crate::event::normalize_hook_event_name(&event).into_owned();
                let is_canonical = event == canonical;
                (canonical, is_canonical, event, command)
            })
            .collect::<Vec<_>>();
        entries.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| left.1.cmp(&right.1))
                .then_with(|| left.2.cmp(&right.2))
        });
        let commands = entries
            .into_iter()
            .map(|(canonical, _, _, command)| (canonical, command))
            .collect();
        Self {
            commands,
            timeout: DEFAULT_TIMEOUT,
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
        self.commands
            .contains_key(crate::event::canonical_hook_event_name(event).as_ref())
    }

    /// Fire-and-forget：在后台跑命令，不阻塞调用方。
    pub fn fire_async(&self, event: &str, payload: &HookPayload) {
        let event = crate::event::canonical_hook_event_name(event);
        let Some(cmd) = self.commands.get(event.as_ref()).cloned() else {
            return;
        };
        let env = env_from_payload(event.as_ref(), payload);
        let event = event.into_owned();
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
        let event = crate::event::canonical_hook_event_name(event);
        let Some(cmd) = self.commands.get(event.as_ref()) else {
            return Ok(());
        };
        let env = env_from_payload(event.as_ref(), payload);
        run_shell(cmd, &env, self.timeout).await
    }
}

pub(crate) fn env_from_payload(event: &str, payload: &HookPayload) -> Vec<(String, String)> {
    let event = crate::event::canonical_hook_event_name(event);
    let mut env = vec![
        ("ASTRO_HOOK_EVENT".into(), event.into_owned()),
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
    use std::sync::Mutex;

    use super::*;
    use crate::outcome::HookPayload;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

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
    fn legacy_yaml_key_is_stored_under_canonical_name() {
        let runner = ShellHookRunner::new(HashMap::from([(
            "post_tool_call".to_string(),
            "true".to_string(),
        )]));

        assert!(runner.has_event(crate::names::POST_TOOL_USE));
        assert_eq!(runner.commands.len(), 1);
        assert_eq!(
            runner
                .commands
                .get(crate::names::POST_TOOL_USE)
                .map(String::as_str),
            Some("true")
        );
    }

    #[test]
    fn legacy_and_canonical_keys_collapse_to_one_slot() {
        let runner = ShellHookRunner::new(HashMap::from([
            ("post_tool_call".to_string(), "false".to_string()),
            (crate::names::POST_TOOL_USE.to_string(), "true".to_string()),
        ]));

        assert!(runner.has_event(crate::names::POST_TOOL_USE));
        assert_eq!(runner.commands.len(), 1);
        assert_eq!(
            runner
                .commands
                .get(crate::names::POST_TOOL_USE)
                .map(String::as_str),
            Some("true"),
            "the canonical spelling wins deterministically"
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
                tool_name: Some("terminal".into()),
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
        assert_eq!(env_value(&env, "ASTRO_HOOK_TOOL"), Some("terminal"));
        assert_eq!(
            env_value(&env, "ASTRO_HOOK_MESSAGE"),
            Some("canonical prompt")
        );
    }

    #[test]
    fn legacy_event_label_is_canonicalized_on_direct_call() {
        let env = env_from_payload("pre_tool_call", &HookPayload::default());

        assert_eq!(
            env_value(&env, "ASTRO_HOOK_EVENT"),
            Some(crate::names::PRE_TOOL_USE)
        );
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
            "post_tool_call",
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
            "post_tool_call",
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
            "post_tool_call",
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
        let _lock = ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let _restore = EnvRestore::preset(&[
            ("ASTRO_HOOK_EVENT", "parent-event"),
            ("ASTRO_HOOK_SESSION", "parent-session"),
            ("ASTRO_HOOK_DETAIL", "parent-detail"),
            ("ASTRO_HOOK_TURN", "parent-turn"),
            ("ASTRO_HOOK_TOOL", "parent-tool"),
            ("ASTRO_HOOK_MESSAGE", "parent-message"),
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
            r#"printf '%s\n' "$ASTRO_HOOK_EVENT" "$ASTRO_HOOK_SESSION" "$ASTRO_HOOK_DETAIL" "${ASTRO_HOOK_TURN-unset}" "${ASTRO_HOOK_TOOL-unset}" "${ASTRO_HOOK_MESSAGE-unset}" "$HOOK_TEST_PASSTHROUGH" > "$HOOK_TEST_OUTPUT""#,
            &env,
            DEFAULT_TIMEOUT,
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(output).unwrap(),
            "PostToolUse\nchild-session\nchild-detail\nunset\nunset\nunset\nparent-visible\n"
        );
    }

    #[tokio::test]
    async fn runs_echo() {
        let mut map = HashMap::new();
        map.insert("post_tool_call".into(), "true".into());
        let runner = ShellHookRunner::new(map);
        runner
            .fire_await(
                "post_tool_call",
                &HookPayload {
                    tool_name: Some("echo".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
    }
}
