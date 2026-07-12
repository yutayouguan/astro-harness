//! 定时任务调度：定义、持久化、到期扫描与工具分发。
//!
//! 职责：
//! - 将任务定义持久化到 `~/.astro/cron/jobs.json`
//! - 解析 `every:` / 五段 cron / `once:` 调度表达式并计算下次运行时间
//! - `claim_due` / `tick` 扫描到期任务并推进 `next_run_at`
//! - 为 Agent 工具与 Extractor 提供自然语言 → 结构化任务的入口
//!
//! 不变量：
//! - 任务 id 为 UUID，持久化前会校验 schedule 可解析
//! - `once:` 任务触发后自动禁用且清空 `next_run_at`
//! - `jobs.json` 通过临时文件原子写入，避免半写损坏

use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Datelike, Duration, Local, Timelike, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::workspace::{default_memory_dir, ensure_default_workspace};

/// 定时任务定义，持久化在 `~/.astro/cron/jobs.json`
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    /// 唯一 id（UUID 字符串）
    pub id: String,
    /// 调度表达式：`every:5m` / `every:1h` / 五段 cron（分 时 日 月 周）
    pub schedule: String,
    /// 到期时交给 Agent 执行的完整指令
    pub task: String,
    /// 短标题（UI 展示；缺省由 task 首行截取）
    #[serde(default)]
    pub title: String,
    /// 执行该任务的 Agent id
    #[serde(default = "default_agent_id")]
    pub agent_id: String,
    /// 可选：覆盖 Agent 默认 Provider
    #[serde(default)]
    pub provider_id: Option<String>,
    /// 可选：覆盖 Agent 默认模型
    #[serde(default)]
    pub model: Option<String>,
    /// 是否参与调度扫描
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// ISO 8601 创建时间
    pub created_at: String,
    /// 上次实际触发时间（手动 touch 或 claim_due）
    #[serde(default)]
    pub last_run_at: Option<String>,
    /// 下次计划触发时间；`once:` 触发后为 null
    #[serde(default)]
    pub next_run_at: Option<String>,
    /// 是否在聊天时间线展示本次执行
    #[serde(default)]
    pub show_in_chat: bool,
}

/// 创建或更新定时任务的输入（供 Tauri / 上层调用）
#[derive(Debug, Clone)]
pub struct NewCronJob {
    /// 调度表达式（同 [`CronJob::schedule`]）
    pub schedule: String,
    /// 到期执行的指令
    pub task: String,
    /// 短标题
    pub title: String,
    /// 目标 Agent id
    pub agent_id: String,
    /// 可选 Provider 覆盖
    pub provider_id: Option<String>,
    /// 可选模型覆盖
    pub model: Option<String>,
    /// 是否在聊天中展示
    pub show_in_chat: bool,
}

/// 自然语言 → 定时任务的结构化抽取目标（Extractor `submit`）。
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
pub struct CronJobExtract {
    /// `every:5m` / `every:1h` / 五段 cron / `once:RFC3339`
    pub schedule: String,
    /// 到期时交给 Agent 执行的指令
    pub task: String,
    /// 可选短标题；缺省由 task 首行截取
    #[serde(default)]
    pub title: Option<String>,
}

/// Extractor preamble：约束 schedule 语法。
pub fn cron_extract_preamble() -> &'static str {
    r#"从用户自然语言提炼一条定时任务。
规则：
1. schedule 必须是下列之一：
   - every:Nm / every:Nh / every:Nd（N 为正整数；可选 ;wd=1,2,3 限定周几，0=周日）
   - 五段 cron：分 时 日 月 周（例如每天 09:00 → 0 9 * * *）
   - once:RFC3339（必须带时区，如 2026-07-12T15:00:00+08:00）
2. task 是到期时要执行的完整指令，保留用户意图，不要空。
3. title 可选，简短中文标题；不确定时可省略。
4. 不要编造用户没说的调度细节；缺省时间可用每天 09:00。"#
}

