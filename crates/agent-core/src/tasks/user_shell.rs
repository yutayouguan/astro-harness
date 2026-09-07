use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;

use agent_protocol::{
    DeltaEvent, EventMsg, ItemEvent, ToolItem, ToolStatus, TurnItem, TurnStartedEvent,
};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

use crate::runtime::{Session, TurnContext};

use super::{SessionTask, SessionTaskResult, TaskKind, TurnCancelled, TurnInput};

const MAX_SHELL_CAPTURE_BYTES: usize = 1024 * 1024;
const OUTPUT_TRUNCATED_MARKER: &str = "\n[output truncated after 1048576 bytes]\n";

#[derive(Default)]
struct ShellStreamOutput {
    bytes: Vec<u8>,
    truncated: bool,
}

fn accepted_output_bytes(captured: usize, incoming: usize) -> usize {
    incoming.min(MAX_SHELL_CAPTURE_BYTES.saturating_sub(captured))
}

pub(crate) struct UserShellTask {
    pub(crate) command: String,
    pub(crate) cwd: Option<PathBuf>,
    pub(crate) item_id: String,
}

impl SessionTask for UserShellTask {
    fn kind(&self) -> TaskKind {
        TaskKind::UserShell
    }

    fn span_name(&self) -> &'static str {
        "session_task.user_shell"
    }

    async fn run(
        self: Arc<Self>,
        session: Arc<Session>,
        ctx: Arc<TurnContext>,
        _input: Vec<TurnInput>,
        cancellation_token: CancellationToken,
    ) -> SessionTaskResult {
        session
            .send_event(
                ctx.sub_id(),
                EventMsg::TurnStarted(TurnStartedEvent {
                    turn_id: ctx.sub_id().to_string(),
                }),
            )
            .await;
        run_user_shell_process(
            session,
            ctx,
            self.command.clone(),
            self.cwd.clone(),
            self.item_id.clone(),
            cancellation_token,
        )
        .await?;
        Ok(None)
    }
}

pub(crate) async fn run_user_shell_process(
    session: Arc<Session>,
    turn_context: Arc<TurnContext>,
    command: String,
    cwd: Option<PathBuf>,
    item_id: String,
    cancellation_token: CancellationToken,
) -> anyhow::Result<()> {
    let cwd = cwd
        .or_else(|| turn_context.project_root().map(ToOwned::to_owned))
        .unwrap_or_else(|| session.memory().workspace_dir.clone());
    let shell = std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .unwrap_or_else(|| PathBuf::from("/bin/sh"));
    let arguments = serde_json::json!({
        "command": command,
        "cwd": cwd,
        "shell": shell,
        "origin": "user",
        "sandbox": "disabled",
    });
    let running = ToolItem {
        id: item_id.clone(),
        name: "user_shell".into(),
        arguments: arguments.clone(),
        output: None,
        web_action: None,
        web_page_title: None,
        media: Vec::new(),
        file_changes: Vec::new(),
        status: ToolStatus::InProgress,
        batch_id: None,
        execution_mode: None,
    };
    session
        .send_event(
            turn_context.sub_id(),
            EventMsg::ItemStarted(ItemEvent {
                turn_id: turn_context.sub_id().to_string(),
                item: TurnItem::CommandExecution(running),
            }),
        )
        .await;

    if !cwd.is_dir() {
        let message = format!("user shell cwd is not a directory: {}", cwd.display());
        emit_shell_completion(
            &session,
            &turn_context,
            item_id,
            arguments,
            ToolStatus::Failed,
            serde_json::json!({"error": message}),
        )
        .await;
        anyhow::bail!(message);
    }

    let mut child = match tokio::process::Command::new(&shell)
        .arg("-lc")
        .arg(&command)
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            emit_shell_completion(
                &session,
                &turn_context,
                item_id,
                arguments,
                ToolStatus::Failed,
                serde_json::json!({"error": error.to_string()}),
            )
            .await;
            return Err(error.into());
        }
    };
    let stdout = child.stdout.take().expect("user shell stdout is piped");
    let stderr = child.stderr.take().expect("user shell stderr is piped");
    let stdout_task = tokio::spawn(read_shell_stream(
        Arc::clone(&session),
        Arc::clone(&turn_context),
        item_id.clone(),
        stdout,
    ));
    let stderr_task = tokio::spawn(read_shell_stream(
        Arc::clone(&session),
        Arc::clone(&turn_context),
        item_id.clone(),
        stderr,
    ));

    let status = tokio::select! {
        result = child.wait() => match result {
            Ok(status) => status,
            Err(error) => {
                stdout_task.abort();
                stderr_task.abort();
                emit_shell_completion(
                    &session,
                    &turn_context,
                    item_id,
                    arguments,
                    ToolStatus::Failed,
                    serde_json::json!({"error": error.to_string()}),
                ).await;
                return Err(error.into());
            }
        },
        _ = cancellation_token.cancelled() => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            stdout_task.abort();
            stderr_task.abort();
            emit_shell_completion(
                &session,
                &turn_context,
                item_id,
                arguments,
                ToolStatus::Failed,
                serde_json::json!({"cancelled": true}),
            ).await;
            return Err(TurnCancelled.into());
        }
    };
    let stdout = stdout_task.await.unwrap_or_default();
    let stderr = stderr_task.await.unwrap_or_default();
    let code = status.code().unwrap_or(-1);
    let output = serde_json::json!({
        "exit_code": code,
        "stdout": String::from_utf8_lossy(&stdout.bytes),
        "stderr": String::from_utf8_lossy(&stderr.bytes),
        "stdout_truncated": stdout.truncated,
        "stderr_truncated": stderr.truncated,
    });
    emit_shell_completion(
        &session,
        &turn_context,
        item_id,
        arguments,
        if status.success() {
            ToolStatus::Completed
        } else {
            ToolStatus::Failed
        },
        output,
    )
    .await;
    Ok(())
}

