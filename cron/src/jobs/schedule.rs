//! 调度表达式解析与下次运行时间计算。

use chrono::{DateTime, Datelike, Duration, Local, Timelike};

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

