# Tauri Commands API 文档

> 阶段：系统设计 | 状态：定稿 | 说明：58 个 Commands + 7 种流式事件

> **当前 Harness 基线（2026-08-29）**：命令数量与旧事件枚举是历史快照。当前对话主链以 Thread `submit/resume/subscribe`、`agent-protocol::Op`、`EventMsg` 和 rollout + live boundary 为准；Tauri command 是桌面适配层，不是 Agent Harness 的事实源。

## 错误返回规范

所有 Command 统一返回 `Result<T, AppError>`，前端收到的错误结构如下：

```rust
/// Rust 侧定义（apps/desktop/src-tauri/src/error.rs）
#[derive(Debug, thiserror::Error, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    #[error("database error: {0}")]
    Db(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("tool error: {0}")]
    Tool(String),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("not found: {0}")]
    NotFound(String),
}
```

```typescript
// 前端 TypeScript 类型
interface AppError {
  kind: "Db" | "Provider" | "Tool" | "Validation" | "NotFound";
  message: string;
}
```

Rust 侧使用 `thiserror` 派生宏，所有 `#[tauri::command]` 函数签名返回 `Result<T, AppError>`，序列化为 `{ kind, message }`。

---

## 流式响应 Event 设计

流式对话通过 Tauri Event 推送，前端监听如下事件：

```typescript
// Token 流
interface TokenChunkEvent {
  conversation_id: string;
  chunk: string;             // 增量文本片段
  thinking_chunk?: string;   // thinking 模式推理内容增量（DeepSeek / MiniMax / Claude / Gemini）
  finish_reason?: "stop" | "length" | "tool_calls";
}

// 进度事件（文件导出、记忆索引等耗时操作）
interface ProgressEvent {
  task_id: string;
  current: number;
  total: number;
  label: string;
}

// 工具调用事件
interface ToolCallEvent {
  conversation_id: string;
  tool_name: string;
  status: "running" | "done" | "error";
  result?: string;
}

// 流式错误事件（Provider 断连、token 超限等中途失败）
interface StreamErrorEvent {
  conversation_id: string;
  error_code: string;   // "PROVIDER_DISCONNECTED" | "TOKEN_LIMIT" | "TIMEOUT" | "USER_REJECTED"
  message: string;
  recoverable: boolean; // true=可继续（如切换 Provider），false=本次对话终止
}

// 人工审批请求事件（HumanGuard 触发）
interface ApprovalRequestEvent {
  call_id: string;                         // 待审批的工具调用 ID
  tool_name: string;
  risk_level: "Low" | "Medium" | "High" | "Critical";
  params_preview: Record<string, unknown>; // 参数预览（脱敏）
  reason: string;                          // 触发审批的原因描述
  timeout_ms: number;                      // 超时后自动拒绝
  expires_at: number;                      // Unix 毫秒时间戳
}

// TTS 音频流事件
interface TtsChunkEvent {
  task_id: string;
  data: number[];    // mp3/PCM 音频字节（前端转 Uint8Array 写入 AudioWorklet）
  is_last: boolean;
}

// 多媒体生成任务事件（图像/视频/音乐异步轮询结果）
interface MediaTaskEvent {
  task_id: string;
  media_type: "image" | "video" | "music";
  status: "pending" | "processing" | "done" | "failed";
  progress?: number;     // 0.0–1.0，processing 阶段可用
  file_paths?: string[]; // done 时本地缓存路径列表（已下载）
  error?: string;
}
```

事件频道命名规则：`snake_case` 扁平命名

| 事件频道 | 类型 | 触发时机 |
| ------- | ---- | ------- |
| `token_chunk` | TokenChunkEvent | LLM 流式输出每个 token / thinking chunk |
| `thinking_delta` | TokenChunkEvent (thinking_chunk) | thinking 模式推理内容增量 |
| `tool_call` | ToolCallEvent | 工具调用状态变化 |
| `stream_error` | StreamErrorEvent | 流式推理中途失败 |
| `approval_request` | ApprovalRequestEvent | HumanGuard 触发审批 |
| `tts_chunk` | TtsChunkEvent | TTS 流式音频块推送 |
| `media_task_update` | MediaTaskEvent | 图像/视频/音乐生成进度与完成通知 |
| `message_done` | — | 本轮消息生成结束 |
| `task_progress` | ProgressEvent | 耗时操作进度更新（导出/重建索引等） |

