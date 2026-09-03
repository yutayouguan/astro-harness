# Turn 级修改与产物总结 Implementation Plan

> 实现状态：进行中（2026-09-03）。

**Goal:** 每个 Agent turn 结束后展示独立的修改与产物总结；审核读取该 turn 的冻结变更，撤销/重新应用只作用于该 turn，并且不覆盖用户或其他任务后续修改。

**Architecture:** 写工具产生结构化变更 → `TurnChangeSet` 在 turn 内聚合净结果 → rollout / SessionStore 持久化 → Desktop 渲染总结卡片与冻结 diff → Tauri 在内容校验后逆序撤销或正序重新应用。文件撤销与会话历史回滚保持独立。

## 不变量

- 只归属当前 turn 的修改，不能把项目相对 HEAD 的全部脏改动算入本轮。
- 记录首次修改前状态与最终状态；同一路径多次修改合并为一个净结果。
- 撤销前验证当前内容等于记录的 after 状态；重新应用前验证等于 before 状态。
- 多批次、多根目录撤销时逆序执行，重新应用时正序执行。
- 不可精确追踪的 shell、MCP 和外部 API 副作用不得宣称可撤销。
- 变更集必须可从 rollout 恢复；刷新或重启后仍能审核。

## 阶段

### Phase 1 — 结构化变更与持久化

- [x] 定义 turn-scoped 文件变更、产物和撤销能力协议。
- [x] `apply_patch` 返回精确 before/after 变更，并通过工具生命周期事件持久化。
- [x] 聚合同一路径的多次修改，生成稳定净变更与统计。
- [x] 未知写入路径降级为不可撤销，而不是生成不可信 diff。

### Phase 2 — 最终总结卡片与审核

- [x] 回合完成后在对应 assistant 回答底部展示总结卡片。
- [x] 默认展示前三个文件，支持展开和增删统计。
- [x] 审核面板读取冻结的 turn diff，不再读取项目相对 HEAD 的实时 diff。
- [x] 单文件、小 diff 支持快速查看；大文件和二进制提供明确降级状态。

### Phase 3 — 安全撤销与重新应用

- [x] 增加 turn change-set apply/revert 命令。
- [x] 使用 before/after 内容校验保护并发修改，冲突时不覆盖。
- [x] 撤销按批次逆序，重新应用按正序。
- [x] 返回 applied / conflicted 明细，并在卡片内保留本次 UI 状态。
- [x] 后续模型以实际工作区为事实源；不向历史注入伪造 user/developer 消息。

### Phase 4 — 覆盖面与验收

- [x] 本地产物纳入总结卡片；外部副作用标为不可撤销。
- [x] 覆盖主聊天、侧边聊天和恢复历史；多 workspace 由项目根路径校验隔离。
- [ ] 完成类型检查、Rust/前端测试、生产构建和最终代码审查。

## Codex 对照依据

- turn 内维护独立 diff tracker，累计为净 diff。
- `turn/diff/updated` 发送完整快照而非增量碎片。
- 总结卡片显示文件数、增删行、前三项和展开入口。
- Review 打开该 turn 的 diff；自动 AI review 是独立动作。
- Undo 使用保存的 patch 逆序应用，成功后可 Reapply；冲突与部分成功显式呈现。

## 实施决策

- 撤销状态不写入聊天正文，刷新后通过 before/after 快照检查真实工作区，恢复为 applied、reverted 或 conflict。
- 当前安全撤销只覆盖 `apply_patch` 的精确文本快照；terminal、code_exec、MCP 与外部 API 不显示可撤销承诺。
- 快照总量超过 512 KiB 时保留路径与统计、清除内容并标记不可撤销，避免把大文件塞入 rollout。