/// 校验并规范化抽取结果（trim、补 title、验证 schedule 可解析）。
pub fn normalize_cron_extract(mut draft: CronJobExtract) -> anyhow::Result<CronJobExtract> {
    draft.schedule = draft.schedule.trim().to_string();
    draft.task = draft.task.trim().to_string();
    if let Some(t) = draft.title.as_mut() {
        *t = t.trim().to_string();
        if t.is_empty() {
            draft.title = None;
        }
    }
    if draft.task.is_empty() {
        anyhow::bail!("task 为空");
    }
    if draft.schedule.is_empty() {
        anyhow::bail!("schedule 为空");
    }
    let _ = compute_next_run(&draft.schedule, Local::now())?;
    if draft.title.is_none() {
        draft.title = Some(title_from_task(&draft.task));
    }
    Ok(draft)
}

fn default_true() -> bool {
    true
}

/// 旧版 jobs.json 缺省 agent_id
fn default_agent_id() -> String {
    "default".into()
}

/// 从 task 首行截取最多 40 字符作为默认标题
fn title_from_task(task: &str) -> String {
    task.lines()
        .next()
        .unwrap_or(task)
        .chars()
        .take(40)
        .collect()
}

/// `jobs.json` 顶层结构
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct JobsFile {
    jobs: Vec<CronJob>,
}

/// 定时任务持久化存储（根目录含 `jobs.json` 与 `output/`）
pub struct CronStore {
    root: PathBuf,
}

impl CronStore {
    /// 打开或初始化指定 cron 根目录
    pub fn open(root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        fs::create_dir_all(root.join("output"))?;
        Ok(Self { root })
    }

    /// 打开默认 `~/.astro/cron`（会先确保工作区存在）
    pub fn open_default() -> anyhow::Result<Self> {
        let _ = ensure_default_workspace()?;
        Self::open(default_memory_dir().join("cron"))
    }

    /// `jobs.json` 路径
    pub fn jobs_path(&self) -> PathBuf {
        self.root.join("jobs.json")
    }

    /// 列出全部任务（加载时补全缺省 title / agent_id）
    pub fn list(&self) -> anyhow::Result<Vec<CronJob>> {
        Ok(self.load()?.jobs)
    }

    /// 快捷添加：仅 schedule + task，其余用默认值
    pub fn add(&self, schedule: &str, task: &str) -> anyhow::Result<CronJob> {
        self.add_job(NewCronJob {
            schedule: schedule.to_string(),
            task: task.to_string(),
            title: title_from_task(task),
            agent_id: default_agent_id(),
            provider_id: None,
            model: None,
            show_in_chat: false,
        })
    }

    /// 添加新任务：校验 schedule、计算 `next_run_at` 并持久化
    pub fn add_job(&self, input: NewCronJob) -> anyhow::Result<CronJob> {
        let schedule = input.schedule.trim();
        let task = input.task.trim();
        if schedule.is_empty() {
            anyhow::bail!("schedule 不能为空");
        }
        if task.is_empty() {
            anyhow::bail!("task 不能为空");
        }
        let title = if input.title.trim().is_empty() {
            title_from_task(task)
        } else {
            input.title.trim().to_string()
        };
        let agent_id = if input.agent_id.trim().is_empty() {
            default_agent_id()
        } else {
            input.agent_id.trim().to_string()
        };
        // 校验表达式
        let next = compute_next_run(schedule, Local::now())?
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let mut file = self.load()?;
        let job = CronJob {
            id: Uuid::new_v4().to_string(),
            schedule: schedule.to_string(),
            task: task.to_string(),
            title,
            agent_id,
            provider_id: input.provider_id,
            model: input.model,
            enabled: true,
            created_at: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            last_run_at: None,
            next_run_at: Some(next),
            show_in_chat: input.show_in_chat,
        };
        file.jobs.push(job.clone());
        self.save(&file)?;
        Ok(job)
    }

