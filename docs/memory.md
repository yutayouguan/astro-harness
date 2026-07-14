# Astro 记忆系统

Astro 将长期记忆与用户档案维护为**有界精炼条目**（`MEMORY.md` / `USER.md`），通过 **Frozen Snapshot** 注入 system prompt，并通过单一 **`memory`** 工具写入。会话全文检索走独立的 **`session_search`** 第二轨。

设计规格见 [`docs/superpowers/specs/2026-07-14-memory-hermes-alignment-design.md`](./superpowers/specs/2026-07-14-memory-hermes-alignment-design.md)。

---

## 配置文件

路径：`~/.astro/config.yaml`（或 `ASTRO_MEMORY_DIR` 指向目录下的 `config.yaml`）。与 hooks 等配置**共用同一文件**；`memory` crate 只解析 `memory:` 段，其余键忽略。

```yaml
memory:
  memory_enabled: true          # 是否注入 MEMORY.md 快照
  user_profile_enabled: true    # 是否注入 USER.md 快照
  memory_char_limit: 2200       # MEMORY.md 字符上限（默认 2200）
  user_char_limit: 1375         # USER.md 字符上限（默认 1375）
  write_approval: false         # 开启后 MEMORY/USER 写入进入 pending 审批队列
  daily_prompt_max_chars: 1024  # 今日日记注入 prompt 的最大字符数
```

| 键 | 默认 | 说明 |
|----|------|------|
| `memory_enabled` | `true` | 关闭后不向 prompt 注入长期记忆，且 Agent 侧禁用对 MEMORY 的 `memory` 写入 |
| `user_profile_enabled` | `true` | 关闭后不注入 USER 档案；对称控制 USER 写入 |
| `memory_char_limit` | `2200` | 条目合计字符上限（含 `§` 分隔开销）；超限**报错**，不做 FIFO 淘汰 |
| `user_char_limit` | `1375` | 同上，作用于 USER.md |
| `write_approval` | `false` | 开启后 MEMORY/USER 写入（工具 / 入梦 / 未来 review）进入 pending 队列，批准后才改 live |
| `daily_prompt_max_chars` | `1024` | 每轮可读盘的今日日记截断上限；**不属于**长期记忆 |

P2 还将增加 `auxiliary.background_review` / `auxiliary.dreaming` 模型路由，见设计 spec。

### `write_approval` pending 队列

路径：`~/.astro/pending/memory/{id}.json`。

- `write_approval: false`（默认）：过 Store 门禁后直接写 live
- `true`：`memory` 工具与入梦 MEMORY 写回**入队不改 live**；安全扫描失败**不入队**
- **日记** `append_daily` **不受**审批门禁
- 批准经 `MemoryStore` / `handle_memory_op` 等价路径落盘；拒绝即删除 pending 文件

Tauri 命令（设置页 UI 可后续接）：

| 命令 | 作用 |
|------|------|
| `list_pending_memory_writes` | 列出 pending |
| `approve_pending_memory_write(id)` | 批准并写 live |
| `reject_pending_memory_write(id)` | 拒绝并丢弃 |

---

## 文件与格式

| 文件 | 用途 |
|------|------|
| `{workspace}/MEMORY.md` | 长期精炼记忆 |
| `{workspace}/USER.md` | 用户档案 |
| `{workspace}/mermaid/YYYY-MM-DD.md` | 今日日记（流水，**不可**通过 `memory` 工具写入） |

- **读**：兼容旧版 `-` / `*` 列表；若无 `§` 则按列表解析。
- **写**：成功保存后盘上统一为 `§` 分隔的多行条目格式。
- **旧文件超默认上限**：仍可加载展示；任何会使用量**上升**的写入会失败，需 consolidate 或调高 limit。

写入前会扫描不可见 Unicode 与轻量威胁模式（prompt injection、凭据外泄等）；命中直接失败，不入盘。

---

## 单一 `memory` 工具

已废除 `memory_add` / `memory_replace` / `memory_remove`；若仍调用旧名，会返回迁移提示。

```json
{
  "name": "memory",
  "parameters": {
    "action": "add | replace | remove",
    "target": "memory | user",
    "content": "string?",
    "old_text": "string?"
  }
}
```

| 字段 | 说明 |
|------|------|
| `action` | `add` 追加；`replace` / `remove` 用 `old_text` **子串**定位，须唯一匹配 |
| `target` | `memory` → MEMORY.md；`user` → USER.md |
| `content` | `add` / `replace` 必填 |
| `old_text` | `replace` / `remove` 必填 |

