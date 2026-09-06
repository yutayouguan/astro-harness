# 桌面端（Tauri v2）

> **Harness 边界（2026-08-29）**：Tauri 是 Harness 的宿主和 UI 适配层。它提交 Thread `Op`、转发 gRPC/live events 并呈现审批，但不应复制 turn 状态机、工具路由或恢复真相。

> 阶段：系统设计 | 状态：定稿 | 说明：Tauri v2 架构、React 前端、事件驱动

## 目录结构

```text
apps/desktop/
├── src-tauri/                      # Tauri Rust 层
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   ├── capabilities/
│   │   └── default.json            # Tauri v2 权限声明
│   └── src/
│       ├── main.rs
│       ├── lib.rs                  # tauri::Builder 配置
│       ├── commands/               # #[tauri::command] 暴露给前端
│       │   ├── mod.rs
│       │   ├── agent.rs            # run_agent, stop_agent
│       │   ├── config.rs           # get_config, save_config
│       │   ├── providers.rs        # list_providers, test_connection
│       │   ├── skills.rs           # list_skills, create_skill
│       │   ├── memory.rs           # search_memory, clear_memory
│       │   ├── evolution.rs        # get_evolution_log, approve_evolution
│       │   └── media.rs            # tts_stream, asr, generate_image
│       ├── state.rs                # Tauri managed state
│       ├── events.rs               # emit 到前端的事件定义
│       └── tray.rs                 # 系统托盘
│
└── src/                            # 前端 React + TypeScript
    ├── main.tsx
    ├── App.tsx
    ├── pages/
    │   ├── Chat.tsx                # 主对话界面
    │   ├── Skills.tsx              # Skill 管理
    │   ├── Memory.tsx              # 记忆浏览器
    │   ├── Evolution.tsx           # 进化日志 & 审批
    │   ├── Providers.tsx           # Provider 配置
    │   └── Settings.tsx
    ├── components/
    │   ├── chat/
    │   │   ├── MessageList.tsx
    │   │   ├── MessageBubble.tsx
    │   │   ├── MediaPreview.tsx    # 图片/音频/视频渲染
    │   │   ├── ToolCallCard.tsx    # 工具调用展示
    │   │   ├── AgentTrace.tsx      # 子 Agent 树形展示
    │   │   └── InputBar.tsx        # 支持拖拽上传媒体
    │   ├── evolution/
    │   │   ├── EvolutionFeed.tsx
    │   │   └── ApprovalModal.tsx
    │   └── shared/
    ├── store/
    │   ├── chat.ts                 # Zustand
    │   ├── agent.ts
    │   └── evolution.ts
    ├── hooks/
    │   ├── useAgent.ts             # 封装 invoke + 事件监听
    │   ├── useStream.ts            # 流式响应
    │   └── useMedia.ts             # 媒体录制/播放
    └── lib/
        └── tauri.ts                # 类型化 invoke 封装
```

---

## Tauri Managed State

```rust
// apps/desktop/src-tauri/src/state.rs

pub struct AppState {
    pub provider_registry: Arc<RwLock<ProviderRegistry>>,
    pub tool_registry: Arc<RwLock<ToolRegistry>>,
    pub skill_registry: Arc<RwLock<SkillRegistry>>,
    pub db_pool: SqlitePool,
    pub session_manager: Arc<SessionManager>,
    pub human_guard: Arc<HumanGuard>,
    pub media_tasks: Arc<RwLock<HashMap<String, MediaTaskStatus>>>,
    pub mcp_manager: Arc<McpClientManager>,
}
```

---

## Tauri Commands