    /// 按 id 或 id 前缀更新任务；未找到返回 `Ok(None)`
    pub fn update_job(&self, id_or_prefix: &str, input: NewCronJob) -> anyhow::Result<Option<CronJob>> {
        let schedule = input.schedule.trim();
        let task = input.task.trim();
        if schedule.is_empty() {
            anyhow::bail!("schedule 不能为空");
        }
        if task.is_empty() {
            anyhow::bail!("task 不能为空");
        }
        let title = if input.title.trim().is_empty() {
            title_from_task(task)
        } else {
            input.title.trim().to_string()
        };
        let agent_id = if input.agent_id.trim().is_empty() {
            default_agent_id()
        } else {
            input.agent_id.trim().to_string()
        };
        let next = compute_next_run(schedule, Local::now())?
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let mut file = self.load()?;
        let mut updated = None;
        for job in &mut file.jobs {
            if job.id == id_or_prefix || job.id.starts_with(id_or_prefix) {
                job.schedule = schedule.to_string();
                job.task = task.to_string();
                job.title = title;
                job.agent_id = agent_id;
                job.provider_id = input.provider_id;
                job.model = input.model;
                job.show_in_chat = input.show_in_chat;
                job.next_run_at = Some(next);
                updated = Some(job.clone());
                break;
            }
        }
        if updated.is_some() {
            self.save(&file)?;
        }
        Ok(updated)
    }

    /// 按 id 或前缀删除任务；未找到返回 `Ok(false)`
    pub fn remove(&self, id_or_prefix: &str) -> anyhow::Result<bool> {
        let mut file = self.load()?;
        let before = file.jobs.len();
        file.jobs.retain(|j| {
            j.id != id_or_prefix && !j.id.starts_with(id_or_prefix)
        });
        if file.jobs.len() == before {
            return Ok(false);
        }
        self.save(&file)?;
        Ok(true)
    }

