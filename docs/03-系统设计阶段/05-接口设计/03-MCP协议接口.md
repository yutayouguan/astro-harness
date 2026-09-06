# MCP 协议接口设计文档

> 阶段：系统设计 | 状态：当前 Client 契约 + Server 目标设计 | 说明：JSON-RPC 2.0、连接生命周期

**协议版本**：2024-11-05  
**传输层**：JSON-RPC 2.0 over STDIO / Streamable HTTP  
**项目角色**：Astro 当前只实现 MCP 客户端；下文 MCP Server 工具表是未实现的目标设计

---

## 1. MCP 消息格式规范

所有消息均为 UTF-8 编码的 JSON，每条消息以换行符 `\n` 分隔（stdio 模式下按行读取）。

**Request**（客户端发出，需要响应）：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "tools/call",
  "params": { ... }
}
```

**Response**（服务端回复，id 与请求对应）：

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": { ... }
}
```

错误时 `result` 替换为 `error: { "code": -32600, "message": "...", "data": {} }`。

**Notification**（单向推送，无 id，无需回复）：

```json
{
  "jsonrpc": "2.0",
  "method": "notifications/tools/list_changed",
  "params": {}
}
```

---

## 2. 客户端侧实现的方法列表

Astro Agent 作为 MCP 客户端时，主动发起以下请求：

| 方法 | 说明 | 关键参数 |
| --- | --- | --- |
| `initialize` | 握手，协商协议版本与能力集 | `protocolVersion`, `clientInfo`, `capabilities` |
| `tools/list` | 获取服务端注册的工具列表 | `cursor`（可选，分页） |
| `tools/call` | 调用指定工具 | `name`, `arguments` |
| `resources/list` | 枚举服务端暴露的资源 | `cursor`（可选） |
| `resources/read` | 读取单个资源内容 | `uri`（资源唯一标识） |
| `prompts/list` | 获取提示模板列表 | 无 |
| `sampling/createMessage` | 请求服务端执行 LLM 采样 | `messages`, `modelPreferences`, `maxTokens` |

`initialize` 完成后，客户端必须发送 `notifications/initialized` 通知，握手才算完成。

---

## 3. 服务端侧目标工具列表（未实现）

Astro Agent 作为 MCP Server 时，向宿主（如 Claude Desktop）注册以下工具：

### 3.1 记忆管理工具

| 工具名 | 描述 | 输入 Schema 摘要 |
| --- | --- | --- |
| `astro.memory.search` | KNN 向量检索本地记忆库（仅返回 `valid_to IS NULL` 活跃版本） | `query: string`, `top_k?: int` |
| `astro.memory.store` | 写入新记忆条目；若同 category+key 已有活跃版本则触发软替换 | `content: string`, `category?: string`, `key?: string` |
| `astro.memory.update` | 更新已有记忆（软替换：旧版本设 `valid_to`，新条目设 `superseded_by`） | `key: string`, `new_value: string`, `category?: string` |
| `astro.memory.forget` | 软删除记忆（设置 `valid_to = now()`，不物理删除，历史可追溯） | `key: string`, `category?: string` |

### 3.2 文件与系统工具

| 工具名 | 描述 | 输入 Schema 摘要 |
| --- | --- | --- |
| `astro.file.read` | 读取本地文件内容 | `path: string`, `encoding?: string` |
| `astro.file.write` | 写入或追加文件 | `path: string`, `content: string`, `append?: bool` |
| `astro.shell.exec` | 在沙箱中执行 Shell 命令 | `command: string`, `timeout_ms?: int` |
| `astro.browser.screenshot` | 截取当前页面截图 | `url?: string` |
| `astro.knowledge.query` | 查询结构化知识图谱 | `sparql?: string`, `natural_query?: string` |

### 3.3 多媒体生成工具

| 工具名 | 描述 | 输入 Schema 摘要 |
| --- | --- | --- |
| `astro.media.tts` | 文字转语音（流式合成，返回音频 base64 或本地路径） | `text: string`, `voice_id?: string`, `model?: string` |
| `astro.media.generate_image` | 文生图 / 图生图，返回本地缓存路径列表 | `prompt: string`, `model?: string`, `aspect_ratio?: string`, `reference_url?: string` |
| `astro.media.submit_video` | 异步提交视频生成任务，立即返回 `task_id` | `prompt: string`, `model?: string`, `aspect_ratio?: string` |
| `astro.media.generate_music` | 音乐生成（可附歌词，支持翻唱） | `prompt: string`, `lyrics?: string`, `reference_audio_url?: string`, `model?: string` |
| `astro.media.get_task` | 查询媒体任务状态与进度 | `task_id: string` |

