# workflow

可视化工作流引擎 -- 定义 DAG 工作流、分层并行执行、变量插值，并以 SQLite 持久化运行历史。

## 核心职责

1. **工作流数据模型** -- 定义 43 种节点类型（跨 6 大类别）、边、位置、变量等完整工作流结构，JSON 序列化持久化到 `~/.astro/automation/workflows/workflows.json`。
2. **DAG 拓扑排序与并行执行** -- Kahn 算法分层拓扑排序，同层节点并行执行（`join_all`），支持条件分支跳过、过滤阻断、循环迭代、子工作流递归（最大深度 5）。
3. **变量上下文与插值** -- `{{var}}` 模板插值、嵌套 JSON 路径解析、条件表达式求值（比较 / 逻辑组合 / 取反）。
4. **运行记录管理** -- SQLite WAL 模式存储运行记录与步骤日志，支持查询、删除、自动清理（保留最近 500 条）。
5. **节点执行器注册表** -- 每种节点类型对应一个 `NodeExecutor` 实现，通过 `OnceLock` 全局注册表统一分发，支持重试与错误策略（abort / skip / fallback）。

## 模块结构

| 文件 | 职责 |
|------|------|
| `lib.rs` | crate 入口，导出所有子模块 |
| `model.rs` | 数据模型：`Workflow` / `WorkflowNode` / `WorkflowEdge` / `NodeType` (43 种) / `NodeCategory` (6 类) |
| `store.rs` | JSON 文件持久化：`WorkflowStore` CRUD、校验、自动备份（保留最近 5 个） |
| `error.rs` | 错误类型：`WorkflowError`（环路 / 无执行器 / 节点失败 / 超时 / 递归深度等）、`StoreError` |
| `run_db.rs` | SQLite 运行记录：`WorkflowRunDb`、`WorkflowRunRow` / `WorkflowStepLogRow`、DDL、清理 |
| `engine/mod.rs` | 执行引擎入口：`execute_workflow()`、层内并行、子工作流递归、循环体迭代、分支跳过 |
| `engine/dag.rs` | DAG 拓扑排序：`resolve_dag()` 返回 `DagPlan`（分层执行计划）、`detect_cycle()`、上下游查询 |
| `engine/executor.rs` | 执行器 trait：`NodeExecutor`（async）、`NodeResult`（Success / Branch / Filtered / Approved / PendingApproval） |
| `engine/variables.rs` | 变量上下文：`VariableContext` 插值、嵌套路径解析、条件表达式求值 |
| `nodes/mod.rs` | 执行器注册表：`executor_registry()` / `build_executor_registry()` 注册全部 41 种节点执行器 |
| `nodes/trigger.rs` | 触发器执行器：ManualTrigger / ScheduledTrigger / WebhookTrigger / EmailTrigger / FileWatchTrigger |
| `nodes/ai.rs` | AI 节点执行器：AiAgentTask / ParameterExtraction / QuestionClassification / KnowledgeRetrieval / Summarization / SentimentAnalysis / DocumentUnderstanding / VisionUnderstanding |
| `nodes/media.rs` | 多媒体节点执行器：ImageGeneration / VideoGeneration / MusicGeneration / TextToSpeech / SubtitleGeneration / VoiceClone / SpeechToText / ImageEdit / Translation |
| `nodes/control.rs` | 流程控制执行器：Conditional / MultiBranch / Filter / Merge / Loop / HumanApproval |
| `nodes/data.rs` | 数据处理执行器：SetFields / FormatText / Json / Code / Sort / Slice / Aggregate |
| `nodes/action.rs` | 动作执行器：HttpRequest / RunLoop / DelayWait / Output / AudioProcessing / SendNotification / FileIo / CustomLoop |

## 核心类型与 API

### 数据模型

- `Workflow` -- 完整工作流定义：id / name / nodes / edges / variables / enabled / agent_tool
- `WorkflowAgentTool` -- Agent 调用契约：`exposure` / `name` / `input_schema` / `output_description` / `examples` / `confirmation`
- `WorkflowNode` -- 节点：id / node_type / label / position / config / disabled
- `WorkflowEdge` -- 边：source / target / source_handle / target_handle
- `NodeType` -- 41 种节点类型枚举（snake_case 序列化）
- `NodeCategory` -- 6 大类别：Trigger / Ai / Media / FlowControl / DataProcessing / Action
- `NewWorkflow` -- 创建工作流输入
- `Position` -- 节点坐标 (x, y)

### 引擎

