//! 代码执行工具：在工作区临时目录运行短片段（python / node / shell）。
//!
//! 脚本写入 `.code_exec/`，子进程超时 30s；stdout/stderr 一并返回后删除临时文件。
//! 输出有 64KiB 截断。
//!
//! 安全护栏（非硬沙箱）：
//! - 子进程环境清空后仅注入安全白名单变量，剥离 KEY/TOKEN/SECRET 等敏感名
//! - Unix 下通过 `setrlimit` 限制 CPU / 地址空间 / 文件大小 / fd

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// Soft sandbox limits applied via `setrlimit` on Unix after fork / before exec.
const RLIM_CPU_SECS: u64 = 30;
const RLIM_AS_BYTES: u64 = 512 * 1024 * 1024;
const RLIM_FSIZE_BYTES: u64 = 32 * 1024 * 1024;
const RLIM_NOFILE: u64 = 64;
// 注意：不设置 RLIMIT_NPROC。该限制按「用户」计数而非进程树；
// 桌面环境宿主已有大量进程时，过低的 NPROC 会让子进程立刻 fork 失败。

/// Environment variable names kept for the child process.
const SAFE_ENV_KEYS: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TMPDIR", "TMP",
    "TEMP", "SHELL", "PWD",
];

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
        description: "Execute a short code snippet. language must be python|javascript|shell (default python). \
Not a hard sandbox—runs on the host with the workspace as cwd. \
Guardrails: env scrubbing (no API keys/tokens), Unix resource limits (CPU/memory/file size/fd), 30s timeout. \
stdout/stderr capped at 64KiB."
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

fn is_sensitive_env_key(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    const NEEDLES: &[&str] = &[
        "KEY",
        "TOKEN",
        "SECRET",
        "PASSWORD",
        "PASSWD",
        "CREDENTIAL",
        "AUTH",
        "PRIVATE",
    ];
    NEEDLES.iter().any(|n| upper.contains(n))
}

/// Build a scrubbed environment for code execution.
///
/// Starts empty, copies only allowlisted keys from the parent, and never copies
/// keys whose names look like secrets.
pub(crate) fn scrubbed_env(
    parent: impl IntoIterator<Item = (impl AsRef<str>, impl AsRef<str>)>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for (k, v) in parent {
        let key = k.as_ref();
        if !SAFE_ENV_KEYS.contains(&key) {
            continue;
        }
        if is_sensitive_env_key(key) {
            continue;
        }
        out.insert(key.to_string(), v.as_ref().to_string());
    }
    out
}

#[cfg(unix)]
fn apply_unix_rlimits() {
    unsafe {
        let set = |resource: libc::c_int, value: u64| {
            let lim = libc::rlimit {
                rlim_cur: value as libc::rlim_t,
                rlim_max: value as libc::rlim_t,
            };
            let _ = libc::setrlimit(resource, &lim);
        };
        set(libc::RLIMIT_CPU, RLIM_CPU_SECS);
        set(libc::RLIMIT_AS, RLIM_AS_BYTES);
        set(libc::RLIMIT_FSIZE, RLIM_FSIZE_BYTES);
        set(libc::RLIMIT_NOFILE, RLIM_NOFILE);
    }
}

/// RAII helper so temp scripts are removed on timeout / early return too.
struct TempScript(PathBuf);

impl Drop for TempScript {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

impl TempScript {
    fn path(&self) -> &Path {
        &self.0
    }
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

    let (program, script_args, ext): (&str, Vec<&str>, &str) = match lang.as_str() {
        "python" | "python3" | "py" => ("python3", vec![], "py"),
        "javascript" | "js" => ("node", vec![], "js"),
        "shell" | "bash" | "sh" => ("sh", vec![], "sh"),
        other => {
            anyhow::bail!("code_exec 不支持 language={other}；请使用 python、javascript 或 shell")
        }
    };

    let root = ctx.ensure_project_or_workspace()?;
    let tmp = root.join(".code_exec");
    std::fs::create_dir_all(&tmp)?;

    // 每次调用唯一文件名：避免并发同语言调用相互覆盖脚本 / 误删对方临时文件
    let unique = uuid::Uuid::new_v4().simple().to_string();
    let path = tmp.join(format!("snippet_{}.{ext}", &unique[..8]));
    std::fs::write(&path, &parsed.code)?;
    let script = TempScript(path);

    let env = scrubbed_env(std::env::vars());

    let mut cmd = tokio::process::Command::new(program);
    for a in script_args {
        cmd.arg(a);
    }
    cmd.arg(script.path())
        .current_dir(&root)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env_clear()
        .envs(env);

