# Astro 上下文压缩（工业级）

> 对齐 Cursor / Claude Code / Hermes / Agno 的最佳实践，在 Astro 现有架构上分两层落地。

## 总览

| 层级 | 机制 | 触发 | 对象 |
|------|------|------|------|
| **Run 内** | `maintain_tool_context` | 窗口占用 Soft/Medium/Hard + 条数兜底 | 单条 tool 结果的 Provider 视图 |
| **Run 内** | mid-run 辅模型摘要 | Hard ≥80%，每用户轮一次 | 中间轮次折叠为 handoff（不拆 session） |
| **Gateway** | 进 LLM 前预维护 | 占用 ≥85% | 再跑一轮 prune / head-tail + 建议 `/compact` |
| **会话级** | `compact_and_split` + `/compact` | 用户手动或 UI 自动阈值 | 整段对话 → 新 session + 摘要 |

**不变量（全链路）**

- `messages.content` / DB `content`：**永远保留全文**（UI、FTS、审计）
- `messages.compressed_content`：仅 **发给模型的视图**（可 spill / prune / head-tail）
- mid-run handoff：只折叠 **Provider 历史**（`provider_history()`），不改 DB、不拆 session
- 丢细节可恢复：spill 文件 + `session_search` + `file_ops read`

---

## Run 内流水线

每轮工具执行后，`multi_turn` 调用 `AgentLoop::maintain_tool_context()`，随后尝试 mid-run 摘要：

```text
记录 tool 结果
  ├─ ≥16 KiB → 落盘 spill + 改写 compressed_content（Cursor 式）
  └─ 写入 session DB（全文）

每轮工具结束后
  ├─ maintain_tool_context
  │    ├─ thrashing 已禁用？→ 跳过
  │    ├─ 未达 Soft 40% 且条数 <12？→ 跳过
  │    ├─ 尾部 protect_last_n 条消息内 → 不 prune
  │    ├─ Prune（廉价，无 LLM）
  │    │    Soft+：保护区外、>200 字符的 tool → [astro:tool-pruned]
  │    │    Hard 80%+：保护区外所有 tool → prune
  │    ├─ Head/Tail 压缩（无 LLM）
  │    │    仍过长 → [astro:compressed-tool-result stage≥N%]
  │    └─ thrashing 记录 + 仍 ≥85% → pending recommendCompact
  └─ maybe_apply_mid_run_summary（占用 ≥80%，辅模型 Compaction，每用户轮一次）

每轮 LLM 请求前（Gateway 85%）
  ├─ 占用 ≥85% → 再跑 maintain_tool_context
  ├─ 尝试 mid-run（若本轮尚未做）
  ├─ 用 provider_history() 作为发给模型的消息
  └─ 发出 ContextUsage（含 recommend_compact）
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

## Mid-run 中间轮次摘要

对齐 Hermes 阶段 3：**不拆 session**。

| 项 | 行为 |
|----|------|
| 触发 | 占用 ≥ **80%**，且本用户轮尚未尝试 |
| 辅模型 | `AuxiliaryTask::Compaction` |
| 写入 | `AgentLoop.mid_run_handoff`；`begin_user_turn` 清零 |
| Provider 视图 | 头 `protect_first=4` + `[astro:mid-run-summary]` + 尾 `protect_last_n` |
| DB / UI | 不变 |

代码：`agent/src/exec/mid_run_summary.rs`。

---

## 四大能力对照

### 1. 主压缩

| 产品 | Astro 对应 |
|------|------------|
| Claude `/compact`、Hermes autocompact | **`compact_and_split`**（会话级，辅模型摘要） |
| Agno CompressionManager | **Run 内 head/tail + prune**（`agent/src/compression.rs`） |
| Cursor `/summarize` | 会话级摘要；Run 内靠 spill + prune 减压 |
| Hermes 中间轮次摘要 | **mid-run**（不拆 session） |

Hard / Gateway 仍 ≥85% 占用时，经 `ContextUsage.recommend_compact` 提示用户 **`/compact`**（流式中不自动拆 session）。

### 2. 廉价预处理

| 产品 | Astro 实现 |
|------|------------|
| Claude microcompact | **Prune** `[astro:tool-pruned]` |
| Hermes clear old tool output | 同上 + Hard 全清保护区外 |
| Cursor 大结果不写进窗 | **Spill** ≥16 KiB → `sessions/tool_spills/{session}/{msg_id}.txt` |
| Hermes Gateway 85% | **进 LLM 前**再跑 `maintain_tool_context` |

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
| mid-run 摘要 | `agent/src/exec/mid_run_summary.rs` |
| 多轮挂钩 | `agent/src/streaming/multi_turn.rs`（Gateway 前 + 工具后） |
| Spill | `common/src/tool_spill.rs` |
| Provider 视图 | `provider_history()` / `compressed_content` 优先 |
| 会话压实 | `session/.../compact_and_split`，`frontend/.../compaction_commands.rs` |
| 辅模型压实 | `AuxiliaryTask::Compaction` → `compact_chat_session` / mid-run |
| 前端提示 | `ContextUsage.recommendCompact` → toast（60s 冷却） |

---

## 配置与调参

| 常量 | 默认 | 位置 |
|------|------|------|
| `DEFAULT_SPILL_THRESHOLD_BYTES` | 16 KiB | `common/tool_spill.rs` |
| `PRUNE_MIN_CHARS` | 200 | `common/tool_spill.rs` |
| `DEFAULT_TOOL_RESULTS_LIMIT` | 12 | `agent/compression.rs` |
| Soft/Medium/Hard 比例 | 40% / 60% / 80% | `DEFAULT_COMPRESSION_STAGES` |
| `protect_last_n` | 20 | `AgentConfig` |
| `MID_RUN_SUMMARY_RATIO` | 80% | `agent/exec/mid_run_summary.rs` |
| `HARD_STAGE_RECOMMEND_COMPACT_RATIO` / Gateway | 85% | `agent/compression.rs` |
| `MAX_CONSECUTIVE_LOW_GAIN` | 3 | thrashing |

---

## 与 Agno 课时关系

- Agno `CompressionManager`（逐条 LLM 摘要 tool）→ **后续**可接到 `AuxiliaryTask::Compaction`，替换 head/tail 启发式。
- 会话生命周期压实见 `docs/superpowers/specs/2026-07-14-session-compaction-design.md`。
- 课时总表：`docs/agno-lessons.md` §五。

---

## 运维建议

1. **长工具链桌面 Agent**：依赖窗口比例 + spill，勿把条数阈值调太低。
2. **仍频繁触顶**：用户主动 `/compact`，或调低自动压实 UI 阈值；关注 `recommendCompact` toast。
3. **调试**：`RUST_LOG=agent=debug` 查看 `tool context maintenance` / `gateway pre-maintain` / `mid-run summary` / `thrashing` 日志。
4. **跨厂商**：`context_window` 由 Tauri 模型元数据注入；未知模型用 128k 估算。

---

## 已落地增强

- [x] Hard 阶段自动调用辅模型做 **mid-run 中间轮次摘要**（仍不拆 session）
- [x] `ContextUsage` 事件增加 `recommendCompact` 供前端 toast
- [x] Gateway 85% 预压安全网（进 LLM 前 `maintain_tool_context`）
