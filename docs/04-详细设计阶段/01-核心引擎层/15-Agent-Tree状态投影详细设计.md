# Agent Tree 状态投影详细设计

> 状态：已实现
> 更新日期：2026-09-03
> 适用范围：`agent-subagents`、Tauri Subagent 控制面、Desktop Agent Tree

## 1. 定位与对齐边界

Astro 的 Agent Tree 是单个根会话下的持久协作树。它负责回答三个问题：

1. 根任务派生了哪些 Agent Thread，它们的父子关系是什么；
2. 每个节点当前处于什么生命周期状态，是否有未读活动；
3. 整棵树当前继承哪一个根级运行策略。

Codex 的 subagent 运行时同样把根任务选择的运行策略沿整棵 agent tree 传播。本轮将
`service_tier` 纳入 Astro 的根级控制面和只读树快照，使运行时继承与桌面可观测状态一致。

Codex TUI 另有 daemon-wide Agents Overview，可聚合最近及已加载的多个根任务。Astro
当前仍以会话为导航单元，Agent Tree 只展示当前 `root_session_id` 的后代；两者是产品
信息架构差异，不把全局任务列表伪装成单树能力。

## 2. 权威数据源

| 数据 | 权威源 | 持久性 |
| --- | --- | --- |
| 节点身份、父子边、状态 | `AgentGraphStore` / `subagents-v2.db` | 持久 |
| 节点真实对话 | `SessionStore` / `state.db` | 持久 |
| 活动序列号 | root-scoped `ActivityBus` | 进程内，供快照与增量事件衔接 |
| 根服务层级 | root-scoped `AgentControl` | 当前运行时快照，不写入 Agent Graph |
| 未读标记 | Desktop `AgentTreeState` | 客户端投影 |

`AgentGraphStore::snapshot()` 只读取持久图，因此把 `root_service_tier` 初始化为
`None`。`AgentControl::snapshot_with_after_cursor()` 在返回控制面快照前，用当前根运行时
的值补齐该字段。这样不会把一次运行请求的路由偏好误建模为耐久图结构。

## 3. 快照契约

```rust
pub struct AgentTreeSnapshotV2 {
    pub root_thread_id: String,
    pub threads: Vec<AgentThreadV2>,
    pub activity_sequence: u64,
    pub root_service_tier: Option<String>,
}
```

字段语义：

- `root_thread_id`：树的稳定根身份，也是 Tauri 查询的作用域。
- `threads`：包含根节点及所有已持久化后代；`Shutdown` 节点保留供桌面归档查看。
- `activity_sequence`：快照覆盖到的活动游标；客户端只重放更大的事件序号。
- `root_service_tier`：根任务当前选中的服务层级。所有后代继承同一个值；仅
  OpenAI/Codex backend 将它写入 Provider 请求。

新增字段采用 `serde(default)` 和 `skip_serializing_if = "Option::is_none"`。旧后端不返回
该字段时，Desktop 必须兼容为 `null`；新后端没有 tier 时也不产生多余 wire 字段。

## 4. 快照与事件衔接

```text
Desktop 注册 session_event listener
  -> 开始缓冲 AgentThreadChanged
  -> list_subagent_threads(root_session_id)
  -> AgentControl 读取 activity cursor
  -> AgentGraphStore 事务读取线程图
  -> AgentControl 注入 root_service_tier
  -> Desktop 只重放 sequence > snapshot.activity_sequence 的缓冲事件
  -> 后续事件直接进入 reducer
```

监听先于快照建立，避免“读取快照期间恰好发生状态变化”的窗口。事件只修改节点及
`activity_sequence`，不得覆盖 `root_service_tier`；标记节点已读时同样保留根级元数据。
切换根会话会递增 generation，旧请求和旧 listener 的迟到结果会被拒绝。

## 5. Desktop 投影

`normalizeAgentTreeSnapshot()` 同时接受 Rust/Tauri 的 `snake_case` 和前端
`camelCase`，输出统一的 `AgentTreeSnapshot`。`projectTree()` 完成以下工作：

- 按 canonical path 构建 `byPath`、`roots` 与 `children`；
- 对孤儿节点保持稳定展示，父节点抵达后重新挂载；
- 保留归档和未读状态；
- 在快照、实时事件与标记已读三个路径中保持 `rootServiceTier`。

右侧摘要中的 `SubagentActivityBar` 在树非空时同时展示状态汇总与根服务层级，例如：

```text
2 个运行中 · 根服务层级 priority
```

服务层级只显示实际快照值，不根据模型名或账号状态推断。

## 6. 不变量

1. 整棵树只有一个根服务层级，子节点不得自行扩大或覆盖该值。
2. 自定义 Agent 的模型、reasoning effort 和 sandbox 规则继续独立继承或收窄；
   `service_tier` 不改变权限。
3. `AgentThreadChanged` 必须按严格递增序号归约；重复和乱序事件不得回退状态。
4. Agent Graph 与真实 Session 时间线分库存储，树快照不复制消息正文。
5. `Shutdown` 只影响可执行性；节点仍可作为只读历史存在。
6. 缺少 `root_service_tier` 是合法兼容状态，Desktop 表示为 `null`。

## 7. 验证矩阵

| 层 | 覆盖点 |
| --- | --- |
| Rust model | 新字段序列化与可选语义 |
| `AgentControl` | clone 共享根 tier，快照读取同一值 |
| Store | 持久图快照不伪造运行时 tier |
| TypeScript normalizer | snake/camel case 与 nullable 兼容 |
| Tree reducer | 实时事件和 mark-read 后保留根 tier |
| UI contract | `App -> ChatRightPanel -> SubagentActivityBar` 贯通 |
| 全量桌面测试 | 612 项通过；TypeScript 无类型错误 |

## 8. 后续边界

- “需要用户输入/审批”目前属于 Session/交互事件，不强行压缩进六态 Agent 状态机。
- 全局多根任务搜索、重命名和按状态分组属于 daemon-wide task overview，若产品需要应以
  独立聚合查询实现，而不是放宽 `list_subagent_threads` 的 root scope。
- Codex Goal 与整树 token budget 尚无 Astro 等价协议；在事件、恢复和计费契约齐备前，
  Agent Tree 不暴露虚假的 goal 或预算字段。

## 9. 参考

- OpenAI Codex Subagents：<https://learn.chatgpt.com/docs/agent-configuration/subagents>
- Codex 对照基线：`e24190caa9..a0dcfe2ada`
- Codex 根服务层级提交：`dc2ccc6843`（Make subagents follow the root service tier）
- Astro Agent Tree 实现：`crates/agent-subagents/src/control.rs`、
  `apps/desktop/src/hooks/chat/subagentTree.ts`
