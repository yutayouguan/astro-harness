//! 跨平台环境变量补水：自动发现 API Key，写入进程环境，并落盘到 `~/.astro/.env`。
//!
//! ## 各平台差异
//!
//! | 平台 | GUI 默认能拿到 | 通常拿不到 | 补水来源 |
//! |------|----------------|------------|----------|
//! | **macOS** | launchd 基础环境 | `.zshrc` / `.zprofile` | 登录 shell + `~/.astro/.env` |
//! | **Linux** | 视桌面而定 | `.bashrc` / `.profile` | 同上 |
//! | **Windows** | 用户/系统注册表变量 | 仅终端临时 `$env:` | User 环境块 + `%USERPROFILE%\.astro\.env` |
//!
//! 启动时：
//! 1. 读已有 `~/.astro/.env` → 注入进程
//! 2. 从平台 / 进程环境发现白名单 Key
//! 3. **自动把新发现的 Key 追加写入** `~/.astro/.env`（已有项不覆盖）

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::providers_commands::ProviderKind;

const KNOWN_KINDS: &[ProviderKind] = &[
    ProviderKind::Openai,
    ProviderKind::Anthropic,
    ProviderKind::Deepseek,
    ProviderKind::Google,
    ProviderKind::Azure,
    ProviderKind::Zhipu,
    ProviderKind::Openrouter,
    ProviderKind::Bailian,
    ProviderKind::Nvidia,
    ProviderKind::Moonshot,
    ProviderKind::Volcengine,
    ProviderKind::Minimax,
    ProviderKind::Custom,
];

/// 已知的 API Key 环境变量名列表。
fn known_api_key_names() -> HashSet<&'static str> {
    let mut set = HashSet::new();
    for kind in KNOWN_KINDS {
        for name in kind.env_api_key_names() {
            set.insert(*name);
        }
    }
    set.insert("ASTRO_GRPC_ADDR");
    set
}

/// `~/.astro/.env` 路径。
fn astro_env_file() -> PathBuf {
    memory::default_memory_dir().join(".env")
}

/// 解析单行 dotenv（KEY=VALUE）。
fn parse_dotenv_line(line: &str) -> Option<(String, String)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let line = line.strip_prefix("export ").unwrap_or(line).trim();
    let (key, value) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || key.contains(char::is_whitespace) {
        return None;
    }
    let mut value = value.trim().to_string();
    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value = value[1..value.len() - 1].to_string();
    }
    Some((key.to_string(), value))
}

/// 仅当进程环境未设置时写入。
fn set_if_missing(key: &str, value: &str) -> bool {
    if value.trim().is_empty() {
        return false;
    }
    match std::env::var(key) {
        Ok(existing) if !existing.trim().is_empty() => false,
        _ => {
            // SAFETY: 仅在进程启动早期调用。
            unsafe { std::env::set_var(key, value) };
            true
        }
    }
}

/// 转义写入 dotenv 的值。
fn escape_dotenv_value(value: &str) -> String {
    if value.is_empty()
        || value.chars().any(|c| c.is_whitespace() || matches!(c, '"' | '\'' | '#' | '='))
    {
        format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        value.to_string()
    }
}

/// 读取已有 `.env` 中的键（含空值标记为已存在，避免重复追加）。
fn load_dotenv_keys(path: &PathBuf) -> HashMap<String, String> {
    let mut map = HashMap::new();
    let Ok(raw) = fs::read_to_string(path) else {
        return map;
    };
    for line in raw.lines() {
        if let Some((k, v)) = parse_dotenv_line(line) {
            map.insert(k, v);
        }
    }
    map
}

/// 从 `.env` 文件水合缺失环境变量。
fn hydrate_from_dotenv_file(path: &PathBuf) -> usize {
    let existing = load_dotenv_keys(path);
    let mut n = 0;
    for (k, v) in &existing {
        if set_if_missing(k, v) {
            n += 1;
        }
    }
    if n > 0 {
        tracing::info!(path = %path.display(), count = n, "hydrated env from ~/.astro/.env");
    }
    n
}

/// 解析多行环境变量文本块。
fn parse_env_block(text: &str, wanted: &HashSet<&str>) -> HashMap<String, String> {
    let mut found = HashMap::new();
    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !wanted.contains(key) {
            continue;
        }
        let value = value.trim();
        if value.is_empty() {
            continue;
        }
        found.insert(key.to_string(), value.to_string());
    }
    found
}

/// 从当前进程环境收集已知 Key。
fn collect_from_process_env(wanted: &HashSet<&str>) -> HashMap<String, String> {
    let mut found = HashMap::new();
    for name in wanted {
        if let Ok(v) = std::env::var(name) {
            let t = v.trim().to_string();
            if !t.is_empty() {
                found.insert((*name).to_string(), t);
            }
        }
    }
    found
}