    /// 手动执行后更新 `last_run_at`（不推进 `next_run_at`）
    pub fn touch_last_run(&self, id_or_prefix: &str, fired_at: Option<String>) -> anyhow::Result<bool> {
        let fired_at = fired_at.unwrap_or_else(|| {
            Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
        });
        let mut file = self.load()?;
        let mut found = false;
        for job in &mut file.jobs {
            if job.id == id_or_prefix || job.id.starts_with(id_or_prefix) {
                job.last_run_at = Some(fired_at);
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
        self.save(&file)?;
        Ok(true)
    }

    /// 启用或禁用任务；启用时会重算过期的 `next_run_at`（`once:` 过期则报错）
    pub fn set_enabled(&self, id_or_prefix: &str, enabled: bool) -> anyhow::Result<bool> {
        let mut file = self.load()?;
        let mut found = false;
        for job in &mut file.jobs {
            if job.id == id_or_prefix || job.id.starts_with(id_or_prefix) {
                if enabled {
                    if job.schedule.trim().starts_with("once:") {
                        let next = compute_next_run(&job.schedule, Local::now())?;
                        if next <= Local::now() {
                            anyhow::bail!("无法启用已过期的单次任务");
                        }
                        job.next_run_at = Some(
                            next.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        );
                    } else if job.next_run_at.is_none() {
                        job.next_run_at = Some(
                            compute_next_run(&job.schedule, Local::now())?
                                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                        );
                    }
                }
                job.enabled = enabled;
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
        self.save(&file)?;
        Ok(true)
    }

    /// 扫描到期任务：更新 next_run，返回已触发的任务（不写 output JSON）
    pub fn claim_due(&self) -> anyhow::Result<Vec<CronJob>> {
        let now = Local::now();
        let mut file = self.load()?;
        let mut fired = Vec::new();

        for job in &mut file.jobs {
            if !job.enabled {
                continue;
            }
            let due = match &job.next_run_at {
                Some(next) => match DateTime::parse_from_rfc3339(next) {
                    Ok(dt) => dt <= now,
                    Err(_) => true,
                },
                None => true,
            };
            if !due {
                continue;
            }

            let fired_at = now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            job.last_run_at = Some(fired_at);
            if job.schedule.trim().starts_with("once:") {
                job.enabled = false;
                job.next_run_at = None;
            } else {
                job.next_run_at = Some(
                    compute_next_run(&job.schedule, now + Duration::seconds(1))?
                        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                );
            }
            fired.push(job.clone());
        }

        if !fired.is_empty() {
            self.save(&file)?;
        } else {
            // 仍保存可能被修正的 next_run（首次）
            for job in &mut file.jobs {
                if job.enabled && job.next_run_at.is_none() {
                    job.next_run_at = Some(
                        compute_next_run(&job.schedule, now)?
                            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
                    );
                }
            }
            self.save(&file)?;
        }

        Ok(fired)
    }

    /// 扫描到期任务（兼容旧调用方）：写 heartbeat，委托 `claim_due`
    pub fn tick(&self) -> anyhow::Result<Vec<CronJob>> {
        let now = Local::now();
        self.write_heartbeat(&now)?;
        let fired = self.claim_due()?;
        if !fired.is_empty() {
            self.write_last_success(&now)?;
        }
        Ok(fired)
    }

    /// 从磁盘加载 `jobs.json`；不存在或空文件返回空列表
    fn load(&self) -> anyhow::Result<JobsFile> {
        let path = self.jobs_path();
        if !path.exists() {
            return Ok(JobsFile::default());
        }
        let raw = fs::read_to_string(&path)?;
        if raw.trim().is_empty() {
            return Ok(JobsFile::default());
        }
        let mut file: JobsFile = serde_json::from_str(&raw)?;
        for job in &mut file.jobs {
            if job.title.trim().is_empty() {
                job.title = title_from_task(&job.task);
            }
            if job.agent_id.trim().is_empty() {
                job.agent_id = default_agent_id();
            }
        }
        Ok(file)
    }

    /// 原子写入 `jobs.json`（先写 `.json.tmp` 再 rename）
    fn save(&self, file: &JobsFile) -> anyhow::Result<()> {
        let path = self.jobs_path();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_string_pretty(file)?)?;
        fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// 记录调度器心跳时间（`ticker_heartbeat` 文件）
    fn write_heartbeat(&self, now: &DateTime<Local>) -> anyhow::Result<()> {
        fs::write(
            self.root.join("ticker_heartbeat"),
            now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        )?;
        Ok(())
    }

    /// 记录最近一次成功触发扫描的时间
    fn write_last_success(&self, now: &DateTime<Local>) -> anyhow::Result<()> {
        fs::write(
            self.root.join("ticker_last_success"),
            now.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        )?;
        Ok(())
    }
}

/// 计算 `schedule` 在 `after` 之后的下一次触发时刻（本地时区）
///
/// 支持 `once:RFC3339`、`every:Nm|h|d`（可选 `;wd=`）与五段 cron。
pub fn compute_next_run(
    schedule: &str,
    after: DateTime<Local>,
) -> anyhow::Result<DateTime<Local>> {
    let schedule = schedule.trim();
    if let Some(rest) = schedule.strip_prefix("once:") {
        let dt = DateTime::parse_from_rfc3339(rest.trim())
            .or_else(|_| DateTime::parse_from_str(rest.trim(), "%Y-%m-%dT%H:%M:%S%z"))
            .map_err(|e| anyhow::anyhow!("无效 once 时间: {e}"))?
            .with_timezone(&Local);
        return Ok(dt);
    }
    if let Some(rest) = schedule.strip_prefix("every:") {
        return parse_every(rest, after);
    }
    parse_five_field_cron(schedule, after)
}

/// 解析 `every:Nunit` 主表达式与 `;wd=` 工作日过滤器
fn parse_every(spec: &str, after: DateTime<Local>) -> anyhow::Result<DateTime<Local>> {
    let spec = spec.trim();
    let mut parts = spec.split(';');
    let main = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("无效的 every 表达式: {spec}"))?
        .trim()
        .to_lowercase();
    let mut weekdays: Option<Vec<u32>> = None;
    for filter in parts {
        let filter = filter.trim();
        if let Some(wd) = filter.strip_prefix("wd=") {
            weekdays = Some(parse_weekday_filter(wd)?);
        } else if !filter.is_empty() {
            anyhow::bail!("不支持的 every 过滤器: {filter}");
        }
    }

    let (num_str, unit) = main.split_at(
        main.find(|c: char| !c.is_ascii_digit())
            .ok_or_else(|| anyhow::anyhow!("无效的 every 表达式: {main}"))?,
    );
    let n: i64 = num_str
        .parse()
        .map_err(|_| anyhow::anyhow!("无效的 every 数字: {main}"))?;
    if n <= 0 {
        anyhow::bail!("every 间隔必须 > 0");
    }
    let delta = match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => Duration::seconds(n),
        "m" | "min" | "mins" | "minute" | "minutes" => Duration::minutes(n),
        "h" | "hr" | "hrs" | "hour" | "hours" => Duration::hours(n),
        "d" | "day" | "days" => Duration::days(n),
        _ => anyhow::bail!("不支持的 every 单位: {unit}（可用 s/m/h/d）"),
    };

    let mut candidate = after + delta;
    if let Some(allowed) = weekdays {
        // 最多推进约 14 天，避免无限循环
        let deadline = after + Duration::days(14);
        while candidate <= deadline {
            let wd = candidate.weekday().num_days_from_sunday();
            if allowed.contains(&wd) {
                return Ok(candidate);
            }
            candidate += delta;
        }
        anyhow::bail!("every 在 14 天内找不到匹配的工作日: {spec}");
    }
    Ok(candidate)
}

/// 解析 `wd=1-5` / `wd=1,2,3`（cron 编号：0=Sun … 6=Sat）
fn parse_weekday_filter(raw: &str) -> anyhow::Result<Vec<u32>> {
    let raw = raw.trim();
    if raw.is_empty() {
        anyhow::bail!("wd 过滤器不能为空");
    }
    let mut out = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((a, b)) = part.split_once('-') {
            let start: u32 = a
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("无效 wd 范围: {part}"))?;
            let end: u32 = b
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("无效 wd 范围: {part}"))?;
            if start > 6 || end > 6 || start > end {
                anyhow::bail!("wd 范围越界: {part}（0-6，且 start<=end）");
            }
            for v in start..=end {
                if !out.contains(&v) {
                    out.push(v);
                }
            }
        } else {
            let v: u32 = part
                .parse()
                .map_err(|_| anyhow::anyhow!("无效 wd 值: {part}"))?;
            if v > 6 {
                anyhow::bail!("wd 值越界: {v}（范围 0-6）");
            }
            if !out.contains(&v) {
                out.push(v);
            }
        }
    }
    if out.is_empty() {
        anyhow::bail!("wd 过滤器不能为空");
    }
    Ok(out)
}

