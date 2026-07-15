//! 音乐播放工具：mpv+yt-dlp / Spotify / Apple Music / 汽水音乐（macOS）。
//!
//! 优先级（auto）：汽水音乐已安装 → mpv 可用 → Spotify 已安装 → Apple Music 本地库。
//! stop 操作会暂停所有已知播放器。

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::context::ToolContext;
use crate::registry::{ToolEntry, ToolRegistry};
use crate::schema::schema_for_args;

#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct MusicArgs {
    /// 歌曲搜索词，例如 "周杰伦 晴天" 或 "The Beatles Hey Jude"；stop 操作时可省略。
    #[serde(default)]
    pub query: Option<String>,
    /// 操作：`play`（默认）或 `stop`（暂停/停止当前播放）。
    #[serde(default)]
    pub action: Option<String>,
    /// 播放器偏好：`auto`（默认）/ `qishui` / `mpv` / `spotify` / `apple_music`。
    #[serde(default)]
    pub player: Option<String>,
}

pub fn register(registry: &mut ToolRegistry) {
    registry.register(ToolEntry {
        name: "music".to_string(),
        toolset: "music".to_string(),
        description: concat!(
            "Play or stop music. ",
            "Auto-detects the best available player: 汽水音乐 (UI scripting, macOS), ",
            "mpv+yt-dlp (plays any song from YouTube), Spotify (opens search), or Apple Music (local library). ",
            "action: 'play' (default) | 'stop'. ",
            "player: 'auto' (default) | 'qishui' | 'mpv' | 'spotify' | 'apple_music'. ",
            "Note: qishui requires Accessibility permission in System Settings."
        )
        .to_string(),
        schema: schema_for_args::<MusicArgs>(),
        check_fn: None,
        icon: "music",
    });
}

pub async fn dispatch(_ctx: &ToolContext<'_>, args: &serde_json::Value) -> anyhow::Result<String> {
    let parsed: MusicArgs =
        serde_json::from_value(args.clone()).map_err(|e| anyhow::anyhow!("music 参数无效: {e}"))?;

    let action = parsed.action.as_deref().unwrap_or("play");
    if action == "stop" {
        return stop_all().await;
    }

    let query = parsed
        .query
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("music play 需要 query"))?;

    match parsed.player.as_deref().unwrap_or("auto") {
        "qishui" => play_qishui(query).await,
        "mpv" => play_mpv(query).await,
        "spotify" => play_spotify(query).await,
        "apple_music" => play_apple_music(query).await,
        _ => play_auto(query).await,
    }
}

// ── 自动选择 ──────────────────────────────────────────────────────────────

async fn play_auto(query: &str) -> anyhow::Result<String> {
    if app_installed("汽水音乐").await {
        return play_qishui(query).await;
    }
    if has_command("mpv").await {
        return play_mpv(query).await;
    }
    if app_installed("Spotify").await {
        return play_spotify(query).await;
    }
    if app_installed("Music").await {
        return play_apple_music(query).await;
    }
    anyhow::bail!(
        "未找到可用播放器。可用选项：汽水音乐 / mpv（brew install mpv yt-dlp）/ Spotify / Apple Music"
    )
}

// ── mpv + yt-dlp ──────────────────────────────────────────────────────────

async fn play_mpv(query: &str) -> anyhow::Result<String> {
    // 先杀掉上一个 mpv 实例，避免多首歌同时播放
    let _ = tokio::process::Command::new("pkill")
        .arg("-x")
        .arg("mpv")
        .spawn();

    let ytdl_url = format!("ytdl://ytsearch1:{query}");
    tokio::process::Command::new("mpv")
        .args(["--no-video", "--quiet", "--really-quiet"])
        .arg(&ytdl_url)
        .spawn()
        .map_err(|e| anyhow::anyhow!("启动 mpv 失败: {e}（请确保已安装：brew install mpv yt-dlp）"))?;

    Ok(format!("▶ 正在播放「{query}」（mpv + yt-dlp）"))
}

// ── Spotify ───────────────────────────────────────────────────────────────

async fn play_spotify(query: &str) -> anyhow::Result<String> {
    // 先激活 Spotify，再用 AS 触发搜索并播放第一条结果
    let safe = escape_as(query);
    let script = format!(
        r#"tell application "Spotify"
    activate
    play track "spotify:search:{safe}"
end tell"#
    );
    let out = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await?;
    if !out.status.success() {
        // 退路：直接 open spotify: URI（会打开搜索页）
        let uri = format!("spotify:search:{}", urlencoding::encode(query));
        let _ = tokio::process::Command::new("open").arg(&uri).spawn();
        return Ok(format!("▶ 已在 Spotify 搜索「{query}」（请手动点击播放）"));
    }
    Ok(format!("▶ 正在通过 Spotify 播放「{query}」"))
}

