# 本地 HTTP API 设计

> v0.3.1 规划 | 状态：规划中 | 覆盖：HTTP 端点、Webhook 回调、API Key 鉴权

---

## 1. 概述

Astro Agent 监听本地端口（默认 `127.0.0.1:9527`），接受外部 HTTP 请求触发任务。

**使用场景**：

- **CI/CD 集成**：构建完成后通知 Agent 执行部署 Skill
- **脚本自动化**：Shell 脚本通过 `curl` 调用 Agent 能力
- **IDE 插件通信**：VSCode / JetBrains 插件通过 HTTP 与 Agent 交互
- **外部集成**：Telegram / Slack 渠道适配器通过本地 API 与 AgentCore 通信

**设计原则**：

- 仅监听 `127.0.0.1`，不暴露到外网
- 所有请求必须携带 API Key
- 与桌面端共享 AppState，不启动独立进程

---

## 2. 鉴权

### 2.1 API Key 生成

首次启动时自动生成 32 字节随机 API Key：

```rust
use rand::Rng;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

fn generate_api_key() -> String {
    let mut rng = rand::thread_rng();
    let bytes: [u8; 32] = rng.gen();
    URL_SAFE_NO_PAD.encode(bytes)
}
```

存储位置：`~/.astro/api_key`，文件权限 `600`。

### 2.2 请求鉴权

所有请求必须在 Header 中携带 API Key：

```
Authorization: Bearer <api_key>
```

鉴权中间件：

```rust
async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let auth_header = request.headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(header) if header.starts_with("Bearer ") => {
            let token = &header[7..];
            if token == state.api_key {
                next.run(request).await
            } else {
                StatusCode::UNAUTHORIZED.into_response()
            }
        }
        _ => StatusCode::UNAUTHORIZED.into_response(),
    }
}
```

### 2.3 Key 重置

```bash
# 通过命令行重新生成 API Key
astro api reset-key
# 输出新 Key，旧 Key 立即失效
```

设置页也提供"重新生成 API Key"按钮。

---

## 3. API 端点

### 3.1 健康检查

```
GET /v1/health
```

响应：

```json
{
  "status": "ok",
  "version": "0.3.1",
  "uptime_seconds": 3600
}
```

无需鉴权（唯一例外），用于监控和连通性检测。

### 3.2 发送消息（流式）

```
POST /v1/chat
Content-Type: application/json
Authorization: Bearer <key>
```

请求体：

```json
{
  "message": "帮我检查一下最近的 Git 提交",
  "conversation_id": "conv_abc123",
  "stream": true
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `message` | string | 是 | 用户消息 |
| `conversation_id` | string | 否 | 对话 ID，不填则创建新对话 |
| `stream` | bool | 否 | 默认 true，SSE 流式响应 |

**流式响应**（SSE）：

```
HTTP/1.1 200 OK
Content-Type: text/event-stream

event: message_start
data: {"conversation_id": "conv_abc123"}

event: content_delta
data: {"text": "让我检查"}

event: content_delta
data: {"text": "最近的 Git 提交..."}

event: tool_use
data: {"tool": "shell_execute", "input": "git log --oneline -5"}

event: tool_result
data: {"output": "abc1234 fix: typo\ndef5678 feat: add login"}

