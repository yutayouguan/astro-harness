//! 后台任务：让 `terminal` 的长命令脱离 60s 超时在后台运行，
//! 再由 `terminal_job` 工具轮询输出、等待完成或终止。
//!
//! 由于工具在每次调用时都跑在临时 tokio 运行时上（见 `agent` 层快照执行），
//! 后台进程不能挂在 [`crate::context::ToolContext`] 上——它会随运行时一起被回收。
//! 因此这里用**进程级全局注册表**（`OnceLock<Mutex<..>>`）持有 OS 子进程，
//! stdout/stderr 由独立 OS 线程抽取写入共享缓冲，生命周期与临时运行时解耦。

use std::collections::VecDeque;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

/// 单个后台任务缓冲的输出上限（字节）；超出后停止追加并标记截断。
const MAX_JOB_OUTPUT: usize = 1024 * 1024;

/// 全局注册表最多保留的任务数；超出时优先淘汰最老的已结束任务。
const MAX_JOBS: usize = 50;

/// 单次 `status`/`wait` 轮询返回的输出字节上限。
const MAX_POLL_BYTES: usize = 60 * 1024;

/// 任务运行状态。
#[derive(Clone, Debug)]
enum JobStatus {
    Running,
    Exited(Option<i32>),
    Killed,
    Failed(String),
}

impl JobStatus {
    fn is_terminal(&self) -> bool {
        !matches!(self, JobStatus::Running)
    }

    fn label(&self) -> String {
        match self {
            JobStatus::Running => "running".to_string(),
            JobStatus::Exited(Some(c)) => format!("exited(code={c})"),
            JobStatus::Exited(None) => "exited(signal)".to_string(),
            JobStatus::Killed => "killed".to_string(),
            JobStatus::Failed(e) => format!("failed({e})"),
        }
    }
}

/// 追加式输出缓冲（stdout/stderr 合并，按到达顺序）。
#[derive(Default)]
struct JobBuf {
    data: Vec<u8>,
    truncated: bool,
}

/// 一个后台任务的运行时句柄。
struct Job {
    id: String,
    session_id: String,
    command: String,
    cwd: String,
    pid: u32,
    started: Instant,
    output: Arc<Mutex<JobBuf>>,
    status: Arc<Mutex<JobStatus>>,
    child: Arc<Mutex<Option<std::process::Child>>>,
}

impl Job {
    fn status(&self) -> JobStatus {
        self.status.lock().unwrap().clone()
    }
}

/// 进程级任务注册表（按插入顺序保存，便于淘汰最老）。
#[derive(Default)]
struct Registry {
    jobs: VecDeque<Arc<Job>>,
}

impl Registry {
    fn insert(&mut self, job: Arc<Job>) {
        self.jobs.push_back(job);
        while self.jobs.len() > MAX_JOBS {
            // 优先淘汰已结束任务，否则淘汰最老
            let pos = self
                .jobs
                .iter()
                .position(|j| j.status().is_terminal())
                .unwrap_or(0);
            self.jobs.remove(pos);
        }
    }

    fn get(&self, id: &str) -> Option<Arc<Job>> {
        self.jobs.iter().find(|j| j.id == id).cloned()
    }
}

fn registry() -> &'static Mutex<Registry> {
    static REG: OnceLock<Mutex<Registry>> = OnceLock::new();
    REG.get_or_init(|| Mutex::new(Registry::default()))
}

/// 把子进程某个输出流持续读入共享缓冲，直至 EOF。
fn drain<R: Read>(mut reader: R, buf: Arc<Mutex<JobBuf>>) {
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                let mut b = buf.lock().unwrap();
                if b.data.len() >= MAX_JOB_OUTPUT {
                    b.truncated = true;
                    continue;
                }
                let room = MAX_JOB_OUTPUT - b.data.len();
                if n <= room {
                    b.data.extend_from_slice(&chunk[..n]);
                } else {
                    b.data.extend_from_slice(&chunk[..room]);
                    b.truncated = true;
                }
            }
            Err(_) => break,
        }
    }
}