async fn read_shell_stream<R: tokio::io::AsyncRead + Unpin>(
    session: Arc<Session>,
    turn_context: Arc<TurnContext>,
    item_id: String,
    mut reader: R,
) -> ShellStreamOutput {
    let mut captured = Vec::new();
    let mut truncated = false;
    let mut buffer = vec![0_u8; 4096];
    loop {
        let count = match reader.read(&mut buffer).await {
            Ok(0) | Err(_) => break,
            Ok(count) => count,
        };
        let accepted = accepted_output_bytes(captured.len(), count);
        if accepted > 0 {
            captured.extend_from_slice(&buffer[..accepted]);
            session
                .send_event(
                    turn_context.sub_id(),
                    EventMsg::ExecCommandOutputDelta(DeltaEvent {
                        turn_id: turn_context.sub_id().to_string(),
                        item_id: item_id.clone(),
                        delta: String::from_utf8_lossy(&buffer[..accepted]).into_owned(),
                    }),
                )
                .await;
        }
        if accepted < count && !truncated {
            truncated = true;
            session
                .send_event(
                    turn_context.sub_id(),
                    EventMsg::ExecCommandOutputDelta(DeltaEvent {
                        turn_id: turn_context.sub_id().to_string(),
                        item_id: item_id.clone(),
                        delta: OUTPUT_TRUNCATED_MARKER.into(),
                    }),
                )
                .await;
        }
    }
    ShellStreamOutput {
        bytes: captured,
        truncated,
    }
}

async fn emit_shell_completion(
    session: &Session,
    turn_context: &TurnContext,
    item_id: String,
    arguments: serde_json::Value,
    status: ToolStatus,
    output: serde_json::Value,
) {
    session
        .send_event(
            turn_context.sub_id(),
            EventMsg::ItemCompleted(ItemEvent {
                turn_id: turn_context.sub_id().to_string(),
                item: TurnItem::CommandExecution(ToolItem {
                    id: item_id,
                    name: "user_shell".into(),
                    arguments,
                    output: Some(output),
                    web_action: None,
                    web_page_title: None,
                    media: Vec::new(),
                    file_changes: Vec::new(),
                    status,
                    batch_id: None,
                    execution_mode: None,
                }),
            }),
        )
        .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_budget_accepts_only_the_remaining_capacity() {
        assert_eq!(accepted_output_bytes(0, 10), 10);
        assert_eq!(accepted_output_bytes(MAX_SHELL_CAPTURE_BYTES - 3, 10), 3);
        assert_eq!(accepted_output_bytes(MAX_SHELL_CAPTURE_BYTES, 10), 0);
    }
}