---

## Command 列表

### 对话类（Chat）

| # | Command | 功能说明 |
| --- | ------- | ------- |
| 1 | `create_conversation` | 新建对话，返回 ConversationId |
| 2 | `list_conversations` | 分页列出所有对话 |
| 3 | `get_conversation` | 按 ID 获取对话详情含消息列表 |
| 4 | `delete_conversation` | 删除对话及其全部消息 |
| 5 | `send_message` | 发送用户消息，触发流式响应 |
| 6 | `cancel_stream` | 取消正在进行的流式生成 |
| 7 | `regenerate_message` | 重新生成指定助手消息 |
| 8 | `fork_conversation` | 从某条消息处分叉出新对话 |

```typescript
// Rust 侧签名示例
// #[tauri::command] async fn send_message(payload: SendMessagePayload, window: Window) -> Result<MessageId, AppError>

interface SendMessagePayload {
  conversation_id: string;
  content: string;
  attachments?: Attachment[];  // 图片/文件路径
  model_override?: string;
}
type MessageId = string;

// 调用示例
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

const msgId = await invoke<string>("send_message", { payload });
const unlisten = await listen<TokenChunkEvent>("token_chunk", (e) => {
  if (e.payload.conversation_id === currentId) appendChunk(e.payload.chunk);
});
```

---

### 工作区类（Workspace）

| # | Command | 功能说明 |
| --- | ------- | ------- |
| 9 | `create_workspace` | 新建工作区 |
| 10 | `list_workspaces` | 列出全部工作区 |
| 11 | `get_workspace` | 获取工作区详情 |
| 12 | `update_workspace` | 更新名称、描述、设置 |
| 13 | `delete_workspace` | 删除工作区及关联数据 |
| 14 | `switch_workspace` | 切换当前活跃工作区 |
| 15 | `list_workspace_files` | 列出工作区目录树 |

```typescript
interface CreateWorkspacePayload { name: string; icon: string; }
interface Workspace { id: string; name: string; icon: string; created_at: number; }

const ws = await invoke<Workspace>("create_workspace", {
  payload: { name: "My Project", icon: "💻" }
});
```

---

### 记忆类（Memory）

| # | Command | 功能说明 |
| --- | ------- | ------- |
| 16 | `add_memory` | 手动插入一条记忆条目 |
| 17 | `search_memories` | 语义搜索记忆（返回 TopK） |
| 18 | `list_memories` | 按标签/时间分页列出记忆 |
| 19 | `delete_memory` | 删除指定记忆条目 |
| 20 | `reindex_memories` | 触发全量向量重建（异步，发送 progress 事件） |

```typescript
interface Memory { id: string; content: string; tags: string[]; embedding_score?: number; }
interface SearchMemoriesPayload { query: string; top_k: number; workspace_id?: string; }

const results = await invoke<Memory[]>("search_memories", {
  payload: { query: "React hooks pattern", top_k: 5 }
});
```

---

### Skills 类（Skills）

| # | Command | 功能说明 |
| --- | ------- | ------- |
| 21 | `list_skills` | 列出所有 Skill（含内置与自定义） |
| 22 | `get_skill` | 获取 Skill 详情（提示词、元数据） |
| 23 | `create_skill` | 创建自定义 Skill |
| 24 | `update_skill` | 编辑 Skill 内容或元数据 |
| 25 | `delete_skill` | 删除自定义 Skill |
| 26 | `invoke_skill` | 在对话中调用指定 Skill |

```typescript
interface Skill { id: string; name: string; description: string; prompt_template: string; is_builtin: boolean; }
const skill = await invoke<Skill>("create_skill", {
  payload: { name: "Code Review", description: "...", prompt_template: "Review: {{input}}" }
});
```

---

### 配置类（Config）