/// 在后台启动一条 shell 命令，返回任务 id。
///
/// 使用 `std::process`（而非 tokio）+ 独立线程抽取输出，确保子进程不随
/// 临时工具运行时被回收。命令通过 `sh -c` 执行，`cwd` 为已校验的沙箱路径。
pub fn spawn_background(
    session_id: &str,
    command: &str,
    cwd: &Path,
    cwd_display: &str,
) -> anyhow::Result<String> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(command)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id();

    let output = Arc::new(Mutex::new(JobBuf::default()));
    if let Some(out) = child.stdout.take() {
        let b = output.clone();
        std::thread::spawn(move || drain(out, b));
    }
    if let Some(err) = child.stderr.take() {
        let b = output.clone();
        std::thread::spawn(move || drain(err, b));
    }

    let status = Arc::new(Mutex::new(JobStatus::Running));
    let child_arc = Arc::new(Mutex::new(Some(child)));

    {
        let status = status.clone();
        let child_arc = child_arc.clone();
        std::thread::spawn(move || loop {
            std::thread::sleep(Duration::from_millis(120));
            let mut guard = child_arc.lock().unwrap();
            let Some(ch) = guard.as_mut() else { break };
            match ch.try_wait() {
                Ok(Some(st)) => {
                    let mut s = status.lock().unwrap();
                    // kill 可能已把状态置为 Killed，仅在仍 Running 时写 Exited
                    if matches!(*s, JobStatus::Running) {
                        *s = JobStatus::Exited(st.code());
                    }
                    *guard = None;
                    break;
                }
                Ok(None) => {}
                Err(e) => {
                    *status.lock().unwrap() = JobStatus::Failed(e.to_string());
                    *guard = None;
                    break;
                }
            }
        });
    }

    let id = format!(
        "job_{}",
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let job = Arc::new(Job {
        id: id.clone(),
        session_id: session_id.to_string(),
        command: command.to_string(),
        cwd: cwd_display.to_string(),
        pid,
        started: Instant::now(),
        output,
        status,
        child: child_arc,
    });
    registry().lock().unwrap().insert(job);
    Ok(id)
}

/// `terminal_job` 工具参数。
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct JobArgs {
    /// 动作：`list` | `status` | `wait` | `kill`。
    pub action: String,
    /// 任务 id（`status`/`wait`/`kill` 必填）。
    #[serde(default)]
    pub id: Option<String>,
    /// `status`/`wait`：从该字节偏移起返回新输出（默认 0）。
    #[serde(default)]
    pub offset: Option<usize>,
    /// `wait`：最多等待秒数（默认 30，钳制 1..=600）。
    #[serde(default)]
    pub timeout_secs: Option<u64>,
}

/// 向注册表登记 `terminal_job` 工具（与 `terminal` 同 toolset，一起启停）。
pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "terminal_job".to_string(),
        toolset: "terminal".to_string(),
        description: "Manage background shell jobs started by terminal(background=true). \
             action=list shows this session's jobs; status returns new output from offset (poll again with the returned offset); \
             wait blocks up to timeout_secs (default 30, max 600) until the job ends; kill terminates it. \
             Output is combined stdout/stderr, capped at 1MiB total and 60KiB per status/wait call."
            .to_string(),
        schema: schema_for_args::<JobArgs>(),
        check_fn: None,
        icon: "list-checks",
        ..ToolEntry::lifecycle_defaults()
    });
}

crate::submit_builtin_tool! {
    register: register,
    names: ["terminal_job"],
    async_ctx: dispatch,
}

/// 按 `action` 管理后台任务。
pub async fn dispatch(ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: JobArgs = serde_json::from_value(args.clone())
        .map_err(|e| anyhow::anyhow!("terminal_job 参数无效: {e}"))?;
    let action = parsed.action.trim().to_lowercase();

    match action.as_str() {
        "list" => Ok(list_jobs(&ctx.session_id)),
        "status" | "poll" => {
            let job = require_job(&parsed.id)?;
            Ok(render_status(&job, parsed.offset.unwrap_or(0)))
        }
        "wait" => {
            let job = require_job(&parsed.id)?;
            let timeout = parsed.timeout_secs.unwrap_or(30).clamp(1, 600);
            let deadline = Instant::now() + Duration::from_secs(timeout);
            while !job.status().is_terminal() && Instant::now() < deadline {
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
            Ok(render_status(&job, parsed.offset.unwrap_or(0)))
        }
        "kill" => {
            let job = require_job(&parsed.id)?;
            kill_job(&job);
            Ok(format!("已请求终止任务 {}（{}）", job.id, job.command))
        }
        other => anyhow::bail!("未知 action: {other}（应为 list|status|wait|kill）"),
    }
}

fn require_job(id: &Option<String>) -> anyhow::Result<Arc<Job>> {
    let id = id
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("该 action 需要 id 参数"))?;
    registry()
        .lock()
        .unwrap()
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("未找到任务 {id}（可能已被淘汰）"))
}

fn kill_job(job: &Arc<Job>) {
    let mut guard = job.child.lock().unwrap();
    if let Some(ch) = guard.as_mut() {
        let _ = ch.kill();
        let _ = ch.wait();
        *guard = None;
        *job.status.lock().unwrap() = JobStatus::Killed;
    }
}

