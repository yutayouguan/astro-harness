# 聊天交互模式与调度设计

> 日期：2026-07-20  
> 状态：设计定稿；Step 1–4 + 增强 + High/Medium 修复已落地；**模式说明已迁入 system prompt（不再拼进用户消息）**  
> 相关：[`chatMode.ts`](../../../apps/desktop/src/lib/chat/chatMode.ts)、[`followUpQueue.ts`](../../../apps/desktop/src/lib/chat/followUpQueue.ts)、[`interaction_mode.rs`](../../../tools/src/interaction_mode.rs)、[`parallelTasks.ts`](../../../apps/desktop/src/lib/chat/parallelTasks.ts)、编排 / delegate / worktree

## 1. 问题（历史背景）

早期四种模式（Agent / Plan / Ask / MultiTask）几乎只靠发送前拼接的 `chatModeHint` 进**用户消息**，差异很弱：

- 流式进行中再发消息会被直接挡住，**没有** Cursor / Claude Code 式的 follow-up 队列；
- MultiTask **没有**真正并行 spawn；
- Plan 无法硬限制写操作。

上述调度与工具 gate 已落地。另：`chatModeHint` 会污染会话历史 / UI / FTS，已废弃。

## 1b. 现行约定（2026-07 起）

| 项 | 约定 |
|---|---|
| 模式行为说明 | `InteractionMode::system_guidance()`（中英并列）写入 **system prompt**，不进用户 `content` |
| 前端 | 只传 `interactionMode`；**禁止**再拼 `[Mode: …]` |
| Tauri `start_chat` | **仅** `invoke("start_chat", { request: StartChatRequest })`；不保留扁平字段兼容 |
| 历史数据 | schema **v17** 打开库时剥离用户消息末尾 `\n\n---\n[Mode: …]`；不双写、不读时兼容 |

用户期望：

| 模式 | 调度 | 工具策略 | 模式切换 |
|------|------|----------|----------|
| **Ask** | 单线程 + 队列 | 尽量只读 | 手动为主 |
| **Plan** | 单线程 + 队列 | 可读可搜可推理，禁止副作用写 | 复杂任务可由 Agent 请求切入；计划确认后请求切回 Agent |
| **Agent** | 单线程 + 队列 | 全开 | 复杂任务可请求切 Plan；用户授权（倒计时 / 立即） |
| **MultiTask** | **每句话立即开新 task**，并行、互不影响 | 按 task 隔离 | 一般不与 Plan↔Agent 自动切换搅在一起 |

## 2. 核心思想

把「模式」拆成两层，而不是再堆更长用户消息：

1. **能力档位**：工具 gate（尤其 Plan 禁写）+ system 层 `system_guidance`；
2. **调度策略**：单线程队列 vs 多 task 并行。

另外：

- **专家（AgentPicker）** = 数据归属（MEMORY / 工作区），与模式正交；
- **模式** = 本轮怎么调度、能用什么工具；
- **MultiTask 的 task** = 一次独立执行单元（独立 `run_id` / 上下文 / 尽量独立 workspace）。

## 3. 单线程模式（Agent / Plan / Ask）

### 3.1 Follow-up 队列

参考 Cursor「1 Queued」：

- 当前 turn 流式中，用户再发送 → **入队**（不清空历史，不清空当前流）；
- UI：`N Queued` 列表，支持编辑 / 上移 / 删除；
- 出队时机（巡检点）：
  1. **硬边界**：当前 turn 结束（`done` / 失败收束）；`turnInFlight` 置 false 后自动 drain；
  2. **软边界**：HITL / 审批停顿期间仍可入队（不新开 `start_chat`）；interrupt 解除后若回合未 Done 仍不出队；
  3. **巡检点**：长任务无 token/tool 活动约 60s 且队列非空时，暂停当前回合并出队。

### 3.2 模式切换授权（Step 3 + 增强）

结构化工具 `request_mode_switch({ to, reason, summary? })`（`stop_after_tool_call`）：

- UI：授权条 + **倒计时**（默认 10s）+「立即切换」+「取消」；
- 取消 = 留在当前模式，并**自动发送拒绝说明**让模型续跑；
- 同轮禁止连环弹；流式结束再弹；
- Plan→Agent 批准后自动发送带 `summary` 的续聊句。

### 3.3 Plan / Ask 工具 gate（Step 3）

`interaction_mode` 经 `ChatRequest` 下传；`filter_schemas` + `check_tool_call`（含 `file_ops` 仅 read/list/search）。

### 3.4 Agent 会话级 worktree（增强）

Agent 模式下按 `session_id` 创建/复用 git worktree，经 `project_root` 下传；新会话时清理。MultiTask 仍为每 task 独立 worktree。

## 4. MultiTask（并行调度）

与其它模式**本质不同**：

