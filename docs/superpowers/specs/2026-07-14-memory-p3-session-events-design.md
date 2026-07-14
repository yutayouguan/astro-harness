# Memory P3：SessionEvents 通知与 Slash 审批

**日期:** 2026-07-14  
**状态:** 已批准（实现中 / 分支 `feat/memory-p3-session-events`）  
**范围:** 独立会话事件流、记忆更新 Toast、pending 角标、「刷新进对话」、可配自动 refresh、完整 `/memory` 子命令  
**前置:** [`2026-07-14-memory-hermes-alignment-design.md`](./2026-07-14-memory-hermes-alignment-design.md)（P1/P2 已落地）

## 命名约束

- 代码、UI、路径中不得出现外部参考项目品牌名。
- 推荐命名：`SessionEvent`、`SubscribeSessionEvents`、`memory_updated`、`pending_changed`、`auto_refresh_on_update`。

## 目标

在 P1/P2 有界记忆与审批之上补齐**可感知、可操作**的闭环：

1. **独立事件通道**：回合后 review / pending / 入梦等副作用不依赖已结束的 Chat 流  
2. **桌面反馈**：Toast + 记忆导航 pending 角标  
3. **显式刷新**：记忆页「刷新进对话」；slash `/memory refresh`  
4. **可配自动 refresh**：默认开启，批准或 live 写入后刷新当前会话 frozen snapshot  
5. **完整 slash**：`list` / `approve [id|all]` / `reject [id|all]` / `refresh`

## 非目标

- SessionEvents 历史 replay / 持久化事件日志  
- 消息平台 slash  
- 将回合内工具写的 Chat `memory_update` 活动卡迁到 SessionEvents  
- 改动 SessionStore schema 或 MEMORY 文件格式

## 决策摘要

| 项 | 选择 |
|----|------|
| 范围 | Toast + 刷新按钮 + pending 角标 + 完整 `/memory` 子命令 |
| 通道 | 会话级 gRPC `SubscribeSessionEvents`（方案 1） |
| Slash | 子命令齐全；裸 `/memory` 仍进记忆页 |
| 快照 | `memory.auto_refresh_on_update` 默认 `true` |

---

## §1 架构与数据流

### 分层

| 层 | 职责 | 主要位置 |
|----|------|----------|
| Proto | `SubscribeSessionEvents` + `SessionEvent` | `proto/proto/astro.proto` |
| Backend hub | 按 `session_id` / `agent_id` fan-out；无订阅者丢弃 | `backend`（新建轻量 hub） |
| Emitters | review 落盘、pending 入队/批驳、入梦写 MEMORY | `agent` review spawn、`memory` pending、dreaming |
| Tauri | 长订阅流 → `app.emit("session_event", …)` | `frontend/src-tauri` |
| 前端 | Toast、nav 角标、可选自动 refresh、slash 分发 | `App.tsx` / `MemoryPanel` / `composerCommands` |

### 数据流

```text
review / pending / dreaming 变更
  → backend publish SessionEvent
  → SubscribeSessionEvents stream
  → Tauri emit session_event
  → UI toast / badge
  → if auto_refresh_on_update && live_written:
        refresh_memory(sessionId)

/memory approve all
  → Tauri approve API
  → emitters publish pending_changed + memory_updated
  → 同上
```

### 与 Chat 流分工

| 通道 | 用途 |
|------|------|
| Chat `memory_update` | 回合内 `memory` 工具成功 → 时间线活动卡（**保持不动**） |
| SessionEvents | 回合后 / pending / toast·角标·自动 refresh |

---

## §2 事件契约与配置

### Proto

```protobuf
rpc SubscribeSessionEvents(SubscribeSessionEventsRequest)
    returns (stream SessionEvent);

message SubscribeSessionEventsRequest {
  string session_id = 1;  // 空：收全局 pending；非空：该会话 + 全局 pending
  string agent_id = 2;    // 可选过滤；空 = 不过滤
}

message SessionEvent {
  string session_id = 1;  // 可空
  string agent_id = 2;
  int64 ts_ms = 3;
  oneof payload {
    MemoryUpdatedEvent memory_updated = 10;
    PendingChangedEvent pending_changed = 11;
  }
}

message MemoryUpdatedEvent {
  string source = 1;      // review | approve | dreaming | tool
  string target = 2;      // memory | user | mixed
  string summary = 3;
  bool live_written = 4;  // false = 仅入 pending
}

message PendingChangedEvent {
  uint32 pending_count = 1;
  string reason = 2;      // enqueued | approved | rejected
}
```

### 发布时机

