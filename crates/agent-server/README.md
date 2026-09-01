# agent-server (package: `server`)

Astro 独立 gRPC 后端：承载 `AstroServiceImpl`，管理 Durable Thread 生命周期，并在独立线程中运行 Cron/Workflow/Webhook 后台服务。可通过 `run_embedded()` 嵌入 Tauri 桌面壳同进程启动。

## 核心职责

1. **gRPC 服务托管** — 基于 tonic 启动 `AstroServiceServer`，实现 `AstroService` 全部 RPC（Chat、ChatControl、SubmitTurn、ResumeThread、InterruptResume、ListSkills、ExecuteSkill、ListMcpServers、QueryMemory、SubscribeThreadEvents 等）。
2. **Durable Thread 管理** — 通过 `ThreadManager` / `ThreadStateManager` 管理 `ManagedThread`（含 `AstroThread` 运行时、事件监听器、副作用 supervisor），支持 idle 30 分钟自动卸载。
3. **连接与订阅传输** — `ConnectionRegistry` 提供有界、独立背压的 per-connection 事件通道，支持同 ID 重连覆盖、慢消费者自动驱逐和精确 generation 清理。
4. **Cron 定时任务** — 在独立 `current_thread` 运行时（因 `AgentLoop` 非 Send）每 30s 认领到期任务，解析 `providers.json` 凭据后调用 `agent::exec::cron` 执行。
5. **Workflow 定时触发与 Webhook** — 每 30s 扫描 `ScheduledTrigger` 工作流并到期执行；Webhook HTTP 服务器接收 `POST /webhook/{workflow_id}` 触发工作流执行。

## 模块结构

| 文件 | 职责 |
|---|---|
| `lib.rs` | 顶层入口：`run()`（独立二进制）、`run_embedded()`（Tauri 内嵌）、`serve()` 启动 gRPC + Cron/Workflow/Webhook 后台线程 |
| `main.rs` | 独立二进制入口：`#[tokio::main]` 调用 `server::run()` |
| `grpc/mod.rs` | gRPC 模块聚合：导出 `AstroServiceImpl` |
| `grpc/astro_service.rs` | `AstroServiceImpl` 核心实现：会话管理（`get_session`/`get_or_create_thread`）、暂停/HITL 控制、回合后副作用（记忆 review + 标题生成）、Agent Thread V2 活动观察 |
| `grpc/thread_service.rs` | Thread 级 RPC 实现：`subscribe_thread_events`、`submit_turn`、`resume_thread`、`unsubscribe_thread`；请求校验与 resume 语义 |
| `grpc/files.rs` | 沙箱目录列举：`list_directory()` 限制在 `sandbox_root` 下，跳过点文件与 `target` |
| `grpc/interrupt_store.rs` | Interrupt 旁路文件：`save_interrupt_file` / `clear_interrupt_file` / `resume_items_from_proto` |
| `cron_runner.rs` | Cron 执行器：`tick_and_execute()` 认领到期任务、解析凭据（`providers.json` + 环境变量）、执行并通知 |
| `thread_listener.rs` | Thread 事件监听器：将 `AstroThread` 的 `Event` 转为 `proto::ThreadEvent` 并分发给订阅者；管理 Extension Waiter 和 Background Sink |
| `thread_manager.rs` | `ManagedThread` 管理：运行时句柄 + 监听器 + 副作用 supervisor 的生命周期；`ThreadManager` 线程安全注册表 |
| `thread_state.rs` | Thread 状态管理：`ThreadHistoryBuilder`（增量跟踪 Turn/Item/Delta）、`ThreadState`（订阅者映射）、`ThreadStateManager`（全局状态索引）、`ListenerCommand` 枚举 |
| `transport.rs` | 连接传输层：`ConnectionRegistry`（有界通道 + CancellationToken）、`ConnectionGenerationKey`（精确 generation 清理） |
| `webhook_server.rs` | 轻量 HTTP Webhook 服务器：解析 HTTP 请求、验证 secret、触发工作流执行 |
| `workflow_ticker.rs` | 工作流定时触发：管理调度状态（`schedule_state.json`）、到期时 `spawn_blocking` 执行工作流 |

## 核心类型与 API

```rust
// 入口
pub async fn run() -> anyhow::Result<()>;
pub async fn run_embedded(ready: Option<oneshot::Sender<String>>) -> anyhow::Result<()>;
pub async fn serve(ready: Option<oneshot::Sender<String>>, ephemeral_if_unset: bool) -> anyhow::Result<()>;

// gRPC 服务
pub struct AstroServiceImpl;                // 实现 AstroService trait 的全部 RPC
impl AstroServiceImpl {
    pub fn new(memory_dir: PathBuf) -> Self;
}

// Thread 管理
pub struct ManagedThread;                   // AstroThread 运行时 + 监听器 + 副作用 supervisor
pub struct ThreadManager;                   // thread_id -> ManagedThread 并发安全注册表
pub enum RemoveCurrentThread { Removed, Leased, NotCurrent }

// Thread 状态
pub struct ThreadState;                     // 单个 Thread 的状态（历史、订阅者、活动）
pub struct ThreadStateManager;              // thread_id -> ThreadState 全局索引
pub struct ThreadHistoryBuilder;            // Turn/Item 增量跟踪器
pub struct ItemSnapshot;                    // 单个 Item 快照
pub struct TurnSnapshot;                    // 单个 Turn 快照
pub struct ThreadSnapshot;                  // Thread 完整快照（含活跃 Turn 和后台 Turn）
pub struct ThreadActivity;                  // 状态 + 是否有订阅者
pub enum ListenerCommand;                   // 事件监听器命令枚举

// 连接传输
pub struct ConnectionRegistry;              // 有界、独立背压的连接注册表
pub struct ConnectionGeneration;            // 精确的连接 generation 标识
pub struct ConnectionGenerationKey;         // generation 键（connection_id + uuid）
```

## 与其他 crate 的关系

- **`agent`（agent-core）** — 调用 `AgentBuilder` / `Session` / `AstroThread` 构建和运行 Agent 循环
- **`subagents`** — Agent Thread V2 控制面：`AgentControl` 活动观察和 watcher 管理
- **`proto`** — Protobuf gRPC 服务契约（`AstroService` 14+ RPC）
- **`providers`** — 读取环境 API Key（`read_env_api_key`）和默认模型
- **`session`** — `SessionStore` 打开会话存储
- **`memory`** — `ensure_workspace` 初始化工作区、`MemoryManager` 记忆 review
- **`home`** — 路径约定（`default_memory_dir`、`logs_dir`、`active_agent_id`）
- **`cron`** — `CronStore` 认领任务、`compute_next_run` 计算下次运行时间
- **`workflow`** — `WorkflowStore` / `WorkflowRunDb` / `execute_workflow` 工作流执行
- **`hooks`** — typed Plugin/Command/MCP/Gateway/Shell 生命周期钩子系统
- **`tools`** — 图像生成目标解析（`image_gen_targets_from_parts`）
- **`agent-protocol`** — Thread 事件协议类型（`Event`、`EventMsg`、`Op`）
- **`agent-rollout`** — Rollout 持久化记录器

## 测试运行命令

```bash
# 运行 agent-server 全部测试
cargo test -p server

# 运行单个测试
cargo test -p server thread_hitl_gate_is_resolvable
cargo test -p server -- --nocapture
```
