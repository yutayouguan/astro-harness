# Astro 上下文压缩（工业级）

> 对齐 Cursor / Claude Code / Hermes / Agno 的最佳实践，在 Astro 现有架构上分两层落地。

## 总览

| 层级 | 机制 | 触发 | 对象 |
|------|------|------|------|
| **Run 内** | `maintain_tool_context` | 窗口占用 Soft/Medium/Hard + 条数兜底 | 单条 tool 结果的 Provider 视图 |
| **会话级** | `compact_and_split` + `/compact` | 用户手动或 UI 自动阈值 | 整段对话 → 新 session + 摘要 |

**不变量（全链路）**

- `messages.content` / DB `content`：**永远保留全文**（UI、FTS、审计）
- `messages.compressed_content`：仅 **发给模型的视图**（可 spill / prune / head-tail）
- 丢细节可恢复：spill 文件 + `session_search` + `file_ops read`

---

## Run 内流水线

每轮工具执行后，`multi_turn` 调用 `AgentLoop::maintain_tool_context()`：

```text
记录 tool 结果
  ├─ ≥16 KiB → 落盘 spill + 改写 compressed_content（Cursor 式）
  └─ 写入 session DB（全文）

每轮工具结束后 maintain_tool_context
  ├─ thrashing 已禁用？→ 跳过
  ├─ 未达 Soft 40% 且条数 <12？→ 跳过
  ├─ 尾部 protect_last_n 条消息内 → 不 prune
  ├─ Prune（廉价，无 LLM）
  │    Soft+：保护区外、>200 字符的 tool → [astro:tool-pruned]
  │    Hard 80%+：保护区外所有 tool → prune
  ├─ Head/Tail 压缩（无 LLM）
  │    仍过长 → [astro:compressed-tool-result stage≥N%]
  └─ thrashing 记录 + 仍 ≥85% → 建议 /compact
```

### 分阶段阈值（相对模型上下文窗口）

| 阶段 | 占用 | Head/Tail 预算 | Prune |
|------|------|----------------|-------|
| Soft | ≥ 40% | 1600 + 600 | >200 字符的旧 tool |
| Medium | ≥ 60% | 1100 + 500 | 同上 |
| Hard | ≥ 80% | 600 + 200 | 保护区外全部 tool |

窗口来源：`ChatRequest.context_window`（Tauri `model_meta`）→ `AgentLoop::set_context_window`；缺省 128k。

条数兜底：未压缩 tool ≥ **12** 时也会进入维护（用 Soft 级参数）。

---

## 四大能力对照

### 1. 主压缩

| 产品 | Astro 对应 |
|------|------------|
| Claude `/compact`、Hermes autocompact | **`compact_and_split`**（会话级，辅模型摘要） |
| Agno CompressionManager | **Run 内 head/tail + prune**（`agent/src/compression.rs`） |
| Cursor `/summarize` | 会话级摘要；Run 内靠 spill + prune 减压 |

Hard 阶段仍 ≥85% 占用时，日志提示用户执行 **`/compact`**（不自动拆 session，避免 mid-run 惊扰）。

### 2. 廉价预处理

| 产品 | Astro 实现 |
|------|------------|
| Claude microcompact | **Prune** `[astro:tool-pruned]` |
| Hermes clear old tool output | 同上 + Hard 全清保护区外 |
| Cursor 大结果不写进窗 | **Spill** ≥16 KiB → `sessions/tool_spills/{session}/{msg_id}.txt` |

代码：`common/src/tool_spill.rs`，记录时 `record_tool_result_with_id`。

### 3. 丢细节恢复

| 手段 | 说明 |
|------|------|
| DB `content` | 全文始终在库 |
| Spill 文件 | `~/.astro/sessions/tool_spills/...` |
| `session_search` | FTS 搜历史对话与 tool |
| `file_ops read` | 按 spill 路径 offset/limit 续读 |
| `compact_and_split` | 旧 session 完整保留，`parent_session_id` 谱系 |

Provider 视图中的 Recovery 提示已写入 spill/prune 模板。

### 4. 防死循环（Thrashing）

对齐 Claude Code「Autocompact is thrashing」：

- 每轮用户消息独立 `CompressionThrashingGuard`（`begin_user_turn` 重置）
- 连续 **3** 次维护后占用下降 <5%，或仍 ≥85% → **本轮不再自动维护**
- 避免：压完立刻又被巨型 tool 填满 → 无限压、无限调模型

---

## 关键代码路径

| 模块 | 路径 |
|------|------|
| 分阶段 + thrashing | `agent/src/compression.rs` |
| 维护入口 | `AgentLoop::maintain_tool_context` — `agent/src/runtime/mod.rs` |
| 多轮挂钩 | `agent/src/streaming/multi_turn.rs`（每轮工具后） |
| Spill | `common/src/tool_spill.rs` |
| Provider 视图 | `agent/src/prompt/messages.rs` → `compressed_content` 优先 |
| 会话压实 | `session/.../compact_and_split`，`frontend/.../compaction_commands.rs` |
| 辅模型压实 | `AuxiliaryTask::Compaction` → `compact_chat_session` |

---

## 配置与调参

| 常量 | 默认 | 位置 |
|------|------|------|
| `DEFAULT_SPILL_THRESHOLD_BYTES` | 16 KiB | `common/tool_spill.rs` |
| `PRUNE_MIN_CHARS` | 200 | `common/tool_spill.rs` |
| `DEFAULT_TOOL_RESULTS_LIMIT` | 12 | `agent/compression.rs` |
| Soft/Medium/Hard 比例 | 40% / 60% / 80% | `DEFAULT_COMPRESSION_STAGES` |
| `protect_last_n` | 20 | `AgentConfig` |
| `HARD_STAGE_RECOMMEND_COMPACT_RATIO` | 85% | `agent/compression.rs` |
| `MAX_CONSECUTIVE_LOW_GAIN` | 3 | thrashing |

---

## 与 Agno 课时关系

- Agno `CompressionManager`（逐条 LLM 摘要 tool）→ **后续**可接到 `AuxiliaryTask::Compaction`，替换 head/tail 启发式。
- 会话生命周期压实见 `docs/superpowers/specs/2026-07-14-session-compaction-design.md`。
- 课时总表：`docs/agno-lessons.md` §五。

---

## 运维建议

1. **长工具链桌面 Agent**：依赖窗口比例 + spill，勿把条数阈值调太低。
2. **仍频繁触顶**：用户主动 `/compact`，或调低自动压实 UI 阈值。
3. **调试**：`RUST_LOG=agent=debug` 查看 `tool context maintenance` / `thrashing` 日志。
4. **跨厂商**：`context_window` 由 Tauri 模型元数据注入；未知模型用 128k 估算。

---

## 后续（可选）

- [ ] Hard 阶段自动调用辅模型做 **mid-run 中间轮次摘要**（Hermes 阶段 3，仍不拆 session）
- [ ] `ContextUsage` 事件增加 `recommendCompact` 供前端 toast
- [ ] Gateway 85% 预压安全网（Hermes 双层之一）