/// 简化五段 cron：`分 时 日 月 周`，字段支持 `*`、数字、逗号列表与区间（如 `1-5`、`1,3,5`）
fn parse_five_field_cron(
    expr: &str,
    after: DateTime<Local>,
) -> anyhow::Result<DateTime<Local>> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        anyhow::bail!(
            "无效调度表达式: {expr}。请使用 every:5m / every:1h，或五段 cron（分 时 日 月 周）"
        );
    }

    let minute = parse_cron_field(parts[0], 0, 59)?;
    let hour = parse_cron_field(parts[1], 0, 23)?;
    let day = parse_cron_field(parts[2], 1, 31)?;
    let month = parse_cron_field(parts[3], 1, 12)?;
    let weekday = parse_cron_field(parts[4], 0, 6)?; // 0=Sun

    // 从下一分钟开始扫描，最多扫 366 天
    let mut cursor = after + Duration::minutes(1);
    cursor = cursor
        .with_second(0)
        .and_then(|t| t.with_nanosecond(0))
        .unwrap_or(cursor);

    for _ in 0..(366 * 24 * 60) {
        let ok_min = match_field(&minute, cursor.minute() as u32);
        let ok_hour = match_field(&hour, cursor.hour());
        let ok_day = match_field(&day, cursor.day());
        let ok_month = match_field(&month, cursor.month());
        let wd = cursor.weekday().num_days_from_sunday();
        let ok_wd = match_field(&weekday, wd);
        if ok_min && ok_hour && ok_day && ok_month && ok_wd {
            return Ok(cursor);
        }
        cursor += Duration::minutes(1);
    }
    anyhow::bail!("无法在一年内找到匹配的 cron 时间: {expr}")
}