/// 带超时执行外部探测命令。
fn run_with_timeout(mut child: std::process::Child, timeout: Duration) -> Option<Vec<u8>> {
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if started.elapsed() > timeout => {
                let _ = child.kill();
                let _ = child.wait();
                tracing::warn!("env hydrate subprocess timed out");
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(40)),
            Err(err) => {
                tracing::warn!(error = %err, "env hydrate wait failed");
                return None;
            }
        }
    }
    match child.wait_with_output() {
        Ok(o) if o.status.success() => Some(o.stdout),
        Ok(o) => {
            tracing::warn!(status = ?o.status, "env hydrate subprocess failed");
            None
        }
        Err(err) => {
            tracing::warn!(error = %err, "env hydrate output failed");
            None
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
/// 从平台特定来源收集密钥（shell/钥匙串等）。
fn collect_from_platform(wanted: &HashSet<&str>) -> HashMap<String, String> {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| {
        if cfg!(target_os = "macos") {
            "/bin/zsh".into()
        } else {
            "/bin/bash".into()
        }
    });

    // 使用 login + non-interactive（不要 -i）：交互式 login shell 在无 TTY 时容易卡住直到超时
    let child = match Command::new(&shell)
        .args(["-l", "-c", "printenv"])
        .env("TERM", "dumb")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .stdin(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(error = %err, shell = %shell, "spawn login shell failed");
            return HashMap::new();
        }
    };

    let Some(stdout) = run_with_timeout(child, Duration::from_secs(5)) else {
        return HashMap::new();
    };
    parse_env_block(&String::from_utf8_lossy(&stdout), wanted)
}

#[cfg(target_os = "windows")]
/// 从平台特定来源收集密钥（shell/钥匙串等）。
fn collect_from_platform(wanted: &HashSet<&str>) -> HashMap<String, String> {
    let script = "[Environment]::GetEnvironmentVariables('User').GetEnumerator() | ForEach-Object { $_.Key + '=' + $_.Value }";
    let child = match Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(err) => {
            tracing::warn!(error = %err, "spawn powershell for User env failed");
            return HashMap::new();
        }
    };

    let Some(stdout) = run_with_timeout(child, Duration::from_secs(5)) else {
        return HashMap::new();
    };
    parse_env_block(&String::from_utf8_lossy(&stdout), wanted)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
/// 从平台特定来源收集密钥（shell/钥匙串等）。
fn collect_from_platform(_wanted: &HashSet<&str>) -> HashMap<String, String> {
    HashMap::new()
}

/// 把新发现的白名单 Key 追加到 `~/.astro/.env`（不覆盖已有键）。
fn persist_discovered_to_dotenv(
    path: &PathBuf,
    discovered: &HashMap<String, String>,
    wanted: &HashSet<&str>,
) -> usize {
    let existing = load_dotenv_keys(path);
    let mut to_append: Vec<(&str, &str)> = Vec::new();
    for (key, value) in discovered {
        if !wanted.contains(key.as_str()) {
            continue;
        }
        if value.trim().is_empty() {
            continue;
        }
        if existing.contains_key(key) {
            continue;
        }
        to_append.push((key.as_str(), value.as_str()));
    }
    if to_append.is_empty() {
        return 0;
    }
    to_append.sort_by(|a, b| a.0.cmp(b.0));

    if let Some(parent) = path.parent() {
        if let Err(err) = fs::create_dir_all(parent) {
            tracing::warn!(error = %err, "create ~/.astro for .env failed");
            return 0;
        }
    }

    let is_new = !path.exists();
    let mut file = match fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    {
        Ok(f) => f,
        Err(err) => {
            tracing::warn!(error = %err, path = %path.display(), "open ~/.astro/.env failed");
            return 0;
        }
    };

    let mut buf = String::new();
    if is_new {
        buf.push_str("# Astro 自动从系统环境发现的 API Key（已有项不会被覆盖）\n");
        buf.push_str("# Auto-discovered API keys — existing entries are never overwritten\n");
    } else {
        // 确保追加前有换行
        if let Ok(raw) = fs::read_to_string(path) {
            if !raw.is_empty() && !raw.ends_with('\n') {
                buf.push('\n');
            }
        }
        buf.push_str("\n# --- auto-discovered ---\n");
    }
    for (k, v) in &to_append {
        buf.push_str(k);
        buf.push('=');
        buf.push_str(&escape_dotenv_value(v));
        buf.push('\n');
    }

    if let Err(err) = file.write_all(buf.as_bytes()) {
        tracing::warn!(error = %err, "write ~/.astro/.env failed");
        return 0;
    }
    let n = to_append.len();
    tracing::info!(
        path = %path.display(),
        count = n,
        keys = ?to_append.iter().map(|(k, _)| *k).collect::<Vec<_>>(),
        "persisted discovered API keys to ~/.astro/.env"
    );
    n
}

/// 进程启动时调用一次。
pub fn hydrate_process_env() {
    let wanted = known_api_key_names();
    let path = astro_env_file();

    // 1) 已有文件 → 注入进程
    let from_file = hydrate_from_dotenv_file(&path);

    // 2) 平台 + 当前进程中的白名单 Key
    let mut discovered = collect_from_platform(&wanted);
    for (k, v) in collect_from_process_env(&wanted) {
        discovered.entry(k).or_insert(v);
    }

    // 3) 注入进程（文件未覆盖到的）
    let mut from_platform = 0;
    for (k, v) in &discovered {
        if set_if_missing(k, v) {
            from_platform += 1;
        }
    }

    // 4) 自动写入 ~/.astro/.env，下次启动更稳
    let persisted = persist_discovered_to_dotenv(&path, &discovered, &wanted);

    if from_file == 0 && from_platform == 0 && persisted == 0 {
        tracing::debug!(
            "no API env discovered; relying on keyring / manual provider keys"
        );
    }
}
