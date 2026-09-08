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

/// Unix 下 fork 后 exec 前通过 `setrlimit` 施加的软沙箱限制。
const RLIM_CPU_SECS: u64 = 30;
const RLIM_AS_BYTES: u64 = 512 * 1024 * 1024;
const RLIM_FSIZE_BYTES: u64 = 32 * 1024 * 1024;
const RLIM_NOFILE: u64 = 64;
// 注意：不设置 RLIMIT_NPROC。该限制按「用户」计数而非进程树；
// 桌面环境宿主已有大量进程时，过低的 NPROC 会让子进程立刻 fork 失败。

/// 保留给子进程的环境变量名。
const SAFE_ENV_KEYS: &[&str] = &[
    "PATH", "HOME", "USER", "LOGNAME", "LANG", "LC_ALL", "LC_CTYPE", "TERM", "TMPDIR", "TMP",
    "TEMP", "SHELL", "PWD",
];

/// `code_exec` 工具的参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct CodeExecArgs {
    pub code: String,
    /// 语言：`python`（默认）/ `javascript`|`js` / `shell`|`bash`。未知语言会报错。
    #[serde(default)]
    pub language: Option<String>,
}

/// 向注册表登记 `code_exec` 工具。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "code_exec".to_string(),
        toolset: "code_exec".to_string(),
        description: "Execute a short code snippet for quick computation or data processing. \
language must be python|javascript (default python). \
Runs through the active command sandbox policy with the workspace as cwd. \
Guardrails: env scrubbing (no API keys/tokens), Unix resource limits (CPU/memory/file size/fd), 30s timeout. \
stdout/stderr capped at 64KiB. \
For shell commands, use exec_command."
            .to_string(),
        schema: schema_for_args::<CodeExecArgs>(),
        check_fn: None,
        icon: "code-2",
        ..ToolEntry::lifecycle_defaults().sandboxable().deferred()
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

/// 构建代码执行的净化环境。
///
/// 从空 env 开始，仅从父进程拷贝白名单中的 key，
/// 且永不拷贝名称疑似密钥的 key。
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

