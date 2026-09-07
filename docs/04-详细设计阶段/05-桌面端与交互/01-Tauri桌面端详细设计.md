# Tauri 桌面端详细设计

> **Harness 边界（2026-08-29）**：Tauri 是 Harness 宿主/投影层，当前默认在进程内启动 `agent-server::run_embedded()`，通过 Thread RPC 提交 `Op`并消费 rollout + live 事件。AppState 不应复制 SessionTask、ToolRouter 或恢复状态机。

> 阶段：详细设计 | 状态：草稿 | 说明：AppState 依赖注入容器、Registry 初始化与生命周期

## 1. AppState 设计

`AppState` 是桌面端的全局依赖注入容器，所有注册表和连接池以 `Arc` 包裹，跨线程安全共享：

```rust
pub struct AppState {
    pub provider_registry: Arc<RwLock<ProviderRegistry>>,
    pub tool_registry:     Arc<RwLock<ToolRegistry>>,
    pub skill_registry:    Arc<RwLock<SkillRegistry>>,
    pub db_pool:           SqlitePool,                           // sqlx 内部已 Arc
    pub session_manager:   Arc<SessionManager>,
    pub human_guard:       Arc<HumanGuard>,
    /// 活跃媒体任务状态缓存（task_id → status），避免频繁查 DB
    pub media_tasks:       Arc<RwLock<HashMap<String, MediaTaskStatus>>>,
    pub mcp_manager:       Arc<McpClientManager>,
}
```

`AppState` 通过 `tauri::Builder::manage` 注入，全局单例。所有 Tauri Command 的第一个参数均为 `State<'_, AppState>`。

---

## 2. 应用启动流程

```text
tauri::Builder::new()
  │
  ├─ plugin::log  初始化（写入 ~/.astro/logs/）
  ├─ SqlitePool::open(~/.astro/sessions/state.db)
  │      └─ 迁移执行（schema v22）
  │
  ├─ load_provider_configs(~/.astro/config.toml, ~/.astro/providers.json)
  ├─ ProviderRegistry::build() 注册专属 Provider 客户端
  ├─ ToolRegistry::register_builtins() 注册内置工具
  ├─ SkillRegistry::load_from_db(&pool) 加载活跃 Skills，构建 BM25 索引
  ├─ McpClientManager::connect_all(~/.astro/config.toml)
  │
  ├─ SessionManager::new(pool, registry)
  ├─ HumanGuard::new(app_handle)
  ├─ AppState::new(all above) → tauri::Builder::manage
  │
  ├─ 后台任务启动（tokio::spawn）
  │      ├─ run_media_poller(pool, registry, app)
  │      ├─ run_memory_distiller(pool, provider)
  │      └─ run_forget_scheduler(pool)
  │
  ├─ invoke_handler(generate_handler![...])  // 全部 58 个 Commands
  ├─ setup(|app| { app.get_webview_window("main")?.show() })
  └─ run()
```

迁移失败时 `setup` 返回 `Err`，Tauri 弹出系统级错误对话框并终止进程。

---

## 3. Tauri Command 实现模式

所有 Command 遵循统一签名，`AppError` 实现 `serde::Serialize`，序列化后传回前端：

```rust
#[tauri::command]
pub async fn list_conversations(
    workspace_id: String,
    state: State<'_, AppState>,
) -> Result<Vec<ConversationMeta>, AppError> {
    ConversationRepo::new(&state.db_pool)
        .list(&workspace_id, 50)
        .await
        .map_err(AppError::Db)
}
```

`AppError` 统一封装所有错误类型：

```rust
#[derive(Debug, Serialize)]
#[serde(tag = "kind", content = "message")]
pub enum AppError {
    Db(String),
    Provider(String),
    Tool(String),
    Validation(String),
    NotFound(String),
}
```

前端按 `error.kind` 字段分支处理，复杂参数通过 `#[serde(rename_all = "camelCase")]` 结构体传入。

---

## 4. 流式响应推送

`send_message` 立即返回 `conversation_id`，实际推流在 `tokio::spawn` 异步任务中完成：

