use anyhow::{bail, Result};
use async_trait::async_trait;

use crate::engine::executor::{NodeExecutor, NodeResult};
use crate::engine::variables::VariableContext;
use crate::model::WorkflowNode;

const MAX_RESPONSE_BYTES: usize = 10 * 1024 * 1024; // 10 MB
const MAX_DELAY_SECONDS: u64 = 86_400; // 24 hours

/// 校验 URL：必须是 http/https，禁止内网和云元数据地址
fn validate_url(url: &str) -> Result<()> {
    let parsed: url::Url = url.parse().map_err(|_| anyhow::anyhow!("无效的 URL: {}", url))?;
    match parsed.scheme() {
        "http" | "https" => {}
        s => bail!("不允许的 URL scheme: {s}，仅支持 http/https"),
    }
    if let Some(host) = parsed.host_str() {
        let h = host.to_lowercase();
        // 去掉 IPv6 方括号
        let h = h.trim_start_matches('[').trim_end_matches(']');
        if h == "localhost"
            || h == "127.0.0.1"
            || h == "::1"
            || h == "0.0.0.0"
            || h.starts_with("10.")
            || h.starts_with("192.168.")
            || is_172_private(h)
            || h.starts_with("fc") || h.starts_with("fd")       // IPv6 ULA
            || h.starts_with("fe80")                              // IPv6 link-local
            || h.starts_with("::ffff:127.") || h.starts_with("::ffff:10.")
            || h.starts_with("::ffff:192.168.") || h.starts_with("::ffff:172.")
            || h == "169.254.169.254"
            || h.ends_with(".internal")
            || h.ends_with(".local")
        {
            bail!("不允许请求内网地址: {host}");
        }
    }
    Ok(())
}

fn is_172_private(host: &str) -> bool {
    if let Some(rest) = host.strip_prefix("172.") {
        if let Some(octet) = rest.split('.').next().and_then(|s| s.parse::<u8>().ok()) {
            return (16..=31).contains(&octet);
        }
    }
    false
}

/// 校验文件路径：规范化后必须在用户目录或 ~/.astro 下，禁止 .. 遍历
fn validate_file_path(path: &str) -> Result<std::path::PathBuf> {
    let p = std::path::Path::new(path);
    let canonical = p.canonicalize().or_else(|_| {
        // 文件不存在时（write/append 场景），检查父目录
        if let Some(parent) = p.parent() {
            let cp = parent.canonicalize()?;
            Ok(cp.join(p.file_name().unwrap_or_default()))
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::NotFound, "路径无效"))
        }
    }).map_err(|e| anyhow::anyhow!("路径解析失败 {}: {}", path, e))?;

    let home_dir = home::user_home_dir().unwrap_or_else(|| std::path::PathBuf::from("/"));
    let astro_dir = home::default_memory_dir();
    if canonical.starts_with(&home_dir) || canonical.starts_with(&astro_dir) {
        Ok(canonical)
    } else {
        bail!("路径 {} 不在允许的目录范围内（用户目录或 ~/.astro）", canonical.display())
    }
}

// ── HttpRequest ──────────────────────────────────────────────────────

pub struct HttpRequestExec;