```rust
// apps/desktop/src-tauri/src/commands/agent.rs

/// 启动 Agent 任务，流式推送事件到前端
#[command]
pub async fn run_agent(
    task: String,
    media: Vec<MediaInput>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<String, String> {
    let task_id = Uuid::new_v4();
    let ctx = state.ctx.clone();
    let app_clone = app.clone();

    let handle = tokio::spawn(async move {
        let mut rx = ctx.agent.run_stream(task, media).await?;
        while let Some(event) = rx.recv().await {
            // 事件类型：token_chunk / thinking_delta / tool_call / message_done 等
            app_clone.emit("token_chunk", AgentEvent { task_id, payload: event }).ok();
        }
        Ok::<_, anyhow::Error>(())
    });

    state.active_tasks.write().await.insert(task_id, TaskHandle(handle));
    Ok(task_id.to_string())
}

/// 流式 TTS，边合成边推音频 chunk 给前端
#[command]
pub async fn text_to_speech_stream(
    text: String,
    voice: String,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    let tts = state.ctx.providers.tts("minimax/speech-2.8-hd")
        .map_err(|e| e.to_string())?;

    let mut stream = tts.stream_synthesize(TtsRequest {
        text, voice, output_format: AudioMime::Mp3, ..Default::default()
    }).await.map_err(|e| e.to_string())?;

    while let Some(chunk) = stream.next().await {
        app.emit("tts_chunk", chunk.map_err(|e| e.to_string())?.to_vec()).ok();
    }
    app.emit("message_done", ()).ok();
    Ok(())
}
```

---

## 前端事件类型

```typescript
// apps/desktop/src/types/events.ts

type AgentEventPayload =
  | { type: "thinking";    content: string }
  | { type: "tool_call";   tool: string; input: unknown }
  | { type: "tool_result"; tool: string; result: unknown; duration_ms: number }
  | { type: "skill_start"; skill: string }
  | { type: "skill_done";  skill: string }
  | { type: "spawn_agent"; child_id: string; role: string; task: string }
  | { type: "text_delta";  delta: string }
  | { type: "media_ready"; media_type: MediaType; url: string }
  | { type: "evolution";   action: EvolutionAction }
  | { type: "done";        metrics: TaskMetrics }
  | { type: "error";       message: string };
```

---

## 类型化 invoke 封装

```typescript
// apps/desktop/src/lib/tauri.ts

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export const agentApi = {
  run:  (task: string, media: MediaInput[]) =>
    invoke<string>("run_agent", { task, media }),

  stop: (taskId: string) =>
    invoke<void>("stop_agent", { taskId }),

  tts:  (text: string, voice: string) =>
    invoke<void>("text_to_speech_stream", { text, voice }),
};

// Agent 事件流 hook
export function useAgentStream(taskId: string | null) {
  const [events, setEvents] = useState<AgentEventPayload[]>([]);

  useEffect(() => {
    if (!taskId) return;
    const unlisten = listen<AgentEvent>("token_chunk", (e) => {
      if (e.payload.taskId !== taskId) return;
      setEvents(prev => [...prev, e.payload.payload]);
    });
    return () => { unlisten.then(fn => fn()); };
  }, [taskId]);

  return events;
}
```

---

## AgentTrace 组件

```typescript
// apps/desktop/src/components/chat/AgentTrace.tsx

export function AgentTrace({ taskId }: { taskId: string }) {
  const events = useAgentStream(taskId);

  return (
    <div className="agent-trace">
      {events.map((e, i) => {
        switch (e.type) {
          case "thinking":
            return <ThinkingBubble key={i} content={e.content} />;
          case "tool_call":
            return <ToolCallCard key={i} tool={e.tool} input={e.input} />;
          case "spawn_agent":
            return <ChildAgentTrace key={i} agentId={e.child_id} role={e.role} />;
          case "media_ready":
            return <MediaPreview key={i} type={e.media_type} url={e.url} />;
          case "evolution":
            return <EvolutionToast key={i} action={e.action} />;
        }
      })}
    </div>
  );
}
```

---

## Tauri v2 权限配置

```json
// apps/desktop/src-tauri/capabilities/default.json
{
  "identifier": "default",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "core:event:allow-listen",
    "core:event:allow-emit",
    "core:window:allow-start-dragging",
    "shell:allow-open",
    "fs:allow-app-read-recursive",
    "fs:allow-app-write-recursive"
  ]
}
```

