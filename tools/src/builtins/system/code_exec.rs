//! 代码执行工具：在工作区临时目录运行短片段（python / node / shell）。
//!
//! 脚本写入 `.code_exec/`，子进程超时 30s；stdout/stderr 一并返回后删除临时文件。
//! 输出有 64KiB 截断。注意：同语言并行调用会争用固定临时文件名。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// `code_exec` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CodeExecArgs {
    /// 要执行的源代码。
    pub code: String,
    /// 语言：必须是 `python`（默认）/ `javascript`|`js` / `shell`|`bash`。未知语言会报错。
    #[serde(default)]
    pub language: Option<String>,
}

/// 向注册表登记 `code_exec` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "code_exec".to_string(),
        toolset: "code_exec".to_string(),
        description: "Execute a short code snippet. language must be python|javascript|shell (default python). Not a sandbox—same privileges as the host process. stdout/stderr capped at 64KiB."
            .to_string(),
        schema: schema_for_args::<CodeExecArgs>(),
        check_fn: None,
        icon: "code-2",
            ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["code_exec"],
    async_ctx: dispatch,
}

/// 按语言选择解释器执行代码，返回 exit code 与输出。
///
/// # 错误
/// 参数无效、未知 language、spawn 失败，或超过 30 秒超时。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    use std::process::Stdio;
    use std::time::Duration;

    let parsed: CodeExecArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("code_exec 参数无效: {e}"))?;
    let lang = parsed
        .language
        .as_deref()
        .unwrap_or("python")
        .trim()
        .to_lowercase();

    let (program, script_args, filename): (&str, Vec<&str>, &str) = match lang.as_str() {
        "python" | "python3" | "py" => ("python3", vec![], "snippet.py"),
        "javascript" | "js" => ("node", vec![], "snippet.js"),
        "shell" | "bash" | "sh" => ("sh", vec![], "snippet.sh"),
        other => anyhow::bail!(
            "code_exec 不支持 language={other}；请使用 python、javascript 或 shell"
        ),
    };

    let root = ctx.ensure_project_or_workspace()?;
    let tmp = root.join(".code_exec");
    std::fs::create_dir_all(&tmp)?;

    let path = tmp.join(filename);
    std::fs::write(&path, &parsed.code)?;

    let mut cmd = tokio::process::Command::new(program);
    for a in script_args {
        cmd.arg(a);
    }
    cmd.arg(&path)
        .current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let child = cmd.spawn()?;
    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("code_exec 超时（30s）"))??;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code_status = output.status.code().unwrap_or(-1);
    let _ = std::fs::remove_file(&path);
    let body = format!("exit={code_status}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}");
    Ok(common::truncate_tool_result(
        &body,
        common::MAX_TOOL_RESULT_BYTES,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};

    fn test_ctx<'a>(
        dir: &'a tempfile::TempDir,
        memory: &'a mut memory::MemoryManager,
        sessions: &'a session::SessionStore,
        providers: &'a providers::registry::ProviderRegistry,
        targets: &'a ImageGenTargets,
    ) -> ToolContext<'a> {
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        ToolContext {
            memory,
            sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws,
            project_root: None,
            image_gen_targets: targets,
            providers,
            session_id: "test".into(),
            turn_id: None,
            chat_api_key: String::new(),
            chat_base_url: String::new(),
            chat_provider: String::new(),
            chat_model: String::new(),
            chat_targets: vec![],
            delegate_runner: None,
            async_spawner: None,
            orchestration_spawner: None,
            hook_bus: None,
        }
    }

    #[tokio::test]
    async fn rejects_unknown_language() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = test_ctx(&dir, &mut memory, &sessions, &providers, &targets);
        let err = dispatch(
            &ctx,
            &serde_json::json!({"code": "1", "language": "ruby"}),
        )
        .await
        .unwrap_err()
        .to_string();
        assert!(err.contains("不支持"), "{err}");
    }

    #[tokio::test]
    async fn large_stdout_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = test_ctx(&dir, &mut memory, &sessions, &providers, &targets);
        let n = common::MAX_TOOL_RESULT_BYTES + 4096;
        let out = dispatch(
            &ctx,
            &serde_json::json!({
                "language": "python",
                "code": format!("print('b'*{n})"),
            }),
        )
        .await
        .unwrap();
        assert!(out.contains("[truncated]"), "{out}");
    }
}