#[async_trait]
impl NodeExecutor for HttpRequestExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let method = node.config.get("method").and_then(|v| v.as_str()).unwrap_or("GET");
        let url_tpl = node.config.get("url_template").and_then(|v| v.as_str()).unwrap_or("");
        let url = ctx.interpolate(url_tpl);
        let timeout = node.config.get("timeout_seconds").and_then(|v| v.as_u64()).unwrap_or(30).min(300);

        validate_url(&url)?;

        let body_tpl = node.config.get("body_template").and_then(|v| v.as_str()).unwrap_or("");
        let body = if body_tpl.is_empty() {
            None
        } else {
            Some(ctx.interpolate(body_tpl))
        };

        let headers_val = node.config.get("headers").cloned().unwrap_or_default();
        let mut header_map = Vec::new();
        if let Some(arr) = headers_val.as_array() {
            for pair in arr {
                if let Some(inner) = pair.as_array() {
                    if inner.len() >= 2 {
                        let k = ctx.interpolate(inner[0].as_str().unwrap_or_default());
                        let v = ctx.interpolate(inner[1].as_str().unwrap_or_default());
                        header_map.push((k, v));
                    }
                }
            }
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(timeout))
            .redirect(reqwest::redirect::Policy::limited(5))
            .build()?;

        let mut req = match method.to_uppercase().as_str() {
            "POST" => client.post(&url),
            "PUT" => client.put(&url),
            "PATCH" => client.patch(&url),
            "DELETE" => client.delete(&url),
            _ => client.get(&url),
        };

        for (k, v) in &header_map {
            req = req.header(k.as_str(), v.as_str());
        }

        if let Some(body) = body {
            req = req.body(body);
        }

        let resp = req.send().await?;
        let status = resp.status().as_u16();

        // 限制响应大小，防止 OOM
        let content_len = resp.content_length().unwrap_or(0) as usize;
        if content_len > MAX_RESPONSE_BYTES {
            bail!("响应体过大 ({} bytes)，上限 {} bytes", content_len, MAX_RESPONSE_BYTES);
        }
        let resp_bytes = resp.bytes().await?;
        if resp_bytes.len() > MAX_RESPONSE_BYTES {
            bail!("响应体过大 ({} bytes)，上限 {} bytes", resp_bytes.len(), MAX_RESPONSE_BYTES);
        }
        let resp_text = String::from_utf8_lossy(&resp_bytes).to_string();

        let resp_body: serde_json::Value = serde_json::from_str(&resp_text)
            .unwrap_or(serde_json::Value::String(resp_text));

        Ok(NodeResult::Success(serde_json::json!({
            "status": status,
            "body": resp_body,
        })))
    }
}

// ── RunLoop (子工作流) ───────────────────────────────────────────────

pub struct RunLoopExec;

#[async_trait]
impl NodeExecutor for RunLoopExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        // 子工作流执行由引擎层 execute_sub_workflow 直接处理（绕过此 executor）
        // 此处仅作 fallback 标记
        let workflow_id = node.config.get("workflow_id").and_then(|v| v.as_str()).unwrap_or("");
        Ok(NodeResult::Success(serde_json::json!({ "sub_workflow": workflow_id })))
    }
}

// ── DelayWait ────────────────────────────────────────────────────────

pub struct DelayWaitExec;

#[async_trait]
impl NodeExecutor for DelayWaitExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let seconds = node.config.get("seconds").and_then(|v| v.as_u64()).unwrap_or(1).min(MAX_DELAY_SECONDS);
        tokio::time::sleep(std::time::Duration::from_secs(seconds)).await;
        Ok(NodeResult::Success(serde_json::json!({
            "waited_seconds": seconds,
        })))
    }
}

// ── Output ───────────────────────────────────────────────────────────

pub struct OutputExec;

#[async_trait]
impl NodeExecutor for OutputExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        // 收集 fields 中定义的输出字段
        let fields = node.config.get("fields").and_then(|v| v.as_array());
        let mut out = serde_json::Map::new();
        if let Some(fields) = fields {
            for f in fields {
                let name = f.get("name").and_then(|v| v.as_str()).unwrap_or_default();
                if let Some(val) = ctx.resolve(name) {
                    out.insert(name.to_string(), val);
                }
            }
        }
        if out.is_empty() {
            // 无显式字段定义时，返回 output 标记
            out.insert("completed".into(), serde_json::Value::Bool(true));
        }
        Ok(NodeResult::Success(serde_json::Value::Object(out)))
    }
}

// ── AudioProcessing ──────────────────────────────────────────────────

pub struct AudioProcessingExec;

#[async_trait]
impl NodeExecutor for AudioProcessingExec {
    async fn execute(&self, node: &WorkflowNode, _ctx: &VariableContext) -> Result<NodeResult> {
        let op = node.config.get("operation").and_then(|v| v.as_str()).unwrap_or("convert");
        bail!("音频处理功能正在开发中 — 待接入 ffmpeg/rodio 引擎。操作: {}", op)
    }
}

// ── Send Notification ───────────────────────────────────────────────

pub struct SendNotificationExec;