/// 五段 cron 单字段的解析结果
#[derive(Debug)]
enum CronField {
    Any,
    Value(u32),
    List(Vec<u32>),
}

/// 解析 cron 单字段：`*`、数字、逗号列表或区间
fn parse_cron_field(raw: &str, min: u32, max: u32) -> anyhow::Result<CronField> {
    if raw == "*" {
        return Ok(CronField::Any);
    }
    if raw.contains(',') || raw.contains('-') {
        let mut values = Vec::new();
        for part in raw.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            for v in expand_cron_token(part, min, max)? {
                if !values.contains(&v) {
                    values.push(v);
                }
            }
        }
        if values.is_empty() {
            anyhow::bail!("无效 cron 列表字段: {raw}");
        }
        values.sort_unstable();
        return Ok(CronField::List(values));
    }
    let v: u32 = raw
        .parse()
        .map_err(|_| anyhow::anyhow!("无效 cron 字段: {raw}"))?;
    if v < min || v > max {
        anyhow::bail!("cron 字段越界: {raw}（范围 {min}-{max}）");
    }
    Ok(CronField::Value(v))
}

/// 展开 cron 区间或单值 token
fn expand_cron_token(part: &str, min: u32, max: u32) -> anyhow::Result<Vec<u32>> {
    if let Some((a, b)) = part.split_once('-') {
        let start: u32 = a
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("无效 cron 区间: {part}"))?;
        let end: u32 = b
            .trim()
            .parse()
            .map_err(|_| anyhow::anyhow!("无效 cron 区间: {part}"))?;
        if start > end {
            anyhow::bail!("无效 cron 区间（起>止）: {part}");
        }
        if start < min || end > max {
            anyhow::bail!("cron 区间越界: {part}（范围 {min}-{max}）");
        }
        return Ok((start..=end).collect());
    }
    let v: u32 = part
        .parse()
        .map_err(|_| anyhow::anyhow!("无效 cron 字段: {part}"))?;
    if v < min || v > max {
        anyhow::bail!("cron 字段越界: {part}（范围 {min}-{max}）");
    }
    Ok(vec![v])
}

/// 判断当前时间分量是否匹配已解析的 cron 字段
fn match_field(field: &CronField, value: u32) -> bool {
    match field {
        CronField::Any => true,
        CronField::Value(v) => *v == value,
        CronField::List(vs) => vs.contains(&value),
    }
}

/// 默认 cron 根目录：`~/.astro/cron`
pub fn cron_dir() -> PathBuf {
    default_memory_dir().join("cron")
}