- 用户每说一句 → **立即 spawn** 新 task，不等当前 task；
- 多个 task **互补影响**：靠隔离（独立 session fork / worktree / 文件域），不靠模型自觉；
- UI：不要与单条消息流混排乱序结果；用 Task 卡片 / 面板看各自进度与汇总。

实现上复用已有 orchestration、delegate、async spawn、git worktree。

**注意**：Step 1 已落地 follow-up 队列。Step 2 起 MultiTask 走「每消息独立 `session_id` 并行流」，同会话禁止并发（后端 pause 重入会 cancel）。

## 5. 落地顺序

1. **Step 1（已完成）**：Agent / Plan / Ask 的 follow-up 队列 + Queued UI；流结束后自动 dequeue 发送；
2. **Step 2（已完成）**：MultiTask 每消息 spawn 独立 session + 并行监听 + Task 面板（MVP 暂不强制 worktree）；
3. **Step 3（已完成）**：Plan/Ask 工具 gate + `request_mode_switch` 授权条（倒计时 / 立即 / 取消）；
4. **Step 4（已完成）**：队列软边界（`turnInFlight`）+ MultiTask 本地汇总 + git worktree 隔离。
5. **模式说明迁入 system（已完成）**：删除前端 `chatModeHint`；`system_guidance` 中英并列；schema v17 剥离历史后缀；`start_chat` 仅 `StartChatRequest` 包装。

## 6. Step 1 验收

- Agent / Plan / Ask：流式中可输入并发送 → 出现在 Queued；当前回复不中断；
- 流结束后自动发送队首；可删除 / 编辑队列项；
- 新开会话清空队列；
- MultiTask：本步不引入队列（避免与「立即干」语义冲突）；仍按现有 streaming 门闩，直到 Step 2。

## 6b. Step 2 验收

- MultiTask 下连发 2～3 条：各自独立 `session_id`，同时出 token，互不 cancel；
- 主时间线可见各 task 的用户句 + 助手气泡；输入框上方有 Task 面板（running / done / error）；
- 并发上限（默认 5）；达上限时 toast，不静默丢消息；
- Agent/Plan/Ask 队列行为不变；
- MVP 不强制 git worktree（并行写同一工作区仍有冲突风险，后续 Step 增强）。

## 6c. Step 3 验收

- Plan/Ask：schema 不含 terminal/code_exec/delegate/MCP；`file_ops(write)` 硬拦；
- `request_mode_switch` 流结束后弹授权条；倒计时或立即切换；取消不改模式；
- Plan→Agent 注入 summary；同轮只弹一次；
- MultiTask 仍传 `interaction_mode=multitask`（不做只读门禁）。

## 6d. Step 4 验收

- HITL 等待中可入队；解除后续跑未 Done 前不出队；Done 后自动出队；
- MultiTask 全部结束后显示汇总计数；「写入对话」插入本地 Markdown 汇总；「清除已结束」保留 running；
- MultiTask 在 git 仓内各 task 独立 `.worktrees/…` 作为 `project_root`；非 git 降级不报错；
- 不做周期性 checkpoint；不改 Agent 模式会话级 worktree。

## 6e. 增强验收（拒绝续跑 / checkpoint / Agent worktree）

- 取消模式切换后自动注入拒绝说明并 `start_chat` 续跑；
- 长任务空闲约 60s 且有排队：toast + 暂停回合 + 出队；
- Agent 模式同 session 复用 worktree；新会话清理；非 git 降级。

## 6f. High / Medium 修复验收

- 授权条展示期间禁止队列 drain；批准/拒绝后 `queueKick`；
- Plan/Ask 拦 `memory` / `pin_context` / `skills` 写操作；Ask 禁 `task_plan`；
- 切入 MultiTask 清空队列；离开 MultiTask 清理并行；开历史会话 reset 调度表面；
- MultiTask 始终可 Send；有并行时亦可 Stop（停全部）；Pause 仅主会话 streaming；
- HITL / `turnInFlight` 时保留 Stop，并清空 pending interrupts；
- 空 `project_root` → 后端清除；非 Agent 发送显式传空；
- mode pill：streaming / turnInFlight / HITL / 并行 running 时锁定；Plan/Ask 只读徽章 + 菜单说明；
- ~~`chatModeHint` 中英 i18n~~ → 已改为 `system_guidance` 中英并列（不进用户消息）；Plan/Ask 专用 placeholder；
- MultiTask 并行 HITL：`activity` → A2UI surface；`waiting` 状态；气泡内审批/澄清；`interrupt_resume` 走 task.sessionId；不写入主会话 pending（不误锁新并行发送）。

## 7. 非目标

- 同 `session_id` 多 turn 真正并发（仍靠 pause 互斥）；
- 不改全局专家切换语义；
- MultiTask 并行 HITL 跨刷新恢复（内存态；主会话有 store）；
- **不**为旧扁平 `start_chat` 参数或历史 `[Mode: …]` 用户后缀保留运行时兼容层（迁移时一次性删净）。