| # | Command | Rust 签名简述 | 功能说明 |
| --- | ------- | ------------ | ------- |
| 27 | `list_providers` | `() -> Vec<Provider>` | 列出所有 LLM Provider |
| 28 | `upsert_provider` | `(UpsertProviderPayload) -> Provider` | 新增或更新 Provider（含 API Key） |
| 29 | `delete_provider` | `(provider_id: String) -> ()` | 删除 Provider |
| 30 | `list_mcp_servers` | `() -> Vec<McpServer>` | 列出 MCP 服务器 |
| 31 | `upsert_mcp_server` | `(McpServerPayload) -> McpServer` | 新增或更新 MCP 服务器 |
| 32 | `test_mcp_connection` | `(server_id: String) -> ConnectionStatus` | 测试 MCP 服务器连通性 |
| 33 | `get_app_settings` | `() -> AppSettings` | 读取全局应用设置 |
| 34 | `update_app_settings` | `(AppSettings) -> ()` | 保存全局设置 |
| 35 | `test_provider` | `(provider_id: String) -> ConnectionStatus` | 发送最小测试请求验证 API Key 有效性 |

```typescript
interface UpsertProviderPayload {
  id?: string; name: string; base_url: string;
  api_key: string;  // Rust 侧写入系统 Keychain（keyring crate），Keychain 不可用时降级写入 secrets/ 加密文件，不落 SQLite 明文
  models: string[];
}
await invoke("upsert_provider", { payload: { name: "Anthropic", base_url: "https://api.anthropic.com", api_key: "sk-...", models: ["claude-sonnet-4-6"] } });
// 写入后可立即测试连通性
const status = await invoke<ConnectionStatus>("test_provider", { providerId: "anthropic" });
```

---

### 人工接管类（HumanGuard）

| # | Command | 功能说明 |
| --- | ------- | ------- |
| 36 | `list_pending_approvals` | 查询当前待审批的工具调用队列 |
| 37 | `approve_tool_call` | 批准指定工具调用，Agent 继续执行 |
| 38 | `reject_tool_call` | 拒绝指定工具调用，Agent 收到 UserRejected 错误 |
| 39 | `pause_agent` | 主动暂停 Agent，进入 L2 暂停等待状态 |
| 40 | `resume_agent` | 恢复已暂停的 Agent |
| 41 | `takeover_agent` | 进入 L3 完全接管模式，Agent 停止自主执行 |
| 42 | `release_control` | 退出接管模式，Agent 恢复自主运行 |

```typescript
interface PendingApproval {
  call_id: string;
  tool_name: string;
  risk_level: "Low" | "Medium" | "High" | "Critical";
  params_preview: Record<string, unknown>;
  reason: string;
  expires_at: number;
}

// 查询待审批队列
const queue = await invoke<PendingApproval[]>("list_pending_approvals");

// 批准
await invoke("approve_tool_call", { payload: { call_id: "..." } });

// 拒绝（附原因，Agent 会收到拒绝原因并可调整策略）
await invoke("reject_tool_call", { payload: { call_id: "...", reason: "路径超出工作区范围" } });

// 前端监听审批请求（HumanGuard 触发时）
await listen<ApprovalRequestEvent>("approval_request", (e) => {
  showApprovalDialog(e.payload);
});
```

---

### 统计与导出类（Stats & Export）

| # | Command | 功能说明 |
| --- | ------- | ------- |
| 43 | `get_cost_summary` | 按时间段汇总 Token 用量与费用（含多媒体生成计费） |
| 44 | `get_conversation_stats` | 单对话的 Token / 耗时统计 |
| 45 | `export_conversation` | 导出对话为 Markdown / PDF / JSON（异步，发 progress 事件） |
| 46 | `export_workspace` | 打包导出工作区全部数据 |
| 47 | `save_ai_artifact` | 将消息中 AI 产物归档到 ai-docs/ 指定子目录，写入 ai_artifacts 表 |
| 48 | `share_conversation` | 生成分享包（HTML 单文件或 ZIP），通过 tauri-plugin-share 唤起系统分享框 |

```typescript
interface CostSummary { total_tokens: number; total_cost_usd: number; by_model: Record<string, number>; }
const summary = await invoke<CostSummary>("get_cost_summary", { payload: { from: "2026-07-01", to: "2026-08-05" } });

// 导出（异步进度）
const taskId = await invoke<string>("export_conversation", { payload: { conversation_id: "...", format: "markdown" } });
await listen<ProgressEvent>("task_progress", (e) => {
  if (e.payload.task_id === taskId) updateProgress(e.payload.current / e.payload.total);
});

// AI 产物归档
await invoke("save_ai_artifact", { payload: {
  conversation_id: "...", message_id: "...",
  filename: "2026-08-06_analysis.md", target_subdir: "reports"
}});

// 分享对话
await invoke("share_conversation", { payload: {
  conversation_id: "...", message_ids: ["id1", "id2"], include_tools: false
}});
```