| 源 | 事件 |
|----|------|
| background review 写 live | `memory_updated(source=review, live_written=true)` |
| review / tool / dreaming **入 pending** | `pending_changed` + `memory_updated(live_written=false)`（toast 提示待审批） |
| approve / reject | `pending_changed`；approve 成功再发 `memory_updated(source=approve, live_written=true)` |
| 入梦写 live MEMORY | `memory_updated(source=dreaming, live_written=true)` |
| 回合内 tool **直接写 live** | **只走 Chat `memory_update`**，不发 SessionEvent（避免双通道；prefix cache 仍按 frozen 语义，用户可用 `/memory refresh`） |
| review 失败 | 只打日志，**不**发 `memory_updated` |

### 配置

```yaml
memory:
  auto_refresh_on_update: true  # 默认开
```

- 扩展 `MemoryConfig` + `get_memory_settings` / `set_*`（审批页开关，与 `write_approval` / `background_review` 并列）
- 写回 `config.yaml` 时保留其它键（沿用现有 Value 合并）

### 订阅语义

- Tauri：有聊天 `sessionId` 时带该 id 订阅；亦可订空 id 只收 `pending_changed`（角标）
- 无订阅者：publish 静默丢弃，不阻塞 Agent
- 断流：前端指数退避重订；**P3 不做 replay**

---

## §3 UI / Slash / 错误处理

### UI

| 表面 | 行为 |
|------|------|
| Toast | `live_written=true` →「记忆已更新」；`false` →「有待审批的记忆写入」；同 summary 2s 去重 |
| Nav 记忆角标 | `pending_count > 0` 显示数字 |
| 记忆页 | 「刷新进对话」调用 `refresh_memory(agent?, sessionId?)`；审批区增加 auto-refresh 开关 |
| Chat 时间线 | SessionEvents **不**插入 memory 活动卡 |

### Slash

| 输入 | 行为 |
|------|------|
| `/memory` | `nav_memory`（现有） |
| `/memory list` | 列出 pending 摘要 |
| `/memory approve` / `approve all` | 批准全部 |
| `/memory approve <id>` | 批准单条 |
| `/memory reject` / `reject all` / `reject <id>` | 拒绝 |
| `/memory refresh` | 强制刷新当前会话 snapshot |

未知子命令 → 本地帮助文案，不发给模型。  
可选 Tauri：`approve_all_pending_memory_writes` / `reject_all_…` 减少 N 次往返。

### 错误处理

| 情况 | 行为 |
|------|------|
| Subscribe 断流 | 退避重连；角标以 list/pending_changed 校正 |
| refresh 失败 | toast 错误；不阻断批准 |
| approve 部分失败 | 汇报成功/失败数并刷新列表 |
| 无 sessionId 时自动 refresh | 跳过会话刷新，仍可 toast / 角标 |

### 测试要求

1. Hub：有/无订阅者 publish 不 panic  
2. 假订阅能收到 review 后事件  
3. Slash 解析矩阵  
4. `auto_refresh_on_update=false` 时不调 refresh  
5. Chat 流 `memory_update` 回归不变  

---

## 验收标准

- [ ] `SubscribeSessionEvents` 可用；review 落盘后桌面收到 `memory_updated`  
- [ ] pending 入队/批驳更新角标  
- [ ] Toast 文案区分 live / pending  
- [ ] 默认自动 refresh；开关关闭后仅 toast  
- [ ] 记忆页「刷新进对话」对当前 `sessionId` 生效  
- [ ] `/memory` 子命令矩阵行为正确；裸 `/memory` 仍导航  
- [ ] 回合内 Chat `memory_update` 时间线不变  
- [ ] 无外部参考项目品牌名  

## 实现顺序建议

1. Proto + backend hub + 单测  
2. Emitters（review / pending / dreaming）  
3. Tauri 订阅 + 前端 toast / 角标 / auto-refresh  
4. MemoryPanel 刷新按钮 + settings 开关  
5. Slash 子命令 + 可选 approve_all API  
6. 文档（`docs/memory.md`）与回归  

## 风险与缓解

| 风险 | 缓解 |
|------|------|
| Hub 与 Chat session 生命周期不一致 | pending 用全局广播；session 事件允许空 id |
| 自动 refresh 打断 prefix cache | 默认开但可关；toast 说明已刷新 |
| 双通道重复打扰 | SessionEvents 不写时间线卡；toast 去重 |
| Proto 膨胀 | P3 仅两个 payload；后续再扩 dreaming_done 等 |

## 参考

- 记忆对齐：[`2026-07-14-memory-hermes-alignment-design.md`](./2026-07-14-memory-hermes-alignment-design.md)  
- 用户文档：[`docs/memory.md`](../../memory.md)  