fn list_jobs(session_id: &str) -> String {
    let reg = registry().lock().unwrap();
    let mut lines: Vec<String> = Vec::new();
    for job in reg.jobs.iter().filter(|j| j.session_id == session_id) {
        let cmd = truncate_one_line(&job.command, 80);
        lines.push(format!(
            "{}  [{}]  pid={} age={}s  cwd={}\n  {}",
            job.id,
            job.status().label(),
            job.pid,
            job.started.elapsed().as_secs(),
            job.cwd,
            cmd
        ));
    }
    if lines.is_empty() {
        "当前会话没有后台任务。用 terminal(background=true) 启动一个。".to_string()
    } else {
        format!("后台任务（{}）:\n{}", lines.len(), lines.join("\n"))
    }
}

fn render_status(job: &Arc<Job>, offset: usize) -> String {
    let status = job.status();
    let (slice, total, next_offset, buf_truncated) = {
        let b = job.output.lock().unwrap();
        let total = b.data.len();
        let start = offset.min(total);
        let end = (start + MAX_POLL_BYTES).min(total);
        let slice = String::from_utf8_lossy(&b.data[start..end]).to_string();
        (slice, total, end, b.truncated)
    };

    let mut out = format!(
        "[job {}] status={} pid={} age={}s cwd={}\ncommand: {}\n--- output bytes {}..{} of {} ---\n{}",
        job.id,
        status.label(),
        job.pid,
        job.started.elapsed().as_secs(),
        job.cwd,
        truncate_one_line(&job.command, 200),
        offset.min(total),
        next_offset,
        total,
        slice
    );

    if next_offset < total {
        out.push_str(&format!(
            "\n[more output; poll again with action=status id={} offset={}]",
            job.id, next_offset
        ));
    } else if !status.is_terminal() {
        out.push_str(&format!(
            "\n[still running, no new output; poll again with action=status id={} offset={} or action=wait]",
            job.id, next_offset
        ));
    }
    if buf_truncated {
        out.push_str("\n[output buffer hit 1MiB cap; earliest/overflow bytes dropped]");
    }
    out
}

fn truncate_one_line(s: &str, max: usize) -> String {
    let one = s.replace('\n', " ");
    if one.chars().count() > max {
        let t: String = one.chars().take(max).collect();
        format!("{t}…")
    } else {
        one
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wait_terminal(id: &str, secs: u64) -> JobStatus {
        let deadline = Instant::now() + Duration::from_secs(secs);
        loop {
            let job = registry().lock().unwrap().get(id).unwrap();
            let st = job.status();
            if st.is_terminal() || Instant::now() > deadline {
                return st;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    #[test]
    fn background_job_captures_output_and_exits() {
        let dir = tempfile::tempdir().unwrap();
        let id = spawn_background("s1", "printf 'hello-bg'", dir.path(), ".").unwrap();
        let st = wait_terminal(&id, 5);
        assert!(matches!(st, JobStatus::Exited(Some(0))), "status={st:?}");
        let job = registry().lock().unwrap().get(&id).unwrap();
        let rendered = render_status(&job, 0);
        assert!(rendered.contains("hello-bg"), "{rendered}");
        assert!(rendered.contains("exited(code=0)"));
    }

    #[test]
    fn kill_stops_running_job() {
        let dir = tempfile::tempdir().unwrap();
        let id = spawn_background("s2", "sleep 30", dir.path(), ".").unwrap();
        // 给它起来的时间
        std::thread::sleep(Duration::from_millis(200));
        let job = registry().lock().unwrap().get(&id).unwrap();
        assert!(!job.status().is_terminal());
        kill_job(&job);
        assert!(matches!(job.status(), JobStatus::Killed));
    }

    #[test]
    fn list_filters_by_session() {
        let dir = tempfile::tempdir().unwrap();
        let id = spawn_background("sess-xyz", "printf done", dir.path(), ".").unwrap();
        wait_terminal(&id, 5);
        let listed = list_jobs("sess-xyz");
        assert!(listed.contains(&id), "{listed}");
        let other = list_jobs("sess-none");
        assert!(!other.contains(&id));
    }

    #[test]
    fn offset_paging_returns_no_duplicate() {
        let dir = tempfile::tempdir().unwrap();
        let id = spawn_background("s3", "printf 'abcdef'", dir.path(), ".").unwrap();
        wait_terminal(&id, 5);
        let job = registry().lock().unwrap().get(&id).unwrap();
        let full = render_status(&job, 0);
        assert!(full.contains("abcdef"));
        // 从末尾偏移读，输出片段应为空（header 仍会回显 command，故只检查 marker 之后的正文）
        let tail = render_status(&job, 6);
        assert!(tail.contains("bytes 6..6 of 6"));
        let body = tail.rsplit("---\n").next().unwrap_or("");
        assert!(
            !body.contains("abcdef"),
            "output body after offset should be empty: {body:?}"
        );
    }
}