/// Agent 工具入口：根据 `name` 分发 cron_add / list / remove / enable / disable
pub fn dispatch_cron_tool(name: &str, args: &serde_json::Value) -> anyhow::Result<String> {
    let store = CronStore::open_default()?;
    match name {
        "cron_add" | "scheduled" => {
            let schedule = args
                .get("cron")
                .or_else(|| args.get("schedule"))
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少 cron/schedule 参数"))?;
            let task = args
                .get("task")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少 task 参数"))?;
            let job = store.add(schedule, task)?;
            Ok(format!(
                "已创建定时任务 {}\n调度: {}\n下次: {}\n任务: {}",
                &job.id[..8],
                job.schedule,
                job.next_run_at.as_deref().unwrap_or("-"),
                job.task
            ))
        }
        "cron_list" => {
            let jobs = store.list()?;
            if jobs.is_empty() {
                return Ok("暂无定时任务".into());
            }
            let body = jobs
                .iter()
                .map(|j| {
                    format!(
                        "- [{}] {} | {} | next={} | {}",
                        if j.enabled { "on" } else { "off" },
                        &j.id[..8],
                        j.schedule,
                        j.next_run_at.as_deref().unwrap_or("-"),
                        j.task
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            Ok(format!("## 定时任务\n{body}"))
        }
        "cron_remove" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少 id 参数"))?;
            if store.remove(id)? {
                Ok(format!("已删除定时任务: {id}"))
            } else {
                Ok(format!("未找到定时任务: {id}"))
            }
        }
        "cron_enable" | "cron_disable" => {
            let id = args
                .get("id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| anyhow::anyhow!("缺少 id 参数"))?;
            let enabled = name == "cron_enable";
            if store.set_enabled(id, enabled)? {
                Ok(format!(
                    "已{}定时任务: {id}",
                    if enabled { "启用" } else { "禁用" }
                ))
            } else {
                Ok(format!("未找到定时任务: {id}"))
            }
        }
        _ => anyhow::bail!("未知 cron 工具: {name}"),
    }
}

/// 对默认 cron 目录执行一次 tick（供 backend 调度器调用）
pub fn tick_default() -> anyhow::Result<Vec<CronJob>> {
    CronStore::open_default()?.tick()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn every_schedule_and_tick() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store.add("every:1m", "提醒喝水").unwrap();
        assert!(job.next_run_at.is_some());

        // 强制到期
        let mut file = store.load().unwrap();
        file.jobs[0].next_run_at = Some("2000-01-01T00:00:00+00:00".into());
        store.save(&file).unwrap();

        let fired = store.claim_due().unwrap();
        assert_eq!(fired.len(), 1);
        assert_eq!(fired[0].task, "提醒喝水");
        assert!(dir.path().join("output").read_dir().unwrap().next().is_none());
        let after = store.list().unwrap();
        assert!(after[0].next_run_at.as_ref().unwrap().as_str() > "2000");
    }

    #[test]
    fn legacy_job_defaults_show_in_chat_false() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("jobs.json");
        std::fs::write(
            &path,
            r#"{
          "jobs": [{
            "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "schedule": "every:5m",
            "task": "提醒喝水\n第二行",
            "enabled": true,
            "created_at": "2026-01-01T00:00:00Z"
          }]
        }"#,
        )
        .unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let jobs = store.list().unwrap();
        assert!(!jobs[0].show_in_chat);
    }

    #[test]
    fn claim_due_advances_schedule_without_output_files() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store.add("every:1m", "x").unwrap();
        let mut file = store.load().unwrap();
        file.jobs[0].next_run_at = Some("2000-01-01T00:00:00+00:00".into());
        store.save(&file).unwrap();
        let due = store.claim_due().unwrap();
        assert_eq!(due.len(), 1);
        assert!(dir.path().join("output").read_dir().unwrap().next().is_none());
        let after = store.list().unwrap();
        assert!(after[0].next_run_at.as_ref().unwrap().as_str() > "2000");
    }

    #[test]
    fn five_field_cron_parses() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 10, 0, 0).unwrap();
        let next = compute_next_run("30 10 * * *", after).unwrap();
        assert_eq!(next.hour(), 10);
        assert_eq!(next.minute(), 30);
    }

    #[test]
    fn remove_by_prefix() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store.add("every:5m", "t").unwrap();
        assert!(store.remove(&job.id[..8]).unwrap());
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn legacy_job_json_gets_default_title_and_agent() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("jobs.json");
        std::fs::write(
            &path,
            r#"{
          "jobs": [{
            "id": "aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee",
            "schedule": "every:5m",
            "task": "提醒喝水\n第二行",
            "enabled": true,
            "created_at": "2026-01-01T00:00:00Z"
          }]
        }"#,
        )
        .unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let jobs = store.list().unwrap();
        assert_eq!(jobs[0].title, "提醒喝水");
        assert_eq!(jobs[0].agent_id, "default");
        assert!(jobs[0].provider_id.is_none());
        assert!(jobs[0].model.is_none());
    }

    #[test]
    fn once_schedule_next_run_is_that_instant() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 4, 0, 0).unwrap();
        let next = compute_next_run("once:2026-07-11T04:22:00+08:00", after).unwrap();
        let expected = DateTime::parse_from_rfc3339("2026-07-11T04:22:00+08:00")
            .unwrap()
            .with_timezone(&Local);
        assert_eq!(next, expected);
    }

    #[test]
    fn tick_disables_once_job() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store
            .add("once:2000-01-01T00:00:00+00:00", "一次性任务")
            .unwrap();
        assert!(job.enabled);
        let fired = store.tick().unwrap();
        assert_eq!(fired.len(), 1);
        let jobs = store.list().unwrap();
        assert!(!jobs[0].enabled);
        assert!(jobs[0].next_run_at.is_none());
        assert!(jobs[0].last_run_at.is_some());
    }

    #[test]
    fn set_enabled_rejects_expired_once_job() {
        let dir = TempDir::new().unwrap();
        let store = CronStore::open(dir.path()).unwrap();
        let job = store
            .add("once:2000-01-01T00:00:00+00:00", "已过期单次")
            .unwrap();
        let fired = store.tick().unwrap();
        assert_eq!(fired.len(), 1);
        assert!(!store.list().unwrap()[0].enabled);

        let err = store.set_enabled(&job.id, true).unwrap_err();
        assert!(
            err.to_string().contains("无法启用已过期的单次任务"),
            "unexpected error: {err}"
        );
        assert!(!store.list().unwrap()[0].enabled);
    }

    #[test]
    fn once_in_the_past_errors_or_returns_past_for_tick() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 12, 0, 0, 0).unwrap();
        // 约定：once 时间已过则 compute_next_run 仍返回该时刻（让 tick 能判定 due）
        let next = compute_next_run("once:2026-07-11T04:22:00+08:00", after).unwrap();
        assert!(next < after);
    }

    #[test]
    fn every_with_weekday_filter() {
        use chrono::TimeZone;
        // 2026-07-11 是周六
        let sat = Local.with_ymd_and_hms(2026, 7, 11, 10, 0, 0).unwrap();
        let next = compute_next_run("every:1h;wd=1-5", sat).unwrap();
        // 应跳到下周一附近
        assert_eq!(next.weekday().num_days_from_monday(), 0);
    }

    #[test]
    fn cron_weekday_list() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 3, 0, 0).unwrap(); // Sat
        let next = compute_next_run("22 4 * * 1,2,3,4,5", after).unwrap();
        assert!(next.weekday().num_days_from_monday() < 5);
        assert_eq!(next.hour(), 4);
        assert_eq!(next.minute(), 22);
    }

    #[test]
    fn normalize_cron_extract_fills_title_and_validates() {
        let draft = CronJobExtract {
            schedule: " every:5m ".into(),
            task: " 提醒喝水 ".into(),
            title: None,
        };
        let ok = normalize_cron_extract(draft).unwrap();
        assert_eq!(ok.schedule, "every:5m");
        assert_eq!(ok.task, "提醒喝水");
        assert_eq!(ok.title.as_deref(), Some("提醒喝水"));
    }

    #[test]
    fn normalize_cron_extract_rejects_bad_schedule() {
        let draft = CronJobExtract {
            schedule: "sometime".into(),
            task: "x".into(),
            title: None,
        };
        assert!(normalize_cron_extract(draft).is_err());
    }

    #[test]
    fn five_field_cron_accepts_weekday_range() {
        use chrono::TimeZone;
        let after = Local.with_ymd_and_hms(2026, 7, 11, 3, 0, 0).unwrap(); // Sat
        let next = compute_next_run("0 9 * * 1-5", after).unwrap();
        assert!(next.weekday().num_days_from_monday() < 5);
        assert_eq!(next.hour(), 9);
        assert_eq!(next.minute(), 0);
    }
}
