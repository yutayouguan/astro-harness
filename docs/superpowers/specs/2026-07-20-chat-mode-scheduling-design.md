# 聊天交互模式与调度设计

> 日期：2026-07-20  
> 状态：设计定稿；Step 1 / Step 2 已落地；Step 3（Plan gate + 模式切换授权）待做  
> 相关：[`chatMode.ts`](../../../frontend/src/lib/chat/chatMode.ts)、[`followUpQueue.ts`](../../../frontend/src/lib/chat/followUpQueue.ts)、编排 / delegate / worktree

## 1. 问题

当前四种模式（Agent / Plan / Ask / MultiTask）几乎只靠发送前拼接的 `chatModeHint`，差异很弱：

- 流式进行中再发消息会被直接挡住，**没有** Cursor / Claude Code 式的 follow-up 队列；
- MultiTask **没有**真正并行 spawn，也谈不上「互不影响」；
- Plan 无法硬限制写操作，模式切换也不会自动发生。

用户期望：

| 模式 | 调度 | 工具策略 | 模式切换 |
|------|------|----------|----------|
| **Ask** | 单线程 + 队列 | 尽量只读 | 手动为主 |
| **Plan** | 单线程 + 队列 | 可读可搜可推理，禁止副作用写 | 复杂任务可由 Agent 请求切入；计划确认后请求切回 Agent |
| **Agent** | 单线程 + 队列 | 全开 | 复杂任务可请求切 Plan；用户授权（倒计时 / 立即） |
| **MultiTask** | **每句话立即开新 task**，并行、互不影响 | 按 task 隔离 | 一般不与 Plan↔Agent 自动切换搅在一起 |

## 2. 核心思想

把「模式」拆成两层，而不是再堆更长 prompt：

1. **能力档位**：工具 gate（尤其 Plan 禁写）；
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
  1. **硬边界**：当前 turn 结束（`done` / 失败收束）；
  2. **软边界**：HITL、工具审批等天然停顿（后续可扩展）；
  3. **巡检点**（可选）：长任务周期性 checkpoint（后续）。

### 3.2 模式切换授权（后续 Step）

结构化请求，例如 `request_mode_switch({ to, reason, summary? })`：

- UI：授权条 + **可配置倒计时**（默认约 10s）+「立即切换」+「取消」；
- 取消 = 留在当前模式，并告知模型用户拒绝；
- 同轮禁止连环弹；流式结束再弹；
- Plan→Agent 时注入已确认计划摘要，避免失忆。

### 3.3 Plan 工具 gate（后续 Step）

后端 / 工具层对 Plan 禁用写文件、有副作用终端等；仅靠 hint 不够。

## 4. MultiTask（并行调度）

与其它模式**本质不同**：

- 用户每说一句 → **立即 spawn** 新 task，不等当前 task；
- 多个 task **互补影响**：靠隔离（独立 session fork / worktree / 文件域），不靠模型自觉；
- UI：不要与单条消息流混排乱序结果；用 Task 卡片 / 面板看各自进度与汇总。

实现上复用已有 orchestration、delegate、async spawn、git worktree。

**注意**：Step 1 已落地 follow-up 队列。Step 2 起 MultiTask 走「每消息独立 `session_id` 并行流」，同会话禁止并发（后端 pause 重入会 cancel）。

## 5. 落地顺序

1. **Step 1（已完成）**：Agent / Plan / Ask 的 follow-up 队列 + Queued UI；流结束后自动 dequeue 发送；
2. **Step 2（本迭代）**：MultiTask 每消息 spawn 独立 session + 并行监听 + Task 面板（MVP 暂不强制 worktree）；
3. **Step 3**：Plan 工具 gate + `request_mode_switch` 授权条（倒计时 / 立即 / 取消）；
4. **Step 4**：队列软边界巡检、task 汇总与 worktree 隔离增强。

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

## 7. 非目标（Step 2 仍不含）

- 同 `session_id` 多 turn 并发；
- 模式自动切换与倒计时授权；
- MultiTask 强制 worktree / delegate_async 作为主路径；
- 不改全局专家切换语义。
