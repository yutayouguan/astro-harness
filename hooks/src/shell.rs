//! Shell Hooks：`config.yaml` 的 `hooks:` 映射，异步执行命令。

use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use tokio::process::Command;
use tracing::{debug, warn};

use crate::outcome::HookPayload;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default)]
pub struct ShellHookRunner {
    /// event / hook name → shell command
    commands: HashMap<String, String>,
    timeout: Duration,
}

impl ShellHookRunner {
    pub fn new(commands: HashMap<String, String>) -> Self {
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

    /// Fire-and-forget：在后台跑命令，不阻塞调用方。
    pub fn fire_async(&self, event: &str, payload: &HookPayload) {
        let Some(cmd) = self.commands.get(event).cloned() else {
            return;
        };
        let event = event.to_string();
        let env = env_from_payload(&event, payload);
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
        ("ASTRO_HOOK_EVENT".into(), event.into()),
        ("ASTRO_HOOK_SESSION".into(), payload.session_id.clone()),
        ("ASTRO_HOOK_DETAIL".into(), payload.detail.clone()),
    ];
    if let Some(tid) = payload.turn_id.as_ref().filter(|s| !s.is_empty()) {
        env.push(("ASTRO_HOOK_TURN".into(), tid.clone()));
    }
    if let Some(t) = &payload.tool_name {
        env.push(("ASTRO_HOOK_TOOL".into(), t.clone()));
    }
    if let Some(m) = &payload.message {
        env.push(("ASTRO_HOOK_MESSAGE".into(), m.clone()));
    }
    env
}

async fn run_shell(
    cmd: &str,
    env: &[(String, String)],
    timeout: Duration,
) -> anyhow::Result<()> {
    let mut child = Command::new("sh");
    child
        .arg("-c")
        .arg(cmd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
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
    use super::*;
    use crate::outcome::HookPayload;

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
        assert!(env.iter().any(|(k, v)| k == "ASTRO_HOOK_TURN" && v == "turn-abc"));
        assert!(env.iter().any(|(k, v)| k == "ASTRO_HOOK_SESSION" && v == "s1"));
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
