# Agent Harness 文档一致性审查

> 审查日期：2026-09-04
>
> 审查范围：`docs/03-系统设计阶段/`、`docs/04-详细设计阶段/` 的当前正式设计文档
>
> 对齐基线：Astro 当前源码、Codex `e24190caa9ee..a0dcfe2ada3`、[术语统一规范](02-术语统一规范.md)

## 1. 审查结论

本轮已把 Agent 的统一定义收敛为：

> **Agent = Model + Harness**

Model 负责推理、决策、文本和工具调用意图；Harness 负责驱动 **Reason → Act → Observe → Reason** 循环，并承担上下文、工具执行、权限、安全、错误、恢复、观测和运行环境。

当前权威入口是：

- [Agent Harness 总体架构](../03-系统设计阶段/01-架构设计/11-Agent-Harness总体架构.md)；
- [Agent Harness 执行外壳详细设计](../04-详细设计阶段/01-核心引擎层/14-Agent-Harness执行外壳详细设计.md)；
- [Responses API 原生工具协议与 Astro 工具协议](../04-详细设计阶段/04-工具与扩展生态/05-Responses-API原生工具协议与Astro工具协议详细设计.md)；
- [Extension Manifest](../extensions.md)；
- [Realtime 子系统](../realtime-subsystem.md)；
- [2026-09-03 Codex 源码对齐](../更新说明/2026-09-03-Codex源码对齐.md)；
- [术语统一规范](02-术语统一规范.md)。

## 2. 已统一的不变量

| 领域 | 统一契约 | 状态 |
| --- | --- | --- |
| 运行层级 | `AstroThread → Session → SessionTask/RegularTask → TurnContext → StepContext → tool attempt` | 已实现 |
| 执行循环 | 每个 Step 进行模型采样；工具意图经 Harness 执行和回灌后进入下一 Step | 已实现 |
| Scaffold | `PromptContract` 管基础指令和动态上下文；原生工具 schema 经 `ResponsesRequest.tools` 独立传递 | 已实现 |
| Provider | Function / Freeform / Namespace / ToolSearch / WebSearch 是 wire protocol，不等同于本地注册表 | 已实现/部分实现 |
| 工具可见性 | Direct / Deferred / Hidden / ModelOnly 与授权正交 | 已实现 |
| 延迟激活 | `tool_search` 搜索 Deferred 工具和 MCP 元数据，激活结果在后续 Step 生效 | 已实现 |
| 工具编排 | 模型直接调用内置工具；Deferred 工具经 `tool_search` 发现 | 已实现 |
| 安全 | interaction mode、approval、hooks、sandbox 和 attempt-scoped network lease 共同裁决 | 已实现/部分实现 |
| 事件 | Core 产生 `EventMsg`；rollout 先记录，Server 再做 live projection | 已实现 |
| Usage | turn aggregate 用于计费，latest sampling 校准上下文；保留 Provider total 和报告状态 | 已实现 |
| 上下文 | provider reported/recomputed 优先，local estimate 保留分层解释与降级 | 已实现 |
| 恢复 | rollout 是稳定事件事实源，SessionStore 是查询投影 | 已实现 |
| 异步输入 | `request_user_input_async` 使用 durable questions；旧工具名不注册 | 已实现 |
| Thread 设置 | provider/backend/model/reasoning 经 `ThreadSettingsApplied` 支持冷/热恢复 | 已实现 |
| Usage checkpoint | `TokenUsageRecord` 保存 latest/cumulative/compaction；fork 不继承累计值 | 已实现 |
| Extension | turn 内快照冻结，reconcile 只在下一 turn 激活并报告受影响能力 | 已实现 |
| MCP event stream | process-owned manager、active 握手、attempt、有界队列和取消边界 | 基础已实现；opener/UI 待接线 |
| Persistent reasoning | 非空模型目录指令 + OpenAI 三层门禁；wire effort 为 `disabled` | 已实现 |
| Network header requirement | active leaf profile 解析、状态携带与 Debug 脱敏 | 已实现；CONNECT 不执行 TLS 内注入 |
| 子 Agent | V2 Agent Threads + Graph/mailbox/status；真实对话仍进入 Session 时间线 | 已实现 |
| 存储路径 | SQLite 职责库统一位于 `{base}/data/`；旧库只经启动迁移读取 | 已实现 |

## 3. 已消除或降级的陈旧表述

以下概念仍可能出现在历史段落、目标方案或迁移说明中，但不得再覆盖当前契约：

- 单一 `AgentLoop`/`round_loop` 拥有完整运行时；当前 `AgentLoop` 只是 `Session` 兼容别名；
- Core 内部 `EventBus`/`SessionEventHub` 是恢复事实源；当前事实源是 rollout；
- `delegate_task`、Supervisor、`agent_spawn` 是当前子 Agent 模型；当前唯一模型是 V2 Agent Threads；
- 单一 `agent.db` 承载全部状态；当前是 rollout + 多职责数据库；
- 固定“8 槽位 SystemPromptBuilder”代表当前 prompt；当前使用 `PromptContract`；
- Provider 工具、客户端 Function 和 MCP 工具共享单一暴露语义；当前必须区分 wire type、exposure 和 authorization。
- 把 `agent-mcp-server` 描述为当前 workspace crate；实际只实现 `agent-mcp` 客户端，Server 暴露仍是目标设计。

## 4. 有意保留的范围

以下内容不按当前实现批量改写：

- `docs/superpowers/plans/` 和 `docs/superpowers/specs/`：任务发生时的计划与规格快照；
- `docs/04-详细设计阶段/_v0.3规划/`：版本规划记录；
- 纯 UI 视觉、产品需求和业务流程中不涉及 Agent 运行契约的段落。

这些资料与当前源码冲突时，只能标为历史参考或目标设计。

## 5. 已知未闭环项

1. MCP event-stream manager 尚未连接具体 Server opener 与 Desktop 订阅 RPC。
2. HTTP CONNECT 代理不能观察 TLS 内 method/path，header injection 目前仅 parse/carry/redact。
3. Remote Extension Marketplace 需要 Astro 自有服务、认证与 bundle 信任根；当前只激活本地扩展。
4. Native voice helper 需要签名的三平台 runtime 产物；当前生产路径仍是 WebView/WebRTC/PCM WebSocket。
5. `agent-protocol::Op` 中部分控制分支仍返回 unsupported，不能因协议类型存在就宣称 runtime 已实现。
6. Provider-hosted `WebSearch` 有协议表示；Registry 当前 `web_search` 仍是客户端 Deferred Function。
7. 历史 Code Mode 运行时代码仍待清理，但已无模型配置入口，也不会注册 `exec` / `wait`。
8. 历史正文中的旧伪代码和目录已显式标为历史参考；新增现行设计不得继续引用已删除 crate。

## 6. 后续维护规则

1. 修改执行循环时，同时更新总体架构、详细设计、事件恢复和测试策略。
2. 修改 Provider 工具协议时，同时更新 Provider、工具系统、Codex 原生协议和历史回放。
3. 修改工具暴露或授权时，必须分别验证 discovery、activation、execution、approval、sandbox 和 hook。
4. 修改持久化路径时，以迁移测试和调用点为准，不只修改路径 helper。
5. 文档声称“已实现”前必须能定位到当前源码路径和至少一个验证入口。