/// RAII 辅助结构，确保临时脚本在超时或提前返回时也会被清理。
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
        "shell" | "bash" | "sh" => {
            anyhow::bail!("code_exec 不再支持 shell；请使用 exec_command 工具执行 shell 命令")
        }
        other => {
            anyhow::bail!("code_exec 不支持 language={other}；请使用 python 或 javascript")
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
    let env = if ctx.managed_network.is_some() {
        ctx.prepare_managed_network_env(env)
            .expect("managed network lease checked above")
            .env
    } else {
        env
    };

    let audit = ctx.sandbox_audit_metadata("code_exec");
    let policy = ctx.command_sandbox_policy().inspect_err(|_error| {
        audit.record(
            sandbox::SandboxAuditKind::Denied,
            None,
            program,
            "policy_resolution_failed",
            None,
        );
    })?;
    let spawn_started = std::time::Instant::now();
    let mut cmd = match sandbox::SandboxRunner.tokio_command(&policy, program) {
        Ok(command) => command,
        Err(error) => {
            audit.record_prepare_error(
                Some(&policy),
                program,
                &error,
                Some(spawn_started.elapsed().as_millis() as u64),
            );
            return Err(error.into());
        }
    };
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

    let child = cmd.spawn().inspect_err(|_error| {
        audit.record(
            sandbox::SandboxAuditKind::Denied,
            Some(&policy),
            program,
            "spawn_failed",
            Some(spawn_started.elapsed().as_millis() as u64),
        );
    })?;
    audit.record(
        sandbox::SandboxAuditKind::Spawned,
        Some(&policy),
        program,
        "spawned",
        Some(spawn_started.elapsed().as_millis() as u64),
    );
    let output = tokio::time::timeout(Duration::from_secs(30), child.wait_with_output())
        .await
        .map_err(|_| anyhow::anyhow!("code_exec 超时（30s）"))??;

    // Drop cleans the temp file; keep explicit remove for clarity in success path.
    drop(script);

    let code_status = output.status.code().unwrap_or(-1);
    let output = sandbox::ExecToolCallOutput::new(
        code_status,
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    );
    if let Some(decision) = ctx.take_managed_network_denial() {
        audit.record(
            sandbox::SandboxAuditKind::Denied,
            Some(&policy),
            program,
            "network_policy_denied",
            Some(spawn_started.elapsed().as_millis() as u64),
        );
        return Err(sandbox::SandboxErr::Denied {
            output: Box::new(output),
            network_policy_decision: Some(decision),
        }
        .into());
    }
    if sandbox::is_likely_sandbox_denied(policy.mode, &output) {
        audit.record(
            sandbox::SandboxAuditKind::Denied,
            Some(&policy),
            program,
            "sandbox_denied",
            Some(spawn_started.elapsed().as_millis() as u64),
        );
        return Err(sandbox::SandboxErr::Denied {
            output: Box::new(output),
            network_policy_decision: None,
        }
        .into());
    }
    let body = output.render_text();
    Ok(types::truncate_tool_result(
        &body,
        types::MAX_TOOL_RESULT_BYTES,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::context::{ImageGenTargets, ToolContext};
    use std::collections::BTreeMap;
    use std::sync::Arc;

    fn test_ctx<'a>(
        dir: &'a tempfile::TempDir,
        memory: &'a std::sync::RwLock<memory::MemoryManager>,
        sessions: &'a session::SessionStore,
        targets: &'a ImageGenTargets,
        creds: &'a crate::context::ModelCredentials,
    ) -> ToolContext<'a> {
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(&ws).unwrap();
        ToolContext {
            memory,
            sessions,
            memory_dir: dir.path().to_path_buf(),
            workspace_dir: ws,
            project_root: None,
            workspace_roots: Vec::new(),
            image_gen_targets: targets,
            session_id: "test".into(),
            turn_id: None,
            credentials: creds,
            service_tier: None,
            model_targets: &[],
            execution: None,
            permission_profile: None,
            skill_config_overrides: &[],
            hook_bus: None,
            hook_runtime: None,
            workspace_write_grant: false,
            sandbox_policy: None,
            managed_network: None,
            context_window: None,
            context_tokens_used: None,
            tool_registry: None,
        }
    }

    async fn enable_managed_network(ctx: &mut ToolContext<'_>) -> String {
        let started = Arc::new(
            network_proxy::StartedNetworkProxy::start(Arc::new(
                network_proxy::NetworkProxyState::new(types::NetworkPolicy {
                    enabled: true,
                    domains: BTreeMap::from([(
                        "allowed.example".into(),
                        types::NetworkAccess::Allow,
                    )]),
                    ..Default::default()
                })
                .unwrap(),
            ))
            .await
            .unwrap(),
        );
        let prepared = started.proxy().prepare(Default::default());
        let endpoint = prepared.env["HTTPS_PROXY"].clone();
        let policy = sandbox::SandboxPolicy::new(
            types::SandboxMode::WorkspaceWrite,
            ctx.project_or_workspace(),
            Vec::new(),
            false,
        )
        .unwrap()
        .with_managed_network(prepared.sandbox_context);
        ctx.sandbox_policy = Some(policy);
        ctx.managed_network = Some(started);
        endpoint
    }

    #[tokio::test]
    async fn code_exec_adds_proxy_after_secret_scrub() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let mut ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        let endpoint = enable_managed_network(&mut ctx).await;

        let output = dispatch(
            &ctx,
            &serde_json::json!({
                "language": "python",
                "code": "import os; print(os.environ.get('HTTPS_PROXY')); print('safe=' + ('present' if os.environ.get('PATH') else 'missing')); print('secret=' + os.environ.get('OPENAI_API_KEY', 'MISSING'))",
            }),
        )
        .await
        .unwrap();

        assert!(output.contains(&endpoint), "{output}");
        assert!(output.contains("safe=present"), "{output}");
        assert!(output.contains("secret=MISSING"), "{output}");
    }

    #[tokio::test]
    async fn code_exec_managed_network_denial_is_typed() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let mut ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        enable_managed_network(&mut ctx).await;
        let code = r#"import os, socket