每个工具的 `inputSchema` 遵循 JSON Schema Draft 7，服务端在 `tools/list` 响应中完整返回。

---

## 4. 错误码规范

**JSON-RPC 标准码**：

| 码值 | 含义 |
| --- | --- |
| -32700 | Parse error，消息不是合法 JSON |
| -32600 | Invalid Request，结构不符合规范 |
| -32601 | Method not found |
| -32602 | Invalid params |
| -32603 | Internal error |

**Astro 自定义码**（范围 -32000 ~ -32099）：

| 码值 | 含义 |
| --- | --- |
| -32000 | 工具执行超时 |
| -32001 | 沙箱权限拒绝 |
| -32002 | 记忆库容量超限 |
| -32003 | 资源 URI 不存在或无法访问 |
| -32004 | 协议版本不兼容 |
| -32005 | 采样请求被模型拒绝（safety filter） |
| -32006 | 记忆版本冲突（key 在活跃版本中已被并发更新） |
| -32007 | 媒体任务不存在或已取消 |
| -32008 | 媒体生成轮询超时（轮询上限已达） |

---

## 5. 连接生命周期

```text
Client                          Server
  │──── initialize ────────────►│  携带 protocolVersion + capabilities
  │◄─── initialize result ──────│  返回服务端能力集
  │──── notifications/initialized ►│  握手完成信号
  │
  │──── tools/list ────────────►│
  │◄─── tools/list result ──────│  工具列表（含 schema）
  │
  │──── tools/call ────────────►│  调用工具
  │◄─── tools/call result ──────│  执行结果或 error
  │
  │  [Server 侧工具列表变更时]
  │◄─── notifications/tools/list_changed ──│  客户端应重新 tools/list
  │
  │  [断连处理]
  │  stdio EOF / HTTP 连接关闭 → 两端清理资源
  │  客户端应实现指数退避重连，最大间隔 30s
```

断连后，所有进行中的请求应以 `-32603` 错误通知调用方；重连成功后需重新执行 `initialize` 握手，不可复用旧 session。

---

## 6. Rust rmcp Crate 集成方式

项目使用 [`rmcp`](https://crates.io/crates/rmcp) crate 实现客户端。下面的 ServerHandler 示例
仅是目标设计，不对应当前 workspace crate：

```toml
[dependencies]
rmcp = { version = "0.1", features = ["server", "client", "transport-io"] }
tokio = { version = "1", features = ["full"] }
```

**作为服务端**，实现 `ServerHandler` trait，在 `list_tools` / `call_tool` 方法中注册上述工具；通过 `serve_server(handler, stdio())` 启动。

**作为客户端**，使用 `ClientBuilder::new(stdio_transport).build().await`，随后调用 `client.initialize()` 完成握手，再通过 `client.call_tool(name, args)` 发起调用。

异步运行时统一使用 Tokio，所有 IO 操作不阻塞主线程。

---

## 7. stdio 与 Streamable HTTP 两种传输模式的差异处理

| 维度 | stdio | Streamable HTTP |
| --- | --- | --- |
| 连接方式 | 子进程 stdin/stdout | 单一 `/mcp` endpoint 上的 HTTP POST/GET |
| 消息分隔 | `\n` 换行 | HTTP 请求/响应体，由 Streamable HTTP transport 处理 |
| 连接管理 | 进程生命周期即连接生命周期 | Session/transport 生命周期，支持显式重连 |
| 并发 | 单连接，消息顺序严格 | 支持多客户端并发连接 |
| 适用场景 | 本地 MCP Server（CLI、桌面插件） | 远程 MCP Server、Web 宿主环境 |
| 认证 | 不需要 | Bearer Token / OAuth |
| Rust 实现 | `rmcp::transport::stdio()` | `rmcp::transport::streamable_http()` + Axum |

stdio 模式下，Astro 作为客户端启动外部 MCP Server 子进程并通过管道通信；Streamable
HTTP 模式下，Astro 作为客户端连接 Server 明确提供的单一 `/mcp` endpoint。Astro 当前不
监听 MCP 端口，也不自动把内部工具暴露给第三方宿主。

`McpEventStreamManager` 已定义独立于 turn task 的订阅所有权、active 握手、attempt id、
有界队列以及权限/Server 取消边界；具体 Server opener 和 Desktop 订阅 RPC 尚未接线。

> **注**：旧 `/sse` + `/message` 双端点传输不受支持。历史配置必须迁移到 Server 明确提供的 Streamable HTTP `/mcp` endpoint，禁止自动转换 URL。
