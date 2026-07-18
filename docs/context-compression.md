# Astro 上下文压缩（工业级）

> 对齐 Cursor / Claude Code / Hermes / Agno 的最佳实践，在 Astro 现有架构上分两层落地。

## 总览

| 层级 | 机制 | 触发 | 对象 |
|------|------|------|------|
| **Run 内** | `maintain_tool_context` | 窗口占用 Soft/Medium/Hard + 条数兜底 | 单条 tool：prune / **LLM 摘要** / head-tail 回退 |
| **Run 内** | mid-run 辅模型摘要 | Hard ≥80%，每用户轮一次 | 中间轮次折叠为 handoff（不拆 session） |
| **Gateway** | 进 LLM 前预维护 | 占用 ≥85% | 再跑一轮 prune / LLM 摘要 / head-tail + 建议 `/compact` |
| **会话级** | `compact_and_split` + `/compact` | **仅用户手动**（`/compact` / 菜单）；前端不自动拆 session | 整段对话 → 新 session + 摘要 |

**不变量（全链路）**

- `messages.content` / DB `content`：**永远保留全文**（UI、FTS、审计）——也称 **会话原文 / DB 视图**
- `messages.compressed_content` + `provider_history()`：仅 **Provider 视图**（发给上游 API 的消息；可 spill / prune / LLM 摘要 / head-tail / mid-run 折叠）
- mid-run handoff：只折叠 Provider 视图，不改 DB、不拆 session
- 丢细节可恢复：spill 文件 + `session_search` + `file_ops read`

### 术语：Provider 视图（不要叫「model 视图」）

压缩改的是「出站给 Provider 的消息」，不是 UI/DB 里的聊天记录。正式术语统一为 **Provider 视图**。

| 说法 | 是否推荐 | 说明 |
|------|----------|------|
| **Provider 视图** | ✅ 正式名 | 对齐 `provider_history()`、`to_provider_messages`、代码里的 *provider-facing* |
| 发给模型的消息 / 发给模型的视图 | ✅ 可作口语解释 | 对外说明「模型实际读到什么」时可用；文档里宜写成「Provider 视图（发给模型的消息）」 |
| model 视图 | ❌ 勿作正式名 | 易与「模型内部表示」「会话级摘要」混淆；仓库也未把它当术语 |

对照关系：

```text
用户在 UI 看到的 / FTS 搜到的     = 会话原文（content / DB）
真正 POST 给 OpenAI/Gemini/… 的 = Provider 视图
  ├─ 单条 tool：compressed_content（spill / prune / LLM 摘要 / head-tail）
  └─ 整段历史：provider_history()（可含 mid-run handoff 折叠）
```

一句话：**对内写 Provider 视图；需要解释时补一句「即发给模型的消息」。**

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
  │    ├─ 仍需压缩的 tool（多为保护区尾部）
  │    │    ├─ Agno 式辅模型摘要 → [astro:llm-compressed-tool-result]（每轮最多 6 条）
  │    │    └─ 失败 / 无 Compaction 目标 / 超额 → head/tail 启发式
  │    └─ thrashing 记录 + 仍 ≥85% → pending recommendCompact
  └─ maybe_apply_mid_run_summary（占用 ≥80%，辅模型 Compaction，每用户轮一次）

每轮 LLM 请求前（Gateway 85%）
  ├─ 占用 ≥85% → 再跑 maintain_tool_context
  ├─ 尝试 mid-run（若本轮尚未做）
  ├─ 用 provider_history() 作为 Provider 视图（发给上游 API）
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
| Agno CompressionManager | **Run 内 LLM 逐条摘要**（失败回退 head/tail + prune） |
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
| 分阶段 + thrashing + head/tail 回退 | `agent/src/compression.rs` |
| 维护入口 | `AgentLoop::maintain_tool_context`（async）— `agent/src/runtime/mod.rs` |
| 逐条 LLM 摘要 | `agent/src/exec/tool_llm_compress.rs` |
| mid-run 摘要 | `agent/src/exec/mid_run_summary.rs` |
| 多轮挂钩 | `agent/src/streaming/multi_turn.rs`（Gateway 前 + 工具后） |
| Spill | `common/src/tool_spill.rs` |
| Provider 视图 | `provider_history()` / `compressed_content` 优先 |
| 会话压实 | `session/.../compact_and_split`，`frontend/.../compaction_commands.rs` |
| 辅模型压实 | `AuxiliaryTask::Compaction` → `compact_chat_session` / mid-run |
| 前端提示 | `ContextUsage.recommendCompact` → toast（60s 冷却） |

