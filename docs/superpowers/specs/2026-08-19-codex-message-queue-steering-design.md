# Codex 风格消息队列与 Steering 设计

## 目标

让单线程 Agent / Plan / Ask 会话在任务运行期间继续接受用户输入，并采用与 Codex 相同的两阶段交互：按 Enter 默认加入 follow-up 队列；用户可在队列项上点击“调整方向”，将该消息显式注入当前活动任务。Shift+Enter 继续换行，MultiTask 保持独立任务语义。

## 已确认交互

- 空闲时按 Enter：立即开始普通新回合。
- 任务运行时按 Enter：加入 follow-up 队列，不自动 steering。
- 流式生成、工具执行和普通活动回合期间，输入框保持可编辑。
- 每条队列项显示文本摘要、附件数量，以及“调整方向”、删除、更多菜单。
- “调整方向”仅在存在可 steering 的活动普通回合时可用；点击后进入 pending-steer 状态。
- Steering 被接受后，队列项不立刻伪装成正式历史消息。后端提交该用户输入时发出正式事件，前端再将它写入聊天记录。
- Steering 因无活动回合或竞态失败时，该消息作为队首 follow-up 保留，并在当前回合结束后作为普通新回合发送。
- Review、Compact 等不可 steering 的回合也采用相同降级，不丢消息。
- 删除只删除尚未提交的 follow-up；已被后端接受的 pending steer 不允许删除。

## 架构

### 显式 Steering 契约

新增独立的 `SteerChat` gRPC 与 Tauri command，不复用 `start_chat`。请求携带 `session_id`、`expected_turn_id`、稳定的 `client_user_message_id`、文本及附件。服务端只允许注入匹配的活动普通回合，返回已接受的 turn id；无活动回合、turn id 不匹配和不可 steering 使用结构化拒绝原因。

该契约避免第二个 `start_chat` 与现有 `chat-stream-{sessionId}` 共用终止事件，也避免在活动回合刚结束的竞态中误开一个没有前端监听的新流。

### Pending steer 与正式提交事件

前端分别维护 follow-up 队列与 pending steers。点击“调整方向”后，队列项从 follow-up 移入 pending steers；后端在下一次模型采样前记录该用户输入时，通过主会话事件流发送包含 `client_user_message_id` 的用户输入提交事件。

前端收到提交事件后才从 pending steers 移除并渲染正式用户消息。后续 token、工具活动和最终回复继续属于当前活动回合，从而保持真实历史顺序。回合结束时仍未提交的 pending steer 回到 follow-up 队首。

### Follow-up 队列

本批复用现有前端 `QueuedFollowUp` 队列及编辑、排序能力，不引入 Codex `thread/queue/*` 的服务端持久化队列。队列按顺序每次只启动一个普通回合；当前回合自然完成或失败后出队。移除现有“空闲 60 秒自动取消当前任务并出队”行为，避免 follow-up 意外中断任务。

后续可独立迁移为服务端持久化队列，不改变本批的 UI 和 steering 契约。

## UI

队列卡片沿用现有 composer 上方布局，调整为截图中的紧凑单行结构：

- 左侧：队列图标和省略文本。
- 右侧主操作：“调整方向”，使用转向箭头图标。
- 右侧次操作：删除图标。
- 更多菜单：编辑、上移、下移；根据位置隐藏不可用项。

更多按钮必须打开真实菜单，不再构造后丢弃 actions。菜单支持点击外部和 Escape 关闭，并为按钮、菜单项提供可本地化的 title 与 aria-label。动画只使用现有轻量过渡并尊重 reduced motion。

Pending steer 在同一区域以“正在调整方向”状态显示，不提供删除和编辑，直到收到正式提交或降级回队列。

## 错误与竞态

- `expected_turn_id` 不匹配：不重试未知回合，消息回到 follow-up 队首。
- 无活动回合：消息回到队首，由空闲出队逻辑作为普通回合发送。
- 不可 steering：消息回到队首并显示非阻塞提示。
- 网络或 Tauri 调用失败：恢复原队列位置，保留附件和编辑内容。
- 重复点击“调整方向”：通过 item id 锁定，最多发出一次请求。
- 回合完成与 steering 响应交错：以正式用户输入提交事件为已提交依据；没有该事件的消息必须恢复。

## 测试与验证

- 纯前端调度测试：忙碌 Enter 入队、空闲 Enter 立即发送、Shift+Enter 换行。
- 队列状态测试：调整方向成功进入 pending、拒绝恢复队首、重复点击不重复提交。
- UI 测试：调整方向、删除、菜单打开及编辑/排序动作。
- Rust 测试：`SteerChat` 接受匹配普通回合，并对无活动回合、ID 不匹配、不可 steering 返回结构化拒绝。
- 流式测试：pending input 在下一次采样前记录并发出带 client message id 的正式提交事件。
- 完成前运行相关 Rust 定向测试、前端测试、`npx tsc --noEmit`、`cargo check` 和桌面生产构建。

## 非目标

- 本批不实现服务端持久化 follow-up 队列。
- 不改变 MultiTask 的独立 session 行为。
- 不允许 follow-up 自动取消当前任务。
- 不修改 Provider 协议或模型消息格式。