**无 `read` 动作**——Agent 从 system prompt 中的 snapshot 阅读当前记忆；需要历史对话时用 `session_search`。

成功写入后返回 live 用量（如 `1474/2200`），并注明：**已写盘（live）；当前会话 prompt 快照未刷新**。

精确重复的 `add` 视为成功（不重复追加）。

---

## Frozen Snapshot 与刷新

`MemoryStore` 维护 **live**（工具/入梦/review 写入后立即反映到内存与磁盘）与 **snapshot**（供 system prompt）双态。

| 事件 | Snapshot 行为 |
|------|----------------|
| 新建 `AgentLoop` / 切换 `session_id` | 从磁盘 load → 固化 snapshot |
| 每轮 `build_system_prompt` | **只读 snapshot** 填充 MEMORY / USER |
| `memory` 工具成功 | 更新 live + 磁盘；**snapshot 不变** |
| `refresh_memory` | 从磁盘 reload → 同步 snapshot |
| 切换 Agent workspace | 新 `MemoryManager`，必 reload |

今日日记每轮可读盘，但按 `daily_prompt_max_chars` 截断，与 MEMORY/USER snapshot 分层。

### Tauri `refresh_memory`

桌面端提供 `refresh_memory` 命令：对当前 Agent 打开 `MemoryManager`，从磁盘重载 MEMORY / USER 并返回最新 snapshot 渲染文本。**会更新该 Manager 的 snapshot**。

**限制（P1 已知 gap）：**

- Tauri 进程**不持有** backend 侧长驻的 `AgentLoop`；聊天经 gRPC 由 backend 按 `session_id` 缓存 Loop。
- 因此 Tauri `refresh_memory` **不会**自动刷新**正在进行中**的后端会话 prompt。
- 要让对话立刻看到最新 MEMORY/USER，需：**新开对话**（新 session 会 reload），或等待后续 backend RPC 调用 `AgentLoop::refresh_memory`（接口已在 agent crate 实现，P2 可接 gRPC）。

入梦写回 MEMORY 经 `MemoryStore` 门禁（scan + 上限）；成功**不**刷新任何在途 AgentLoop snapshot。

---

## 第二轨：`session_search`

与精炼记忆并列，保留 FTS 全文检索历史消息：

```json
{
  "name": "session_search",
  "parameters": {
    "query": "string",
    "limit": "number?"
  }
}
```

- `limit` 默认 5，最大 10。
- 匹配消息正文、工具名等；结果格式化为 Markdown 列表供 Agent 按需召回。

动态召回（FTS）与 Frozen Snapshot **独立**：snapshot 稳定以利于 prefix cache；全历史检索走 `session_search`。

---

## 入梦（Dreaming）

入梦管线读取 `mermaid/` 日记，经抽取模型产出 MEMORY 条目，写回时：

1. 解析输出为条目（`§` 或列表）
2. 经 `MemoryStore::replace_all_entries` 整体替换 live
3. 强制执行字符上限与安全扫描；失败可观测，**禁止**静默截断

P2 将把抽取模型路由到 `auxiliary.dreaming` 便宜模型；P1 仍用现有路由，但写盘路径已对齐 Store 门禁。

---

## P1 手测清单

- [ ] 旧 `-` 列表 MEMORY 可加载，首次写入后盘上变为 `§` 格式
- [ ] 超限 `memory` 调用报错且含用量（如 `2200/2200`）
- [ ] 同会话写入后 system prompt 仍为旧 snapshot；新 session 或 `refresh_memory` 后更新
- [ ] `session_search` 可检索历史消息
- [ ] 旧工具名 `memory_add` 等返回迁移错误
- [ ] 代码与 UI 无禁用品牌字符串（`rg -i hermes` 仅 spec 外链）

---

## 相关实现

| 组件 | 路径 |
|------|------|
| `MemoryStore` | `memory/src/agent/store.rs` |
| 安全扫描 | `memory/src/agent/scan.rs` |
| 配置 | `memory/src/config.rs` |
| `MemoryManager` / dispatch | `memory/src/session/manager.rs` |
| Frozen Snapshot | `agent/src/loop_.rs` |
| 工具注册 | `tools/src/builtins/memory_tools.rs` |
| Tauri refresh | `frontend/src-tauri/src/memory_commands.rs` |
| 入梦写回 | `memory/src/dreaming/mod.rs` |
