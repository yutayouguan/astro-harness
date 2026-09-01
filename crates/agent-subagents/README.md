# agent-subagents (package: `subagents`)

Codex 风格一等公民子 Agent 线程系统：管理持久化线程元数据、自定义 Agent 配置和运行时生命周期控制。实际的模型/工具循环由 `agent::exec::subagents` 实现，以避免与主运行时的循环依赖。

## 核心职责

1. **线程图持久化** — `AgentGraphStore` 基于 SQLite WAL 管理线程树（`subagents-v2.db`）：线程预留/提交/回滚、状态事件记录、派生边管理、邮箱消息入队/投递/回滚。
2. **内存注册表与配额** — `AgentRegistry` 维护 path-to-thread 映射、RAII 派生预留（`SpawnReservation`）和执行许可（`ExecutionPermit`），强制深度/数量/并发限制。
3. **根级共享控制** — `AgentControl` 聚合存储、注册表、活动总线和运行时句柄，提供派生、消息、邮箱、等待、中断和关闭子树的完整 API。
4. **活动事件总线** — `ActivityBus` 以有界环形缓冲发布活动事件（派生、邮箱、状态变更、边关闭、主任务转向），支持 watch 通知的异步等待和间隙检测。
5. **自定义 Agent 配置** — 从 `~/.astro/agents/` 和项目 `.astro/agents/` 加载 TOML Agent 定义（含 MCP 服务器、Skills、沙箱模式），内置 default/worker/explorer 三类 Agent。

## 模块结构

| 文件 | 职责 |
|---|---|
| `lib.rs` | 模块聚合与公共 re-export |
| `model.rs` | V2 领域类型定义：`AgentStatusV2`（6 种生命周期状态）、`AgentThreadV2`（持久化投影）、`ThreadReservation`（预留身份）、`RunnerEvent`（运行器事件）、`AgentTreeSnapshotV2`（线程树快照）、Spawn/Message/Wait/Interrupt/List 请求与结果类型 |
| `store.rs` | `AgentGraphStore` SQLite 存储句柄：线程 CRUD、状态事件原子写入、邮箱委托、运行时描述符、进程崩溃恢复（`recover_running_as_interrupted`）、终端错误截断 |
| `registry.rs` | `AgentRegistry` 内存注册表：path/thread_id 双向映射、`SpawnReservation`（RAII 预留/提交/回滚）、`ExecutionPermit`（并发执行槽位）、`Limits`（配额） |
| `control.rs` | `AgentControl` 根级共享控制器：聚合 store/registry/activity/runtimes；派生预留（含关闭前缀冲突检查）、消息入队、邮箱排空、等待活动、运行时注册/注销、子树关闭屏障 |
| `activity.rs` | `ActivityBus` 活动总线：有界环形缓冲（1024 条）、单调序号、`ActivityCursor` 增量拉取、`ActivityObservation`（含间隙检测）、`ModelWaitSignal`（Codex 模型等待过滤） |
| `mailbox.rs` | 邮箱消息层：`MailboxKind`（Message/Followup/Steer/Result/Status）、`NewMailboxMessage`/`MailboxMessage`、幂等入队（`idempotency_key`）、投递标记、回滚删除 |
| `config.rs` | Agent 配置加载：`AgentDefinition`（TOML 定义含 MCP/Skills/sandbox_mode）、`AgentCatalog`（内置 + 自定义）、`AgentsSettings`（全局设置含并发限制）、`resolve_agent`（模型/沙箱解析，沙箱权限只可收窄不可扩大） |
| `migration.rs` | Schema 迁移：v1 归档到 `historical_agent_threads_v1`、v2 到 v4 增量升级、`agent_runtime_descriptors` 恢复状态标记 |
| `path.rs` | `AgentPath` 规范化路径：`/root/research/citations` 格式、段校验（小写 ASCII + 数字 + 下划线）、parent/child/resolve/starts_with/depth |

## 核心类型与 API