---

## Tauri Cargo.toml

```toml
# apps/desktop/src-tauri/Cargo.toml
[dependencies]
agent-core      = { path = "../../../crates/agent-core" }
agent-providers = { path = "../../../crates/agent-providers" }

tauri      = { version = "2", features = ["tray-icon"] }
tokio      = { version = "1", features = ["full"] }
serde      = { version = "1", features = ["derive"] }
serde_json = "1"
uuid       = { version = "1", features = ["v4"] }
anyhow     = "1"

[build-dependencies]
tauri-build = "2"
```

---

---

## 多窗口状态同步

Tauri v2 允许多窗口共存（如主窗口 + 侧边工具窗口），共享同一个 `AppState`（Rust Managed State），但每个 `WebviewWindow` 有独立的 JS 运行时和 Zustand store，不会自动同步。

### 同步策略：Rust 广播 → 各窗口订阅

```rust
// apps/desktop/src-tauri/src/events.rs

/// 广播给所有窗口的通用方法
pub fn broadcast<S: Serialize + Clone>(app: &AppHandle, event: &str, payload: S) {
    // Tauri v2：向所有 WebviewWindow 广播
    app.emit(event, payload).ok();
}
```

前端各窗口均在 `App.tsx` 挂载时 `listen()` 同一组事件，Zustand store 根据事件更新本地状态：

```typescript
// apps/desktop/src/store/agent.ts

listen<AgentEvent>("token_chunk", (e) => {
  useAgentStore.getState().handleEvent(e.payload);
});

listen("yolo_force_disabled", (e) => {
  useAgentStore.getState().setYolo(false);
});

listen("budget_alert", (e) => {
  useNotificationStore.getState().push(e.payload);
});
```

### 需要跨窗口同步的状态

| 状态 | 事件名 | 说明 |
| ---- | ------ | ---- |
| Token 流 | `token_chunk` | LLM 流式输出每个 token |
| 思考过程 | `thinking_delta` | thinking 模式推理内容增量 |
| 工具调用 | `tool_call` | 工具调用状态变化 |
| TTS 音频流 | `tts_chunk` | TTS 流式音频块推送 |
| 多媒体任务 | `media_task_update` | 图像/视频/音乐生成进度与完成通知 |
| 消息完成 | `message_done` | 本轮消息生成结束 |
| YOLO 被强制关闭 | `yolo_force_disabled` | 所有窗口切换 YOLO 开关到 OFF |
| 预算告警 | `budget_alert` | 所有窗口展示 Toast |
| 审批弹窗 | `approval_request` | 仅主窗口处理（`windows: ["main"]`，通过 capabilities 限制） |

### 审批弹窗的窗口限制

`approval_request` 等安全相关事件只允许 main 窗口接收，在 `capabilities/default.json` 中为其他窗口排除此事件，防止重复弹窗：

```json
{
  "identifier": "tool-window",
  "windows": ["tools"],
  "permissions": [
    "core:default",
    "core:event:allow-listen",
    "core:event:allow-emit"
  ]
}
```

> `approval_request` 不加入 tool-window 的 permissions 即可阻止接收；Rust 侧 emit 时指定目标窗口 label 也可精确投递。

---

## 关键设计决策

- **事件驱动而非轮询** — 全程 `app.emit()` 推事件，前端 `listen()` 订阅
- **媒体文件走本地路径** — 生成结果保存到 `AppData`，前端用 `asset://` 协议加载
- **多任务并发** — `HashMap<Uuid, TaskHandle>` 支持同时运行多个 Agent 任务
- **进化实时推送** — 进化引擎产生事件直接 emit，用户在 Evolution 页面实时看到
- **多窗口共享 AppState** — Rust 层单一 `AppState`，JS 层通过事件同步；安全相关事件限 main 窗口
