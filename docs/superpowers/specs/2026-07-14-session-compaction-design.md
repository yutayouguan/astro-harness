# 会话压实与拆分续聊（Session Compaction P1b）

**日期:** 2026-07-14  
**状态:** 已批准（对话确认）  
**范围:** 上下文逼近上限时（或手动）生成摘要、结束旧会话、新会话挂 `parent_session_id` 续聊；前端无感切换  
**关联:** [`2026-07-13-session-store-design.md`](./2026-07-13-session-store-design.md)（P1 预留）、编辑截断 / 分支 fork、chat fallback 链  
**外部参考（正文不重复品牌名）:** 参考 Agent 的 context compression：辅模型摘要 + 会话拆分与谱系；社区亦有「同 session 原地压实」讨论，本设计**不采用**原地方案

## 命名约束

- 代码、UI、路径中不得出现外部参考项目品牌字符串。
- 推荐命名：`compact_and_split`、`compact_chat_session`、`end_reason = "compacted"`、`/compact`、`keep_tail_bubbles`。

## 目标

1. 长对话在逼近上下文窗口时能**继续聊**而不必用户手工删历史。  
2. **旧全文可回看**（只读归档），新会话带着摘要 + 近轮原文接着写。  
3. 自动阈值 + 手动入口；流式中禁止压实。  
4. 与现有 `parent_session_id`、`fork_session`、编辑/删除截断语义一致，不另起存储模型。

## 非目标

- 同一 `session_id` 内 truncate + 插入摘要（原地压实 / 方案 B）  
- Compaction 事件持久化日志 / OTEL  
- 把未完成 HITL interrupt 迁到新会话（压实前须无 pending interrupt）  
- 拷贝父会话 billing 累计到子会话  
- Gateway 多通道会话压实  

## 决策摘要

| 项 | 选择 |
|----|------|
| 存储策略 | **拆新会话**：`end_session(old, compacted)` + `create(new, parent=old)` |
| 触发 | 自动（默认 ~50% 上下文）+ 手动 `/compact` 或菜单「压实并继续」 |
| 摘要 | 优先辅/主模型压缩总结；失败则启发式剪贴，**仍拆 session** 并 toast 标明降级 |
| 新会话开局 | **摘要消息** + **最近 K 轮** user/assistant 原文（含其尾随 tool 行）；默认 K=3，可配 |
| UI | 无感切换 `sessionId`、重载气泡；toast 一次；侧栏保留旧会话并标已结束 |
| 流式 / interrupt | 进行中禁止；有 pending interrupt 时禁止 |
| 计费 | 新会话从 0；不拷贝父累计 |

---

## §1 架构与数据流

```text
触发（自动阈值 / 手动 compact）
  → 校验：非 streaming、无 pending interrupt、消息量足够
  → 读取旧会话消息 → 生成摘要（LLM → 失败则启发式）
  → SessionStore.compact_and_split:
       end_session(old, reason=compacted)
       create_session(new, parent=old, 继承 model 等元数据)
       append 摘要消息（role=user 或带明确标记的 system/user 约定，见 §3）
       拷贝最近 K 个聊天气泡及其尾随 tool 行
  → 返回 new_session_id + 摘要预览
  → 前端切换 sessionId / 重载 get_chat_history；toast
  → 后续 start_chat / hydrate 只走新会话
```

### 职责划分

| 层 | 职责 |
|----|------|
| `memory` SessionStore | 原子拆分：结束旧会话、建新、写摘要、拷贝尾部 |
| `agent` / Tauri | 触发、token 估算或前端 context%、LLM 摘要调用、组装启发式降级文本 |
| 前端 | 手动入口、自动观察 context 占用、切换会话、toast、侧栏状态 |

旧会话：`ended_at` / `end_reason="compacted"`；只允许读历史，禁止再 `append`（调用方校验）。

---

## §2 触发与阈值

### 自动

- **时机**：一轮完整结束（非流式中）；启动下一用户发送前也可二次检查。  
- **条件**：估算占用 ≥ `threshold × context_window`（默认 `threshold=0.50`）；聊天气泡数 ≥ 最小门槛（建议 ≥ 6，避免极短对话误触发）。  
- **来源**：优先用上一轮 API 回报的 usage / UI `contextUsagePercent`；缺失时用字符粗估。  
- **冷却**：自动失败或刚完成一次压实后短冷却（建议 60s），防止连环触发。

### 手动

- Slash：`/compact`（可选 focus 主题字符串，留给后续；MVP 可忽略 focus）。  
- 或消息操作区「压实并继续」。  
- 不受自动冷却限制（与参考实现对齐「强制重试」语义）。

### 失败