```rust
#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: SendMessagePayload,
) -> Result<String, AppError> {
    let conv_id = payload.conversation_id.clone();
    let session_manager = state.session_manager.clone();

    tokio::spawn(async move {
        let handle = session_manager.get_or_create(&conv_id).await;
        let (event_tx, mut event_rx) = mpsc::channel(128);

        // 启动 Agent 编排循环（通过 agent-runtime 的 AgentExecutor，内部调用 agent-core::run_agent_turn）
        let executor = AgentExecutor::new(handle, event_tx);
        tokio::spawn(async move { executor.round_loop(payload.message).await });

        // 转发 AgentEvent → Tauri 前端事件
        while let Some(event) = event_rx.recv().await {
            match event {
                AgentEvent::TokenChunk(delta) =>
                    app.emit("token_chunk", TokenChunkEvent { conv_id: conv_id.clone(), delta }).ok(),
                AgentEvent::ThinkingDelta(delta) =>
                    app.emit("thinking_delta", ThinkingDeltaEvent { conv_id: conv_id.clone(), delta }).ok(),
                AgentEvent::ToolCall(tc) =>
                    app.emit("tool_call", tc).ok(),
                AgentEvent::ToolResult(tr) =>
                    app.emit("tool_result", tr).ok(),
                AgentEvent::ApprovalRequest(req) =>
                    app.emit("approval_required", req).ok(),
                AgentEvent::TtsChunk(bytes) =>
                    app.emit("tts_chunk", TtsChunkEvent { data: bytes }).ok(),
                AgentEvent::MediaTaskUpdate(ev) =>
                    app.emit("media_task_update", ev).ok(),
                AgentEvent::Done =>
                    { app.emit("message_done", &conv_id).ok(); break; }
                AgentEvent::Error(msg) =>
                    { app.emit("message_error", ErrorEvent { conv_id: conv_id.clone(), message: msg }).ok(); break; }
            };
        }
    });

    Ok(conv_id)
}
```

---

## 5. HumanGuard 与前端交互

Rust 侧 HumanGuard 暂停工具调用，推送审批请求；前端用户操作后回调：

```rust
// HumanGuard::check 内部
let (tx, rx) = oneshot::channel::<ApprovalDecision>();
state.human_guard.pending.insert(request_id.clone(), tx);
app.emit("approval_required", ApprovalRequestEvent {
    request_id: request_id.clone(),
    tool_name: tool.name(),
    risk_level: tool.risk_level(),
    timeout_ms: match tool.risk_level() { RiskLevel::Medium => 30_000, _ => 0 },
}).ok();
let decision = rx.await?;   // 挂起等待
```

```rust
#[tauri::command]
pub async fn resolve_approval(
    state: State<'_, AppState>,
    request_id: String,
    approved: bool,
) -> Result<(), AppError> {
    state.human_guard.resolve(
        &request_id,
        if approved { ApprovalDecision::Approved } else { ApprovalDecision::Rejected },
    );
    Ok(())
}
```

---

## 6. 媒体任务事件路由

媒体生成（图像/视频/音乐）采用异步提交 + 轮询模式，状态变更通过 `media_task_update` 事件推送前端：

```rust
#[tauri::command]
pub async fn generate_video(
    app: AppHandle,
    state: State<'_, AppState>,
    payload: GenerateVideoPayload,
) -> Result<String, AppError> {
    let client = state.provider_registry.read().await
        .video_client(&payload.provider)?;

    // 提交异步任务，获取 task_id
    let task_id = client.submit(VideoRequest::from(&payload)).await
        .map_err(|e| AppError::Provider(e.to_string()))?;

    // 写入 media_tasks 表
    MediaTaskRepo::new(&state.db_pool)
        .create(&NewMediaTask { task_id: task_id.clone(), ..payload.into() })
        .await.map_err(AppError::Db)?;

    // 缓存状态
    state.media_tasks.write().await.insert(task_id.clone(), MediaTaskStatus::Pending);

    Ok(task_id)
}
```

`run_media_poller` 后台任务每 3s 查询一次，状态变更时：

```rust
state.media_tasks.write().await.insert(task_id.clone(), result.status.clone());
app.emit("media_task_update", MediaTaskEvent {
    task_id, status: result.status, progress_pct: result.progress_pct,
    file_paths: result.output_urls,
}).ok();
```

---

## 7. Provider 密钥管理

