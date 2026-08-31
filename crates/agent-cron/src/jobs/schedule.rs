//! 调度表达式解析与下次运行时间计算。

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Timelike};

pub fn compute_next_run(schedule: &str, after: DateTime<Local>) -> anyhow::Result<DateTime<Local>> {
    let schedule = schedule.trim();
    if let Some(rest) = schedule.strip_prefix("custom:") {
        return parse_custom(rest, after);
    }
    if let Some(rest) = schedule.strip_prefix("every:") {
        return parse_every(rest, after);
    }
    parse_five_field_cron(schedule, after)
}

/// 解析日历型自定义重复：`custom:<frequency>;every=N;...`。
fn parse_custom(spec: &str, after: DateTime<Local>) -> anyhow::Result<DateTime<Local>> {
    let mut parts = spec.split(';');
    let frequency = parts
        .next()
        .ok_or_else(|| anyhow::anyhow!("自定义重复类型不能为空"))?
        .trim();
    let mut every = 1_u32;
    let mut minute = 0_u32;
    let mut time = (9_u32, 0_u32);
    let mut weekdays = vec![1_u32];
    let mut day = 1_u32;
    let mut month = 1_u32;

    for field in parts {
        let (key, value) = field
            .split_once('=')
            .ok_or_else(|| anyhow::anyhow!("无效的自定义重复字段: {field}"))?;
        match key.trim() {
            "every" => {
                every = value
                    .trim()
                    .parse()
                    .map_err(|_| anyhow::anyhow!("无效的重复间隔: {value}"))?;
                if every == 0 || every > 999 {
                    anyhow::bail!("重复间隔必须在 1-999 之间");
                }
            }
            "minute" => {
                minute = parse_bounded(value, 0, 59, "分钟")?;
            }
            "time" => time = parse_clock_time(value)?,
            "wd" => weekdays = parse_weekday_filter(value)?,
            "day" => day = parse_bounded(value, 1, 31, "日期")?,
            "month" => month = parse_bounded(value, 1, 12, "月份")?,
            other => anyhow::bail!("不支持的自定义重复字段: {other}"),
        }
    }

    match frequency {
        "hourly" => next_custom_hourly(after, every, minute),
        "daily" => next_custom_daily(after, every, time),
        "weekly" => next_custom_weekly(after, every, &weekdays, time),
        "monthly" => next_custom_monthly(after, every, day, time),
        "yearly" => next_custom_yearly(after, every, month, day, time),
        _ => anyhow::bail!("不支持的自定义重复类型: {frequency}"),
    }
}

fn parse_bounded(raw: &str, min: u32, max: u32, label: &str) -> anyhow::Result<u32> {
    let value: u32 = raw
        .trim()
        .parse()
        .map_err(|_| anyhow::anyhow!("无效的{label}: {raw}"))?;
    if value < min || value > max {
        anyhow::bail!("{label}越界: {value}（范围 {min}-{max}）");
    }
    Ok(value)
}

fn parse_clock_time(raw: &str) -> anyhow::Result<(u32, u32)> {
    let (hour, minute) = raw
        .trim()
        .split_once(':')
        .ok_or_else(|| anyhow::anyhow!("无效时间: {raw}"))?;
    Ok((
        parse_bounded(hour, 0, 23, "小时")?,
        parse_bounded(minute, 0, 59, "分钟")?,
    ))
}

fn local_candidate(date: NaiveDate, hour: u32, minute: u32) -> Option<DateTime<Local>> {
    let naive = date.and_hms_opt(hour, minute, 0)?;
    Local.from_local_datetime(&naive).earliest()
}

fn next_custom_hourly(
    after: DateTime<Local>,
    every: u32,
    minute: u32,
) -> anyhow::Result<DateTime<Local>> {
    let mut cursor = (after + Duration::minutes(1))
        .with_second(0)
        .and_then(|value| value.with_nanosecond(0))
        .unwrap_or(after + Duration::minutes(1));
    let limit = usize::try_from(every).unwrap_or(999) * 60 + 48 * 60;
    for _ in 0..=limit {
        let absolute_hour = cursor.timestamp().div_euclid(3600);
        if cursor.minute() == minute && absolute_hour.rem_euclid(i64::from(every)) == 0 {
            return Ok(cursor);
        }
        cursor += Duration::minutes(1);
    }
    anyhow::bail!("无法计算下一个每 {every} 小时计划")
}