---

### 多媒体类（Media）

| # | Command | 功能说明 |
| --- | ------- | ------- |
| 49 | `text_to_speech` | TTS 语音合成（流式），返回 task_id；音频块通过 `tts_chunk` 推送 |
| 50 | `transcribe_audio` | ASR 语音识别，同步返回转录文本及时间戳片段 |
| 51 | `generate_image` | 图像生成（文生图 / 图生图），同步返回本地缓存路径列表 |
| 52 | `generate_video` | 视频生成（异步轮询），返回 task_id；进度通过 `media_task_update` 推送 |
| 53 | `generate_music` | 音乐生成（文本 + 歌词），返回 task_id；支持 MiniMax 流式和 Google Lyria 3 |
| 54 | `generate_lyrics` | 歌词生成（MiniMax 专属），同步返回 song_title / style_tags / lyrics |
| 55 | `music_cover` | 翻唱生成（MiniMax 专属），需参考音频 URL 或 cover_feature_id |
| 56 | `get_media_task` | 查询异步媒体任务当前状态（pending / processing / done / failed） |
| 57 | `cancel_media_task` | 取消进行中的异步媒体任务 |
| 58 | `design_voice` | 音色设计（MiniMax 专属），返回 voice_id 供 TTS 使用 |

```typescript
// TTS（流式）
const taskId = await invoke<string>("text_to_speech", {
  payload: { text: "Hello world", voice_id: "male-qn-qingse", model: "speech-2.8-hd" }
});
const unlisten = await listen<TtsChunkEvent>("tts_chunk", (e) => {
  if (e.payload.task_id === taskId) {
    audioWorklet.port.postMessage(new Uint8Array(e.payload.data));
    if (e.payload.is_last) unlisten();
  }
});

// 图像生成（同步）
const paths = await invoke<string[]>("generate_image", {
  payload: { prompt: "A cat in space", model: "image-01", n: 2, aspect_ratio: "16:9" }
});

// 视频/音乐生成（异步轮询）
const taskId = await invoke<string>("generate_video", {
  payload: { prompt: "...", model: "veo-3.1-generate-preview" }
});
await listen<MediaTaskEvent>("media_task_update", (e) => {
  if (e.payload.task_id !== taskId) return;
  if (e.payload.status === "done") openMediaPreview(e.payload.file_paths!);
  if (e.payload.status === "failed") showError(e.payload.error);
});

// 歌词生成（MiniMax）
const lyrics = await invoke("generate_lyrics", {
  payload: { mode: "write_full_song", prompt: "一首关于夏日海边的轻快情歌" }
});
// 用生成歌词继续生成音乐
const musicTaskId = await invoke<string>("generate_music", {
  payload: { model: "music-3.0", prompt: "流行, 轻快, 夏日", lyrics: lyrics.lyrics }
});

// 音色设计
const { voice_id } = await invoke("design_voice", {
  payload: { prompt: "讲述悬疑故事的播音员，声音低沉富有磁性", preview_text: "夜深了……" }
});
```

---

## 命名与调用约定

- 所有 Command 名称使用 `snake_case`，与 Rust 函数名一致；Tauri 自动映射，前端 `invoke("snake_case_name")` 直接使用。
- 复杂入参统一包裹为 `payload` 字段，避免 Tauri 参数列表过长。
- API Key 等敏感字段在 Rust 侧优先写入系统 Keychain（macOS Keychain / Windows Credential Manager / Linux Secret Service，通过 `keyring` crate）；当系统 Keychain 不可用时，降级写入 `~/.astro/secrets/<provider_id>.key` 加密文件（AES-256-GCM，密钥由主密码派生）。任何情况下均不落入 SQLite 明文。
- 流式 Command 立即返回任务 ID，实际数据通过 Event 推送，前端负责在组件卸载时调用 `unlisten()`。