---

## 配置与调参

主配置段：`config.yaml` 的 **`compression:`**（`memory::CompressionConfig`）。偏好设置 →「上下文与压缩」可读写；Agent / Gateway / mid-run / `/compact` keep_tail **热读**该段。

```yaml
compression:
  enabled: true
  soft_ratio: 0.40
  medium_ratio: 0.60
  hard_ratio: 0.80
  soft_max_chars: 2400
  soft_head_chars: 1600
  soft_tail_chars: 600
  medium_max_chars: 1800
  medium_head_chars: 1100
  medium_tail_chars: 500
  hard_max_chars: 900
  hard_head_chars: 600
  hard_tail_chars: 200
  tool_results_limit: 12
  mid_run_summary_ratio: 0.80
  recommend_compact_ratio: 0.85
  protect_last_n: 20
  protect_first_messages: 4
  thrashing_min_gain_ratio: 0.05
  thrashing_max_consecutive: 3
  keep_tail_bubbles: 3
```

| 项 | 默认 | 来源 |
|------|------|------|
| Soft/Medium/Hard 比例与字符预算 | 见上表 | `compression:` → `ToolCompressionManager::from_config` |
| `tool_results_limit` | 12 | 同上 |
| `mid_run_summary_ratio` | 80% | mid-run |
| `recommend_compact_ratio` / Gateway | 85% | Gateway 预维护 + `ContextUsage` |
| `protect_last_n` / `protect_first_messages` | 20 / 4 | prune / mid-run 折叠 |
| thrashing | 5% / 连续 3 次 | `CompressionThrashingGuard` |
| `keep_tail_bubbles` | 3 | 手动 `/compact` |
| `DEFAULT_SPILL_THRESHOLD_BYTES` | 16 KiB | 代码常量（UI 未暴露） |
| `PRUNE_MIN_CHARS` | 200 | 代码常量 |
| `MAX_LLM_TOOL_COMPRESS_PER_PASS` | 6 | 代码常量 |
| `MAX_LLM_INPUT_CHARS` | 24_000 | 代码常量 |

UI：占用查看仍在聊天右栏 Context Explorer；辅模型页的 `auxiliary.compaction` 只选摘要模型，阈值在偏好设置。

---

## 与 Agno 课时关系

- Agno `CompressionManager`（逐条 LLM 摘要 tool）→ **已落地**：`tool_llm_compress` + `maintain_tool_context`（无目标 / 失败回退 head/tail）。
- 会话生命周期压实见 `docs/superpowers/specs/2026-07-14-session-compaction-design.md`。
- 课时总表：`docs/agno-lessons.md` §五。

---

## 运维建议

1. **长工具链桌面 Agent**：依赖窗口比例 + spill，勿把条数阈值调太低。
2. **仍频繁触顶**：用户主动 `/compact`；关注 `recommendCompact` toast（仅建议，前端不会自动拆 session）。上下文条/详情只展示后端 `context_usage` 与真实 `context_window`，不按 128K 估算。
3. **调试**：`RUST_LOG=agent=debug` 查看 `tool context maintenance` / `tool LLM compress` / `gateway pre-maintain` / `mid-run summary` / `thrashing` 日志。
4. **跨厂商**：`context_window` 由 Tauri 从 models 缓存 / LiteLLM 注入；未知模型 agent 侧缺省 128k，前端未知则显示「—」而非假百分比。
5. **辅模型**：配置 `AuxiliaryTask::Compaction` 目标；未配置时自动退回 head/tail，不影响主对话。

---

## 已落地增强

- [x] Hard 阶段自动调用辅模型做 **mid-run 中间轮次摘要**（仍不拆 session）
- [x] `ContextUsage` 事件增加 `recommendCompact` 供前端 toast
- [x] Gateway 85% 预压安全网（进 LLM 前 `maintain_tool_context`）
- [x] Agno 式 **逐条 LLM 摘要 tool**（失败回退 head/tail）