Provider 配置统一存储在 `~/.astro/config.toml`（含自定义 Provider 声明和 `env_keys` 环境变量映射），前端 Provider 配置状态写入 `~/.astro/providers.json`。密钥通过环境变量或 `config.toml` 中的 `env_keys` 字段引用，密钥明文不直接持久化在配置文件中。

```rust
#[tauri::command]
pub async fn save_provider_key(
    provider: String,
    key: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    // 热更新 ProviderRegistry，无需重启
    state.provider_registry.write().await.reload_key(&provider, &key);
    Ok(())
}
```

前端获取的 `ProviderConfig` 只含 `has_key: bool`，密钥明文永不离开 Rust 进程。

---

## 8. 多窗口策略

采用"单主窗口 + 按需弹窗"策略：

| 窗口 | 标识符 | 生命周期 | 说明 |
| --- | --- | --- | --- |
| 主窗口 | `main` | 常驻 | 5 项侧栏导航（chat/cron/loop/skills/settings）、聊天面板、右侧面板 |
| 快速对话窗口 | `quick-chat` | 按需 | 系统托盘触发，`always_on_top + skip_taskbar` |

主窗口 `visible(false)` 启动，初始化完成后 `window.show()`，避免白屏闪烁（FOUC）。

---

## 9. 系统托盘

```rust
SystemTray::new()
    .with_menu(tray_menu![
        MenuItem::new("new_chat",    "新建对话",   true, None::<&str>),
        MenuItem::new("toggle_main", "显示/隐藏", true, None::<&str>),
        PredefinedMenuItem::separator(),
        MenuItem::new("quit",        "退出",       true, None::<&str>),
    ])
    .on_menu_event(|app, event| match event.id.as_ref() {
        "new_chat"    => app.emit_to("main", "tray_new_chat", ()).ok(),
        "toggle_main" => toggle_main_window(app),
        "quit"        => app.exit(0),
        _ => {}
    })
```

`toggle_main_window` 检查主窗口 `is_visible()` 后切换可见性，显示时调用 `set_focus()` 置前。

---

## 10. 全局快捷键

```rust
// src-tauri/src/hotkeys.rs
app.global_shortcut_manager().register("CmdOrCtrl+Shift+Space", move || {
    // 唤起/隐藏主窗口（全局快捷键）
    toggle_main_window(&app_handle);
})?;
```

应用内快捷键由前端 `react-hotkeys-hook` 管理（`Cmd+K` 命令面板、`Esc` 关闭弹窗、`Cmd+N` 新对话等），不经过 Tauri 全局快捷键层，避免与系统快捷键冲突。

---

## 11. 多窗口管理

### 11.1 创建新窗口

| 触发方式 | 目标窗口内容 |
|---------|------------|
| 右键对话 → "在新窗口打开" | 独立对话窗口（无侧边栏） |
| 右键工作区 → "在新窗口打开" | 完整主界面（含侧边栏），显示该工作区 |
| Cmd+N | 新建空白主窗口 |

### 11.2 窗口标识

- 标题栏格式："{对话标题} — Astro Agent" 或 "{工作区名} — Astro Agent"
- 主窗口标题栏无前缀，直接显示 "Astro Agent"
- macOS: 窗口代理图标（proxy icon）显示工作区路径

### 11.3 窗口生命周期

- 关闭主窗口：所有子窗口保持打开，应用不退出（系统托盘驻留）
- 关闭最后一个窗口：应用最小化到托盘，不退出
- 托盘图标右键：显示最近对话列表 + "新窗口" + "退出"
- 点击托盘图标：恢复主窗口

### 11.4 窗口间状态同步

- 对话列表变更（新建/删除/重命名）→ 所有窗口实时同步
- 对话内容变更 → 仅显示该对话的窗口更新
- 工作区切换 → 仅主窗口响应（子窗口绑定到特定对话/工作区）
- 使用 Tauri 的 `WebviewWindow::emit_all()` 广播事件

### 11.5 Tauri 实现

窗口创建代码:
```rust
app.create_webview_window(label, url, config)
```
- `label`: `"conversation-{conv_id}"` 或 `"workspace-{ws_id}"`
- `url`: `"/conversation/{conv_id}"` 或 `"/workspace/{ws_id}"`
- `config`: 宽高继承主窗口，位置偏移 (20, 20)