#[async_trait]
impl NodeExecutor for SendNotificationExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let channel = node.config.get("channel").and_then(|v| v.as_str()).unwrap_or("system");
        let title_tpl = node.config.get("title_template").and_then(|v| v.as_str()).unwrap_or("");
        let body_tpl = node.config.get("body_template").and_then(|v| v.as_str()).unwrap_or("");
        let title = ctx.interpolate(title_tpl);
        let body = ctx.interpolate(body_tpl);
        let recipient = node.config.get("recipient").and_then(|v| v.as_str()).unwrap_or("");

        Ok(NodeResult::Success(serde_json::json!({
            "type": "send_notification",
            "channel": channel,
            "title": title,
            "body": body,
            "recipient": recipient,
            "note": "通知发送待接入系统通知/邮件/Webhook 推送"
        })))
    }
}

// ── File I/O ────────────────────────────────────────────────────────

pub struct FileIoExec;

#[async_trait]
impl NodeExecutor for FileIoExec {
    async fn execute(&self, node: &WorkflowNode, ctx: &VariableContext) -> Result<NodeResult> {
        let op = node.config.get("operation").and_then(|v| v.as_str()).unwrap_or("read");
        let path = node.config.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let path = ctx.interpolate(path);

        if path.trim().is_empty() {
            bail!("文件读写节点的路径为空");
        }
        let safe_path = validate_file_path(&path)?;

        match op {
            "read" => {
                let content = std::fs::read_to_string(&safe_path)
                    .map_err(|e| anyhow::anyhow!("读取文件失败 {}: {}", path, e))?;
                Ok(NodeResult::Success(serde_json::json!({
                    "operation": "read",
                    "path": path,
                    "content": content,
                })))
            }
            "write" | "append" => {
                let content_tpl = node.config.get("content_template").and_then(|v| v.as_str()).unwrap_or("");
                let content = ctx.interpolate(content_tpl);
                if op == "append" {
                    use std::io::Write;
                    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&safe_path)
                        .map_err(|e| anyhow::anyhow!("打开文件失败 {}: {}", path, e))?;
                    f.write_all(content.as_bytes())?;
                } else {
                    std::fs::write(&safe_path, &content)
                        .map_err(|e| anyhow::anyhow!("写入文件失败 {}: {}", path, e))?;
                }
                Ok(NodeResult::Success(serde_json::json!({
                    "operation": op,
                    "path": path,
                    "bytes_written": content.len(),
                })))
            }
            "copy" => {
                let dest = node.config.get("dest_path").and_then(|v| v.as_str()).unwrap_or("");
                let dest = ctx.interpolate(dest);
                let safe_dest = validate_file_path(&dest)?;
                std::fs::copy(&safe_path, &safe_dest)
                    .map_err(|e| anyhow::anyhow!("复制失败 {} → {}: {}", path, dest, e))?;
                Ok(NodeResult::Success(serde_json::json!({
                    "operation": "copy", "source": path, "dest": dest,
                })))
            }
            "move" => {
                let dest = node.config.get("dest_path").and_then(|v| v.as_str()).unwrap_or("");
                let dest = ctx.interpolate(dest);
                let safe_dest = validate_file_path(&dest)?;
                std::fs::rename(&safe_path, &safe_dest)
                    .map_err(|e| anyhow::anyhow!("移动失败 {} → {}: {}", path, dest, e))?;
                Ok(NodeResult::Success(serde_json::json!({
                    "operation": "move", "source": path, "dest": dest,
                })))
            }
            "delete" => {
                std::fs::remove_file(&safe_path)
                    .map_err(|e| anyhow::anyhow!("删除失败 {}: {}", path, e))?;
                Ok(NodeResult::Success(serde_json::json!({
                    "operation": "delete", "path": path,
                })))
            }
            "list" => {
                let entries: Vec<String> = std::fs::read_dir(&safe_path)
                    .map_err(|e| anyhow::anyhow!("读取目录失败 {}: {}", path, e))?
                    .filter_map(|e| e.ok().map(|e| e.file_name().to_string_lossy().to_string()))
                    .collect();
                Ok(NodeResult::Success(serde_json::json!({
                    "operation": "list", "path": path, "entries": entries,
                })))
            }
            _ => bail!("未知文件操作: {}", op),
        }
    }
}
