# proto

Protobuf / tonic gRPC 服务契约 -- Astro Agent 桌面壳与 backend 之间的唯一通信边界。

## 核心职责

1. **定义 `AstroService`**：31 个 RPC 方法，覆盖 Thread 提交/订阅、聊天控制、Realtime、图片生成、技能执行、Extension reconcile、MCP 管理、记忆查询、Token 计数与 Batch API
2. **消息类型定义**：`ChatRequest`、`ThreadEvent`、`ThreadSnapshot` 等 69 个 protobuf message，承载前后端全部数据交换
3. **双端代码生成**：`build.rs` 通过 `tonic_build` 同时生成 server trait（`AstroServiceServer`）和 client stub（`AstroServiceClient`），供 `agent-server` 实现、Tauri shell 调用
4. **流式事件推送**：`SubscribeThreadEvents` 返回 `stream ThreadEvent`，支持 turn 开始/结束、item 完成、delta 增量、控制请求等实时推送

## 模块结构

| 文件 | 职责 |
|------|------|
| `proto/astro.proto` | Protobuf 服务与消息定义源文件（31 RPC，包含 Extension reconcile 请求/报告） |
| `build.rs` | 编译 `.proto` 文件，生成 tonic server + client Rust 代码 |
| `src/lib.rs` | 通过 `tonic::include_proto!("astro")` 导出生成代码 |

## 核心类型与 API

### RPC 方法（`AstroService` trait）

| 方法 | 说明 |
|------|------|
| `SubmitTurn` | 提交用户输入到 Thread（新建或已有） |
| `SubscribeThreadEvents` | 长连接订阅事件流（server-streaming） |
| `ResumeThread` | 重建持久历史并订阅连接 |
| `UnsubscribeThread` | 取消订阅但不停止 Thread |
| `ChatControl` | 暂停/继续/取消/新建对话/刷新记忆/释放会话 |
| `SteerChat` | Codex 式中途引导注入 |
| `InterruptResume` | 校验并登记 interrupt 恢复项 |
| `ResolveElicitation` | 按 server/request id 解决一个 MCP elicitation |
| `UpdateTurnSettings` | 原子更新 active turn 下一次 sampling 的模型/reasoning/service tier |
| `ReconcileExtensions` | 比较扩展快照并返回 changed IDs 与能力刷新标志 |
| `RealtimeConversation*` | Realtime start/audio/text/speech/close/voice-list 控制面 |
| `GenerateImage` | 文生图流（进度 + 图片字节） |
| `ListSkills` / `ExecuteSkill` | 技能列表与流式执行 |
| `ListMcpServers` / `ReconnectMcpServer` | MCP 服务器状态查询与重连 |
| `QueryMemory` | 记忆召回（MEMORY/USER + 会话摘要） |
| `ListFiles` | 沙箱内文件列举 |

### 关键 message 类型

| 类型 | 说明 |
|------|------|
| `ChatRequest` | 聊天发起请求：会话 ID、模型凭证、图片附件、辅助模型目标、workspace roots、tool mode 与 `persistent_instructions` 等 42 个字段（最高 field number 为 43） |
| `ChatFallbackTarget` | 聊天后备目标（provider / model / api_key / base_url） |
| `AuxiliaryModelTarget` | 辅助任务模型目标（title_generation / compaction 等五类） |
| `ChatControlAction` | 流控枚举：PAUSE / RESUME / CANCEL / NEW_CHAT / REFRESH_MEMORY / RELEASE_SESSION |
| `ThreadEvent` | 事件推送载体，`oneof payload` 含 15 种事件变体 |
| `ThreadSnapshot` | Thread 快照：状态、当前 `provider_id` / `backend_id` / `model` / `reasoning_effort`、历史 turn 列表、活跃 turn、后台 turn ID |
| `ThreadTurn` / `ThreadItem` | Turn 和 Item 的线协议表示 |
| `ImageRequest` / `ImageEvent` | 图片生成请求与流式响应 |

## 与其他 crate 的关系

```
agent-proto (本 crate)
  ├── 被 agent-server 实现 → AstroServiceServer trait
  ├── 被 apps/desktop/src-tauri 调用 → AstroServiceClient
  └── 依赖：tonic + prost（无内部 crate 依赖）
```

- **agent-server**：实现 `AstroService` trait，将 RPC 请求转发给 `agent-core` 运行时
- **apps/desktop (Tauri)**：通过 `AstroServiceClient` 连接内嵌或独立 backend
- **无依赖其他内部 crate**：proto 层保持纯净，仅依赖 tonic / prost

## 测试运行

```bash
# 编译检查（验证 proto 生成代码）
cargo check -p proto

# 构建（触发 build.rs 重新生成）
cargo build -p proto
```

> 本 crate 无独立测试文件；proto 定义的正确性通过 `agent-server` 集成测试间接覆盖。