- `execute_workflow()` -- 执行一条工作流，返回 `WorkflowRunResult`
- `execute_workflow_with_provider_configs_and_run_id()` -- 使用调用方预分配的 run id 和运行时 Provider 凭证执行
- `DagPlan` -- DAG 分层执行计划：layers (可并行的节点 id 层) + trigger_node_id
- `resolve_dag()` -- Kahn 拓扑排序，禁用节点自动跳过，含环则报错
- `detect_cycle()` -- 环路检测，返回参与环的节点 id 列表
- `upstream_nodes()` / `downstream_from_handle()` -- 上下游查询
- `NodeExecutor` trait -- `async fn execute(&self, node, ctx) -> Result<NodeResult>`
- `NodeResult` -- Success(Value) / Branch(Vec) / Filtered / Approved / PendingApproval
- `VariableContext` -- 全局变量 + 节点输出管理、`interpolate()` / `resolve()` / `evaluate_condition()`
- `WorkflowRunResult` -- 执行结果：run_id / status / output / error / steps_executed

### 持久化

- `WorkflowStore` -- JSON 文件 CRUD：`list()` / `get()` / `create()` / `update()` / `delete()` / `save_workflow()` / `set_enabled()` / `update_agent_tool()` / `validate_workflow()`
- `WorkflowRunDb` -- SQLite 运行记录：`insert_run()` / `finish_run()` / `get_run()` / `list_runs()` / `delete_run()` / `insert_step_log()` / `finish_step_log()` / `list_step_logs()` / `prune_old_runs()`
- `WorkflowRunRow` / `WorkflowStepLogRow` -- 运行记录与步骤日志行

### 错误

- `WorkflowError` -- CycleDetected / NoExecutor / NodeExecFailed / Timeout / MaxDepthExceeded / SubWorkflowNotFound / MissingConfig
- `StoreError` -- ReadFailed / ParseFailed / WriteFailed / ValidationFailed / Db

## 与其他 crate 的关系

- **types** -- 共享基础类型
- **home** -- `~/.astro` 路径约定（`default_memory_dir()` 用于定位 workflows 目录）
- **providers** -- AI 节点执行器通过 providers crate 调用 LLM / 图像 / 语音等服务
- **前端** -- `apps/desktop` 使用 `@xyflow/react` 渲染流程图编辑器，通过 Tauri 命令调用本 crate
- **Agent 工具** -- 启用的 Workflow 按 `agent_tool` 契约投影为 `workflow` namespace 子工具；Deferred 仅在 `tool_search` 可用时可发现

## Agent 工具契约

```yaml
agent_tool:
  exposure: deferred # disabled | deferred | direct
  name: generate_weekly_report
  input_schema:
    type: object
    properties:
      topic:
        type: string
    required: [topic]
    additionalProperties: false
  output_description: 结构化周报
  examples:
    - topic: Astro
  confirmation: auto # auto | always
```

Registry 内部使用 `workflow__<workflow_id>` 作为稳定执行键，模型侧使用
`(namespace="workflow", name=<agent_tool.name>)`。每个 Step 都从 WorkflowStore 重建并冻结
Registry 快照；后续编辑或删除不会改变已签发调用的执行内容。

Agent 调用参数会先按 `input_schema` 校验，再原样注入 `trigger_input`。结果包含
`run_id/status/output/error/steps_executed`。超过 30 秒的执行转入后台，Agent 可用
`workflow.get_run` 查询或用 `workflow.cancel_run` 取消。AI、媒体、Action 和人工审批节点
会在运行整个 Workflow 前进入 Agent 审批链。

`get_run` / `cancel_run` 只允许访问当前 Agent Session 启动的 run；其他 Session 的 id
按不存在处理。Agent Step 还会冻结被引用的子工作流，执行期间的编辑/删除不会
改变已签发调用。`Code` 节点当前仍使用旧的本地子进程执行器，因此不允许通过
Agent 工具调用，直到它接入 Agent sandbox。Workflow HTTP/Webhook 对初始 URL 和每一次
重定向都执行 DNS/私网 SSRF 检查。

### 目录约定

```
~/.astro/
  workflows/
    workflows.json        # 全部工作流定义
    backups/              # 自动备份（最近 5 个）
    workflow.db           # 运行记录 SQLite (WAL)
```

## 测试运行命令

```bash
# 全部测试
cargo test -p workflow

# 单个测试函数
cargo test -p workflow linear_dag
cargo test -p workflow crud_roundtrip -- --nocapture
```