// ── Apple Music ───────────────────────────────────────────────────────────

async fn play_apple_music(query: &str) -> anyhow::Result<String> {
    let safe = escape_as(query);
    let script = format!(
        r#"tell application "Music"
    activate
    set hits to search playlist "Library" for "{safe}"
    if length of hits > 0 then
        play (item 1 of hits)
        set t to name of item 1 of hits
        return "playing: " & t
    else
        return "not_found"
    end if
end tell"#
    );
    let out = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await?;
    let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if !out.status.success() || stdout == "not_found" {
        anyhow::bail!("Apple Music 本地库中未找到「{query}」，试试 player=spotify 或 player=mpv");
    }
    let title = stdout.trim_start_matches("playing: ");
    Ok(format!("▶ 正在通过 Apple Music 播放「{title}」"))
}

// ── 汽水音乐（System Events UI 脚本）──────────────────────────────────────
//
// 汽水音乐是 Electron 应用，无 URL Scheme 也无 AppleScript 字典，只能通过
// System Events 模拟键盘操作控制。需要在「系统设置 → 隐私与安全性 → 辅助功能」
// 中为调用方（终端/agent）授权。

async fn play_qishui(query: &str) -> anyhow::Result<String> {
    // 激活（未运行时启动）
    let launch = tokio::process::Command::new("open")
        .args(["-a", "汽水音乐"])
        .output()
        .await?;
    if !launch.status.success() {
        anyhow::bail!("无法打开汽水音乐");
    }

    // 等应用主窗口就绪（首次启动需要更长时间）
    let safe = escape_as(query);
    let script = format!(
        r#"
set appName to "汽水音乐"
set ready to false
repeat 15 times
    if application process appName exists of application "System Events" then
        set ready to true
        exit repeat
    end if
    delay 0.5
end repeat
if not ready then error "汽水音乐进程未就绪"

tell application "System Events"
    tell process appName
        set frontmost to true
        delay 0.8
        -- Cmd+F 打开搜索（汽水音乐支持此快捷键）
        keystroke "f" using command down
        delay 0.6
        -- 清空已有内容后输入搜索词
        keystroke "a" using command down
        delay 0.1
        keystroke "{safe}"
        delay 0.4
        -- 回车确认搜索
        key code 36
    end tell
end tell
return "ok"
"#
    );

    let out = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .await?;

    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        // 常见原因：辅助功能未授权
        if err.contains("not allowed") || err.contains("-1719") || err.contains("1728") {
            anyhow::bail!(
                "汽水音乐 UI 脚本需要辅助功能权限：\
                系统设置 → 隐私与安全性 → 辅助功能 → 添加终端/Claude\n原始错误：{err}"
            );
        }
        anyhow::bail!("汽水音乐 UI 脚本失败: {err}");
    }

    Ok(format!(
        "▶ 已在汽水音乐搜索「{query}」（搜索结果已显示，首条结果请点击播放或按回车）"
    ))
}

// ── 停止播放 ──────────────────────────────────────────────────────────────

async fn stop_all() -> anyhow::Result<String> {
    let mut stopped = vec![];

    // mpv
    if let Ok(mut c) = tokio::process::Command::new("pkill")
        .args(["-x", "mpv"])
        .spawn()
    {
        let _ = c.wait().await;
        stopped.push("mpv");
    }

    // 汽水音乐：发送 Space 键切换暂停（若窗口在前台）
    let qishui_script = r#"tell application "System Events"
    if exists (process "汽水音乐") then
        tell process "汽水音乐"
            keystroke space
        end tell
    end if
end tell"#;
    let _ = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(qishui_script)
        .spawn();
    stopped.push("汽水音乐");

    // Spotify
    let sp_script = r#"tell application "Spotify"
    if it is running then pause
end tell"#;
    let _ = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(sp_script)
        .spawn();
    stopped.push("Spotify");

    // Apple Music
    let am_script = r#"tell application "Music"
    if it is running then pause
end tell"#;
    let _ = tokio::process::Command::new("osascript")
        .arg("-e")
        .arg(am_script)
        .spawn();
    stopped.push("Apple Music");

    Ok(format!("⏹ 已发送停止指令：{}", stopped.join(" / ")))
}

// ── 工具函数 ──────────────────────────────────────────────────────────────

async fn has_command(cmd: &str) -> bool {
    tokio::process::Command::new("which")
        .arg(cmd)
        .output()
        .await
        .map(|o| o.status.success())
        .unwrap_or(false)
}

async fn app_installed(app: &str) -> bool {
    let path = format!("/Applications/{app}.app");
    tokio::fs::metadata(&path).await.is_ok()
}

/// 转义 AppleScript 字符串中的双引号和反斜杠。
fn escape_as(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}