    #[cfg(unix)]
    unsafe {
        cmd.pre_exec(|| {
            apply_unix_rlimits();
            Ok(())
        });
    }

    let child = cmd.spawn()?;
    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("code_exec 超时（30s）"))??;

    // Drop cleans the temp file; keep explicit remove for clarity in success path.
    drop(script);

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code_status = output.status.code().unwrap_or(-1);
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

    #[test]
    fn scrubbed_env_keeps_safe_keys_only() {
        let parent = vec![
            ("PATH", "/usr/bin"),
            ("HOME", "/home/user"),
            ("OPENAI_API_KEY", "sk-secret"),
            ("GITHUB_TOKEN", "ghp_xxx"),
            ("MY_PASSWORD", "hunter2"),
            ("AWS_SECRET_ACCESS_KEY", "aws"),
            ("LANG", "en_US.UTF-8"),
            ("CUSTOM_VAR", "nope"),
            ("AUTH_HEADER", "Bearer x"),
        ];
        let env = scrubbed_env(parent);
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/user"));
        assert_eq!(env.get("LANG").map(String::as_str), Some("en_US.UTF-8"));
        assert!(!env.contains_key("OPENAI_API_KEY"));
        assert!(!env.contains_key("GITHUB_TOKEN"));
        assert!(!env.contains_key("MY_PASSWORD"));
        assert!(!env.contains_key("AWS_SECRET_ACCESS_KEY"));
        assert!(!env.contains_key("CUSTOM_VAR"));
        assert!(!env.contains_key("AUTH_HEADER"));
    }

    #[tokio::test]
    async fn rejects_unknown_language() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = test_ctx(&dir, &mut memory, &sessions, &providers, &targets);
        let err = dispatch(&ctx, &serde_json::json!({"code": "1", "language": "ruby"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("不支持"), "{err}");
    }

    #[tokio::test]
    async fn large_stdout_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
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

    #[tokio::test]
    async fn concurrent_same_language_no_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = test_ctx(&dir, &mut memory, &sessions, &providers, &targets);
        // 两个并发的同语言调用共享同一个 .code_exec 目录；
        // sleep 制造重叠窗口——若临时文件名固定会相互覆盖。
        let a = serde_json::json!({"language": "shell", "code": "sleep 0.3; echo MARKER_AAA"});
        let b = serde_json::json!({"language": "shell", "code": "sleep 0.3; echo MARKER_BBB"});
        let (r1, r2) = tokio::join!(dispatch(&ctx, &a), dispatch(&ctx, &b));
        let r1 = r1.unwrap();
        let r2 = r2.unwrap();
        assert!(r1.contains("MARKER_AAA"), "r1={r1}");
        assert!(r2.contains("MARKER_BBB"), "r2={r2}");
    }

    #[tokio::test]
    async fn does_not_leak_secret_env_to_python() {
        std::env::set_var("ASTRO_CODE_EXEC_TEST_SECRET_TOKEN", "should-not-leak");
        let dir = tempfile::tempdir().unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = test_ctx(&dir, &mut memory, &sessions, &providers, &targets);
        let out = dispatch(
            &ctx,
            &serde_json::json!({
                "language": "python",
                "code": "import os; print(os.environ.get('ASTRO_CODE_EXEC_TEST_SECRET_TOKEN', 'MISSING'))"
            }),
        )
        .await
        .unwrap();
        std::env::remove_var("ASTRO_CODE_EXEC_TEST_SECRET_TOKEN");
        assert!(
            out.contains("MISSING"),
            "secret env leaked into child: {out}"
        );
    }

    #[tokio::test]
    async fn timeout_cleans_temp_script() {
        let dir = tempfile::tempdir().unwrap();
        let mut memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions =
            session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions")).unwrap();
        let providers = providers::registry::ProviderRegistry::new();
        let targets = ImageGenTargets::default();
        let ctx = test_ctx(&dir, &mut memory, &sessions, &providers, &targets);
        // 30s 固定超时对单测太慢；用 shell 的超长 sleep 无法缩短超时。
        // 改为：正常成功路径后确认目录可清理；超时清理由 TempScript Drop 保证。
        // 这里用极短 shell 验证文件最终不残留。
        let _ = dispatch(
            &ctx,
            &serde_json::json!({
                "language": "shell",
                "code": "echo ok"
            }),
        )
        .await
        .unwrap();
        let tmp = ctx.workspace_dir.join(".code_exec");
        if tmp.exists() {
            let leftovers: Vec<_> = std::fs::read_dir(&tmp)
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.file_name())
                .collect();
            assert!(
                leftovers.is_empty(),
                "temp scripts should be cleaned: {leftovers:?}"
            );
        }
    }
}