```rust
// 线程状态
pub enum AgentStatusKind { PendingInit, Running, Interrupted, Completed, Errored, Shutdown }
pub enum AgentStatusV2 { PendingInit, Running, Interrupted, Completed{..}, Errored{..}, Shutdown }
pub struct AgentThreadV2;                // 持久化线程投影
pub struct ThreadReservation;            // 启动前的预留身份
pub struct AgentRuntimeDescriptorV2;     // 恢复运行时的最小描述符
pub struct AgentTreeSnapshotV2;          // 线程树只读快照

// 运行器事件
pub enum RunnerEvent { TurnStarted, TurnCompleted, TurnInterrupted, TurnErrored, RuntimeTerminated }

// 请求/结果
pub struct SpawnAgentV2Request;          // deny_unknown_fields 的模型可见派生请求
pub struct MessageAgentV2Request;        // 消息发送请求
pub struct WaitAgentV2Request;           // 等待请求
pub struct InterruptAgentV2Request;      // 中断请求
pub struct ListAgentsV2Request;          // 列表请求

// 存储
pub struct AgentGraphStore;              // SQLite 图存储（WAL 模式）
pub struct StoredStatusEvent;            // 已持久化的状态事件

// 注册表
pub struct AgentRegistry;                // 内存注册表（路径映射 + 配额）
pub struct Limits;                       // 资源配额（max_threads/max_depth/max_running）
pub struct SpawnReservation<'a>;         // RAII 派生预留（drop 自动回滚）
pub struct ExecutionPermit<'a>;          // RAII 执行许可（drop 自动释放）

// 控制器
pub struct AgentControl;                 // 根级共享控制器
pub struct AgentSpawnReservation<'a>;    // 控制器级派生预留（含关闭前缀检查）
pub struct CloseAdmissionGuard;          // 子树关闭屏障
pub struct AgentRuntimeHandle;           // 运行时中断/终止回调
pub struct RuntimeHandleRegistry;        // 线程 ID -> 运行时句柄注册表
pub struct AgentThreadControl;           // 单 Turn 取消信号

// 活动总线
pub struct ActivityBus;                  // 有界环形缓冲活动总线
pub struct ActivityCursor(pub u64);      // 单调递增的活动序号游标
pub enum AgentActivityKind { Spawned, Mailbox, StatusChanged, EdgeClosed, MainSteer }
pub struct AgentActivity;                // 带序号的活动事件
pub enum ActivityObservation { Activity, Gap, TimedOut }

// 邮箱
pub enum MailboxKind { Message, Followup, Steer, Result, Status }
pub struct NewMailboxMessage;             // 待入队消息
pub struct MailboxMessage;                // 已持久化消息

// 路径
pub struct AgentPath;                    // 规范化线程路径

// 配置
pub struct AgentDefinition;              // TOML Agent 定义
pub struct AgentCatalog;                 // Agent 目录（内置 + 自定义）
pub struct AgentsSettings;               // 全局 Agent 设置
pub struct ResolvedAgent;                // 解析后的 Agent（含模型/沙箱决策）
pub fn load_agent_catalog(memory_dir, project_root) -> AgentCatalog;
pub fn load_agents_settings(memory_dir, project_root) -> AgentsSettings;
pub fn resolve_agent(catalog, settings, name, ...) -> Result<ResolvedAgent>;
```

## 与其他 crate 的关系

- **`types`（agent-types）** — 共享基础类型：`ModelTarget`、`SandboxMode` 等
- **`home`（agent-home）** — 路径约定（`default_memory_dir`）和 SQLite WAL 打开工具（`open_wal`）
- **`hooks`（agent-hooks）** — `PluginHookBus` 传递给派生子 Agent
- **被 `agent`（agent-core）** — `exec::subagents` 模块使用本 crate 的控制器和存储进行实际的派生执行
- **被 `server`（agent-server）** — `AstroServiceImpl` 使用 `AgentControl` 观察活动并发布 Thread Extension

## 测试运行命令

```bash
# 运行 agent-subagents 全部测试
cargo test -p subagents

# 运行单个测试
cargo test -p subagents runner_events_atomically_update
cargo test -p subagents -- --nocapture
```