| 情况 | 行为 |
|------|------|
| LLM 摘要失败 / 空 | 启发式摘要（最近若干轮文本剪贴 + 截断）仍拆 session；toast「摘要为降级」 |
| DB 拆分失败 | 不切换 `sessionId`；toast 错误；旧会话保持可写 |
| streaming / interrupt | 直接拒，不调摘要 |

---

## §3 摘要与开局消息

### 摘要内容（LLM）

结构化中文/英文均可（跟会话语言），至少覆盖：目标、关键约束、已完成、进行中、相关路径/结论、建议下一步。单次摘要长度有上限（建议 ≤ 主模型窗口的 5% 或硬顶约 4k–8k 字符）。

### 启发式降级

拼接最近 N 条 user/assistant 文本（截断），前缀固定标记如 `[CONTEXT COMPACTION — fallback summary]`，便于模型识别。

### 落盘形状

新会话消息顺序：

1. **摘要**：一条 `user` 消息，正文以固定前缀开头，例如 `[CONTEXT COMPACTION]` + 摘要正文（避免破坏 user/assistant 交替；与现有 `validate_message_order` 兼容）。  
2. **尾部 K 轮**：自旧会话末尾向前数 K 个 user|assistant 气泡，连同其尾随 `tool` 行拷贝（复用 `fork_session` / bubble 边界逻辑）。

UI 可将摘要气泡样式化为「系统说明 / 已压实」卡片，但 role 仍为 user（或后续单独 `role` 扩展——**本期不做新 role**）。

默认 **K=3**，配置键建议：`session.compaction.keep_tail_bubbles`。

---

## §4 API 形状

### SessionStore

```text
compact_and_split(
  old_id,
  new_id,
  summary_text,
  keep_tail_bubbles: usize,
) -> Result<()>
```

语义：

- `old_id != new_id`；`new_id` 不得已存在。  
- 设置 old：`ended_at=now`，`end_reason="compacted"`。  
- 创建 new：`parent_session_id=old_id`，继承 `source`/`model` 等（billing 归零）。  
- 写入摘要 + 拷贝尾部。  
- 标题：可设 `"{old_title} · continued"` 或保留主题并给 old 加「已压实」展示由 UI 根据 `end_reason` 判断。

可选配套：

```text
end_session(id, end_reason)   # 若不存在则在本任务内实现
assert_session_writable(id)   # ended 则 bail
```

### Tauri

```text
compact_chat_session(
  session_id,
  keep_tail_bubbles?: number,
  focus?: string,           # MVP 可忽略
) -> { new_session_id, summary_preview, degraded: bool }
```

内部：拉历史 → 调摘要（agent/provider）→ `compact_and_split` → 返回。

### 配置（可后续落地 yaml）

```yaml
session:
  compaction:
    enabled: true
    threshold: 0.50
    keep_tail_bubbles: 3
    min_bubbles: 6
    cooldown_secs: 60
```

MVP 可用常量 + 后续再接 `config.yaml`。

---

## §5 前端与侧栏

1. 成功压实后：`setSessionId(newId)`，加载新会话历史，清 interrupt。  
2. Toast：成功 / 降级成功 / 失败。  
3. 侧栏：旧会话仍列出；根据 `ended_at`/`end_reason` 显示「已压实」徽章；点击可只读回放。  
4. 当前聊天区不展示旧会话全文（已换到新 id）；需要看全文时从侧栏打开旧会话。

---

## §6 测试计划

- `compact_and_split`：旧 ended + reason；新 parent；消息 = 摘要 + 尾 K（含 tool）；计数字段正确。  
- `keep_tail_bubbles=0`：仅摘要。  
- 对已 ended 会话再 `append` 应失败（若实现 writable guard）。  
- 摘要 LLM mock 失败 → 启发式路径仍产生新会话。  
- 前端：压实后 `sessionId` 变化；streaming 时按钮禁用。

## 验收标准

1. 手动 `/compact`（或等价）后，当前 UI 续聊使用新 `session_id`，模型能看到摘要与近轮。  
2. 侧栏可打开旧会话看压实前全文。  
3. 自动路径在模拟高占用时可触发，流式中不触发。  
4. LLM 失败时仍能拆分并降级提示。  
5. 命名无外部品牌字符串。

## 实现顺序建议

1. `end_session`（若缺）+ `compact_and_split` + 单测  
2. Tauri `compact_chat_session` + LLM/启发式摘要  
3. 前端手动入口 + 无感切换 + toast + 侧栏徽章  
4. 自动阈值挂接 + 冷却  
5. 文档（`docs` 短节）与本 spec 状态 → 已实现  

## 与方案 B 的边界

本期明确采用谱系拆分。若未来要做「同 id 原地压实」，另开 spec；`parent_session_id` 仍可用于 branch，不必删除列。
