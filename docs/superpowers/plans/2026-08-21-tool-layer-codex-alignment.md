# Tool Layer Codex Alignment Implementation Plan

> Astro 工具层与 Codex 上游对齐——引入 trait 契约、结构化审批、工具规格分类。

**Goal:** 在保持 28 个现有工具零改动的前提下，引入 Codex 对齐的 ToolExecutor trait、ToolName/ToolSpec 类型、ExecApprovalRequirement 声明式审批。

**Architecture:** 新增纯类型和 trait 定义，通过 LegacyToolAdapter 桥接现有工具；ToolRouter 快照 approval_requirement 供 force_serial 决策使用。

**Design:** `/Users/iswm/.claude/plans/fluttering-finding-newt.md`

---

### Batch A — 工具名与规格类型

- [x] **Step 1: 定义 ToolName（Plain/Namespaced）**
- [x] **Step 2: 定义 ToolSpec（Function/Namespace/Freeform）**
- [x] **Step 3: 定义 ExecApprovalRequirement（Skip/NeedsApproval/Forbidden）**
- [x] **Step 4: ToolEntry 新增 approval_requirement 字段**
- [x] **Step 5: 更新 types lib.rs re-export**
- [x] **Step 6: 添加 roundtrip/edge-case 测试**
- [x] **Step 7: cargo test -p types 通过**
- [x] **Step 8: 提交**

### Batch B — ToolExecutor trait

- [x] **Step 1: 定义 ToolExecutor trait（tool_name/spec/description/toolset/handle）**
- [x] **Step 2: 实现 LegacyToolAdapter 桥接 BuiltinToolHandler**
- [x] **Step 3: 导出 ToolExecutor/LegacyToolAdapter/ToolExecutorFuture**
- [x] **Step 4: 添加 EchoTool + adapter 保元数据测试**
- [x] **Step 5: cargo test -p tools 209 passed 无回归**
- [x] **Step 6: 提交**

### Batch C — 结构化审批声明

- [x] **Step 1: ToolRoute 增加 approval_requirement 字段**
- [x] **Step 2: ToolRouter 增加 approval_requirement() 和 any_may_require_approval()**
- [x] **Step 3: multi_turn.rs force_serial 决策增加 any_may_require_approval**
- [x] **Step 4: 添加 router approval snapshot 测试**
- [x] **Step 5: clippy 清洁 + 提交**

### Batch D — Namespace 工具示范（后续）

- [x] **Step 1: registry schemas_for_api 支持 Namespace ToolSpec 输出**
- [x] **Step 2: dispatch 层支持 namespace.name 路由**
- [x] **Step 3: cron 工具迁移为 cron.add/list/remove/enable/disable**
- [x] **Step 4: 模型 prompt guidance 适配 namespace 工具名**
- [x] **Step 5: 测试 + 提交**
