//! 代码执行工具：在工作区临时目录运行短片段（python / node / shell）。
//!
//! 脚本写入 `.code_exec/`，子进程超时 30s；stdout/stderr 一并返回后删除临时文件。

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
    /// 语言：`python`（默认）/ `javascript`|`js` / `shell`|`bash`。
    #[serde(default)]
    pub language: Option<String>,
}

/// 向注册表登记 `code_exec` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "code_exec".to_string(),
        toolset: "code_exec".to_string(),
        description: "Execute a short code snippet. language: python|javascript|shell (default python)."
            .to_string(),
        schema: schema_for_args::<CodeExecArgs>(),
        check_fn: None,
        icon: "code-2",
    });
}

/// 按语言选择解释器执行代码，返回 exit code 与输出。
///
/// # 错误
/// 参数无效、spawn 失败，或超过 30 秒超时。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    use std::process::Stdio;
    use std::time::Duration;

    let parsed: CodeExecArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("code_exec 参数无效: {e}"))?;
    let lang = parsed
        .language
        .as_deref()
        .unwrap_or("python")
        .to_lowercase();

    ctx.ensure_workspace()?;
    let tmp = ctx.workspace_dir.join(".code_exec");
    std::fs::create_dir_all(&tmp)?;

    let (program, script_args, filename): (&str, Vec<&str>, &str) = match lang.as_str() {
        "javascript" | "js" => ("node", vec![], "snippet.js"),
        "shell" | "bash" => ("sh", vec![], "snippet.sh"),
        _ => ("python3", vec![], "snippet.py"),
    };

    let path = tmp.join(filename);
    std::fs::write(&path, &parsed.code)?;

    let mut cmd = tokio::process::Command::new(program);
    for a in script_args {
        cmd.arg(a);
    }
    cmd.arg(&path)
        .current_dir(&ctx.workspace_dir)
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
    Ok(format!(
        "exit={code_status}\n--- stdout ---\n{stdout}\n--- stderr ---\n{stderr}"
    ))
}