event: message_end
data: {"finish_reason": "end_turn"}
```

**非流式响应**（`stream: false`）：

```json
{
  "conversation_id": "conv_abc123",
  "response": "最近 5 条 Git 提交如下：...",
  "finish_reason": "end_turn"
}
```

### 3.3 执行 Skill

```
POST /v1/skill/run
Content-Type: application/json
Authorization: Bearer <key>
```

请求体：

```json
{
  "skill_name": "deploy-k8s",
  "parameters": {
    "env": "staging",
    "tag": "v1.2.0"
  },
  "async": true
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `skill_name` | string | 是 | Skill 名称 |
| `parameters` | object | 否 | Skill 参数 |
| `async` | bool | 否 | 默认 false。true 则立即返回 task_id |

同步响应：

```json
{
  "task_id": "task_xyz789",
  "status": "completed",
  "result": "部署完成，staging 环境已更新到 v1.2.0"
}
```

异步响应（`async: true`）：

```json
{
  "task_id": "task_xyz789",
  "status": "running"
}
```

异步任务可通过 `GET /v1/tasks/{task_id}` 查询状态。

### 3.4 列出对话

```
GET /v1/conversations?limit=20&offset=0
Authorization: Bearer <key>
```

响应：

```json
{
  "conversations": [
    {
      "id": "conv_abc123",
      "title": "Git 提交检查",
      "created_at": "2026-08-13T10:00:00Z",
      "message_count": 5,
      "source_channel": "api"
    }
  ],
  "total": 42
}
```

### 3.5 查询任务状态

```
GET /v1/tasks/{task_id}
Authorization: Bearer <key>
```

响应：

```json
{
  "task_id": "task_xyz789",
  "status": "completed",
  "skill_name": "deploy-k8s",
  "started_at": "2026-08-13T10:05:00Z",
  "completed_at": "2026-08-13T10:05:30Z",
  "result": "部署完成"
}
```

`status` 枚举值：`pending` | `running` | `completed` | `failed` | `cancelled`

---

## 4. Webhook 回调

### 4.1 概述

任务完成后 Agent 主动 POST 结果到用户配置的 URL，适用于 CI 集成等需要被动接收结果的场景。

### 4.2 配置

`~/.astro/webhooks.json`：

```json
{
  "webhooks": [
    {
      "name": "ci-notify",
      "url": "http://localhost:8080/astro-callback",
      "events": ["task_complete", "task_failed"],
      "secret": "webhook_secret_abc123",
      "enabled": true
    },
    {
      "name": "slack-alert",
      "url": "https://hooks.slack.com/services/T.../B.../xxx",
      "events": ["task_failed"],
      "secret": "",
      "enabled": true
    }
  ]
}
```

### 4.3 Webhook 请求格式

```
POST {webhook_url}
Content-Type: application/json
X-Astro-Signature: sha256=<hmac_hex>
X-Astro-Event: task_complete
```

请求体：

```json
{
  "event": "task_complete",
  "task_id": "task_xyz789",
  "skill_name": "deploy-k8s",
  "status": "completed",
  "result": "部署完成",
  "timestamp": "2026-08-13T10:05:30Z"
}
```

### 4.4 HMAC 签名

使用 `secret` 对请求体计算 HMAC-SHA256 签名：

```rust
use hmac::{Hmac, Mac};
use sha2::Sha256;

fn sign_payload(secret: &str, payload: &[u8]) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(payload);
    let result = mac.finalize();
    format!("sha256={}", hex::encode(result.into_bytes()))
}
```

接收方验证签名以确保请求来自 Astro Agent。`secret` 为空时不发送签名头。

### 4.5 重试策略

- 发送失败（非 2xx 响应或超时）自动重试
- 最多重试 3 次，间隔 5s / 15s / 60s
- 3 次失败后记录错误日志，不再重试
- 单次请求超时 10 秒

---

## 5. 速率限制

### 5.1 限制规则

| 端点 | 限制 | 说明 |
|------|------|------|
| `/v1/chat` | 30 req/min | 对话请求较重 |
| `/v1/skill/run` | 20 req/min | Skill 执行较重 |
| `/v1/conversations` | 60 req/min | 轻量查询 |
| `/v1/health` | 不限制 | 健康检查 |
| 全局 | 60 req/min | 所有端点合计 |

### 5.2 超限响应

```
HTTP/1.1 429 Too Many Requests
Retry-After: 30

{
  "error": "rate_limit_exceeded",
  "message": "请求过于频繁，请在 30 秒后重试",
  "retry_after": 30
}
```

### 5.3 实现

使用 `tower` 中间件的 `RateLimitLayer`，基于令牌桶算法：

```rust
use tower::limit::RateLimitLayer;
use std::time::Duration;

let rate_limit = RateLimitLayer::new(60, Duration::from_secs(60));
```

---

## 6. 实现

### 6.1 技术栈

- HTTP 框架：`axum`（已在 Tauri 后端使用，无额外依赖）
- 内嵌到 Tauri 进程，与桌面端共享 `AppState`
- 独立 `tokio::spawn` 启动 HTTP server

### 6.2 启动流程

```rust
pub async fn start_api_server(state: AppState) -> Result<()> {
    if !state.config.api_enabled {
        return Ok(());  // 未启用则不启动
    }

    let app = Router::new()
        .route("/v1/health", get(health_handler))
        .route("/v1/chat", post(chat_handler))
        .route("/v1/skill/run", post(skill_run_handler))
        .route("/v1/conversations", get(conversations_handler))
        .route("/v1/tasks/:task_id", get(task_status_handler))
        .layer(auth_middleware)
        .layer(rate_limit_layer)
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 9527));
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await?;
    Ok(())
}
```

### 6.3 设置页集成

设置 → 高级 → 本地 API：

- 开关："启用本地 API"（默认关闭）
- 显示当前 API Key（可复制、可重新生成）
- 端口配置（默认 9527）
- Webhook 管理（增删改）

---

## 7. MVP 范围

| 功能 | MVP | 后续 |
|------|-----|------|
| `/v1/health` | ✅ | - |
| `/v1/chat`（流式） | ✅ | 多模态输入 |
| `/v1/skill/run`（同步） | ✅ | 异步 + 进度推送 |
| `/v1/conversations` | ✅ | 分页/搜索 |
| API Key 鉴权 | ✅ | - |
| Webhook 回调 | ✅ | 自定义事件过滤 |
| 速率限制 | ✅ | 可配置限额 |
| 设置页 UI | ✅ | - |

---

## 8. 风险与约束

- **端口冲突**：9527 被占用时尝试 9528-9537，均失败则报错提示用户配置
- **安全**：仅绑定 `127.0.0.1`，外部网络无法访问；API Key 防止本机其他进程越权
- **性能**：本地 API 吞吐不是瓶颈，Agent 的 LLM 调用才是瓶颈
- **状态同步**：API 和桌面 UI 共享 AppState，对话列表实时同步