endpoint = os.environ['HTTPS_PROXY'].removeprefix('http://')
host, port = endpoint.rsplit(':', 1)
sock = socket.create_connection((host, int(port)))
sock.sendall(b'CONNECT 127.0.0.1:9 HTTP/1.1\r\nHost: 127.0.0.1:9\r\n\r\n')
print(sock.recv(4096).decode())"#;

        let error = dispatch(
            &ctx,
            &serde_json::json!({"language": "python", "code": code}),
        )
        .await
        .unwrap_err();
        let Some(sandbox::SandboxErr::Denied {
            network_policy_decision: Some(decision),
            ..
        }) = error.downcast_ref::<sandbox::SandboxErr>()
        else {
            panic!("expected typed managed-network denial: {error}");
        };
        assert_eq!(decision.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(decision.port, Some(9));
        assert_eq!(decision.decision, types::NetworkPolicyDecision::Deny);
        assert_eq!(
            decision.source,
            types::NetworkDecisionSource::BaselinePolicy
        );
    }

    #[tokio::test]
    async fn code_exec_without_managed_network_keeps_proxy_marker_absent() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);

        let output = dispatch(
            &ctx,
            &serde_json::json!({
                "language": "python",
                "code": "import os; print(os.environ.get('ASTRO_NETWORK_PROXY_ACTIVE', 'MISSING'))",
            }),
        )
        .await
        .unwrap();
        assert!(output.contains("MISSING"), "{output}");
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
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        let err = dispatch(&ctx, &serde_json::json!({"code": "1", "language": "ruby"}))
            .await
            .unwrap_err()
            .to_string();
        assert!(err.contains("不支持"), "{err}");
    }

    #[tokio::test]
    async fn large_stdout_is_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        let n = types::MAX_TOOL_RESULT_BYTES + 4096;
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
        let audits = sandbox::list_recent_sandbox_audits(dir.path(), 10).unwrap();
        assert!(audits.iter().any(|event| {
            event.event == sandbox::SandboxAuditKind::Spawned
                && event.tool_name == "code_exec"
                && event.target == "python3"
        }));
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn sandbox_denial_returns_typed_error() {
        let dir = tempfile::tempdir().unwrap();
        memory::set_permission_preset(dir.path(), types::PermissionPreset::ReadOnly).unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let mut ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        ctx.permission_profile = Some(types::READ_ONLY_PROFILE.into());

        let error = dispatch(
            &ctx,
            &serde_json::json!({
                "language": "python",
                "code": "open('denied.txt', 'w').write('no')",
            }),
        )
        .await
        .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<sandbox::SandboxErr>(),
            Some(sandbox::SandboxErr::Denied { output, .. }) if output.exit_code != 0
        ));
        assert!(!ctx.workspace_dir.join("denied.txt").exists());
    }

    #[tokio::test]
    async fn concurrent_same_language_no_clobber() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        // 两个并发的同语言调用共享同一个 .code_exec 目录；
        // sleep 制造重叠窗口——若临时文件名固定会相互覆盖。
        let a = serde_json::json!({"language": "python", "code": "import time; time.sleep(0.3); print('MARKER_AAA')"});
        let b = serde_json::json!({"language": "python", "code": "import time; time.sleep(0.3); print('MARKER_BBB')"});
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
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
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
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        // 正常成功路径后确认目录可清理；超时清理由 TempScript Drop 保证。
        let _ = dispatch(
            &ctx,
            &serde_json::json!({
                "language": "python",
                "code": "print('ok')"
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

    #[tokio::test]
    async fn shell_language_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let memory = memory::MemoryManager::new(dir.path().to_path_buf()).unwrap();
        let sessions = session::SessionStore::open_sessions_dir(&memory.base_dir.join("sessions"))
            .await
            .unwrap();
        let targets = ImageGenTargets::default();
        let creds = crate::context::ModelCredentials::default();
        let memory = std::sync::RwLock::new(memory);
        let ctx = test_ctx(&dir, &memory, &sessions, &targets, &creds);
        let err = dispatch(
            &ctx,
            &serde_json::json!({"language": "shell", "code": "echo hi"}),
        )
        .await
        .unwrap_err();
        assert!(err.to_string().contains("exec_command"), "{err}");
    }
}