fn next_custom_daily(
    after: DateTime<Local>,
    every: u32,
    time: (u32, u32),
) -> anyhow::Result<DateTime<Local>> {
    let anchor = NaiveDate::from_ymd_opt(1970, 1, 1).expect("valid epoch date");
    for offset in 0..=i64::from(every) + 1 {
        let date = after.date_naive() + Duration::days(offset);
        let day_index = date.signed_duration_since(anchor).num_days();
        if day_index.rem_euclid(i64::from(every)) != 0 {
            continue;
        }
        if let Some(candidate) = local_candidate(date, time.0, time.1) {
            if candidate > after {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!("无法计算下一个每 {every} 天计划")
}

fn next_custom_weekly(
    after: DateTime<Local>,
    every: u32,
    weekdays: &[u32],
    time: (u32, u32),
) -> anyhow::Result<DateTime<Local>> {
    let anchor = NaiveDate::from_ymd_opt(1970, 1, 5).expect("valid Monday anchor");
    let limit = i64::from(every) * 7 + 7;
    for offset in 0..=limit {
        let date = after.date_naive() + Duration::days(offset);
        if !weekdays.contains(&date.weekday().num_days_from_sunday()) {
            continue;
        }
        let week_index = date.signed_duration_since(anchor).num_days().div_euclid(7);
        if week_index.rem_euclid(i64::from(every)) != 0 {
            continue;
        }
        if let Some(candidate) = local_candidate(date, time.0, time.1) {
            if candidate > after {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!("无法计算下一个每 {every} 周计划")
}

fn next_custom_monthly(
    after: DateTime<Local>,
    every: u32,
    day: u32,
    time: (u32, u32),
) -> anyhow::Result<DateTime<Local>> {
    let start_index = after.year() * 12 + i32::try_from(after.month0()).unwrap_or(0);
    let anchor_index = 1970 * 12;
    let limit = i64::from(every) * 12 + 12;
    for offset in 0..=limit {
        let index = start_index + i32::try_from(offset).unwrap_or(i32::MAX);
        if (index - anchor_index).rem_euclid(i32::try_from(every).unwrap_or(1)) != 0 {
            continue;
        }
        let year = index.div_euclid(12);
        let month = u32::try_from(index.rem_euclid(12) + 1).unwrap_or(1);
        let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
            continue;
        };
        if let Some(candidate) = local_candidate(date, time.0, time.1) {
            if candidate > after {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!("无法计算下一个每 {every} 月计划")
}

fn next_custom_yearly(
    after: DateTime<Local>,
    every: u32,
    month: u32,
    day: u32,
    time: (u32, u32),
) -> anyhow::Result<DateTime<Local>> {
    let limit = i32::try_from(every).unwrap_or(999) * 8 + 8;
    for offset in 0..=limit {
        let year = after.year() + offset;
        if (year - 1970).rem_euclid(i32::try_from(every).unwrap_or(1)) != 0 {
            continue;
        }
        let Some(date) = NaiveDate::from_ymd_opt(year, month, day) else {
            continue;
        };
        if let Some(candidate) = local_candidate(date, time.0, time.1) {
            if candidate > after {
                return Ok(candidate);
            }
        }
    }
    anyhow::bail!("无法计算下一个每 {every} 年计划")
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
fn parse_five_field_cron(expr: &str, after: DateTime<Local>) -> anyhow::Result<DateTime<Local>> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        anyhow::bail!(
            "无效调度表达式: {expr}。请使用 every:5m / every:1h、custom:...，或五段 cron（分 时 日 月 周）"
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
        let ok_min = match_field(&minute, cursor.minute());
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
