# Codex 网络恢复对齐实施计划

**目标：** 将 Astro 的模型断网行为从“Provider 链失败后结束”硬切为 Codex 的“请求层有限重试 + 前台 Turn 持续等待网络恢复 + 非终态 StreamError”。

**设计：** [`2026-08-28-codex-network-recovery-alignment-design.md`](../specs/2026-08-28-codex-network-recovery-alignment-design.md)

**Codex 基线：** `/Users/iswm/CodeRope/codex` commit `e24190caa9ee355044a7d70177d48a556d766d35`

**状态：** 待实施

## 实施原则

- 每个批次先写失败测试，再实现，再独立提交。
- 不保留 Google 私有 retry 或基于错误字符串的兼容分类。
- 不把连接恢复与 Provider fallback、managed network policy 混为一层。
- 保留用户现有未提交文件；只暂存当前批次文件。
- 前台无限等待必须可取消；后台执行必须有界。

## Batch A：结构化错误与通用请求重试

### Task 1：扩展 Provider 错误契约

**文件：**

- `crates/agent-providers/src/types/error.rs`
- `crates/agent-providers/src/types/stream.rs`
- `crates/agent-providers/src/traits/`
- `crates/agent-providers/src/shared/sse.rs`
- `crates/agent-providers/src/compat/`
- 各文本 Provider 的 HTTP/stream 适配器
- `crates/agent-providers/src/dispatch.rs`
- `crates/agent-core/src/streaming/types.rs`
- `crates/agent-core/src/streaming/provider.rs`

**步骤：**

- [ ] 为 connection、request timeout、network、response stream、HTTP status、cancel 增加稳定变体。
- [ ] 有界 retry 耗尽时返回最后一个 typed `ProviderError`；不新建重复的 Runtime 层
  “后台网络耗尽”变体。
- [ ] 将 reqwest `is_connect`、`is_timeout` 和 HTTP status 在 provider 边界转换一次。
- [ ] 只有可证明的 connect 错误映射为 `ConnectionFailed`；其他传输错误映射为有界的 `Network`。
- [ ] 将 `CompletionStream` 改为 `Stream<Item = ProviderResult<StreamChunk>>`，并更新 trait、compat wrapper、
  `ChatOverride` 与测试 fixture。
- [ ] 删除 `StreamChunk::Error(String)`；SSE/provider 错误一律从 `Err(ProviderError)` 通道传递。
- [ ] `ProviderError::Other` 仅作为 non-retryable 兜底，任何 retry/fallback 判定不得解析其文本。
- [ ] 删除调用方对 `anyhow::Error` 文本的 TLS/DNS/status 判断。
- [ ] 测试每个文本 Provider 的关键错误映射。

**验证：**

```bash
cargo test -p providers error
cargo test -p providers stream
cargo check -p providers --all-targets
```

### Task 2：实现共享 request retry 与 provider 配置传递

**文件：**

- 新建 `crates/agent-providers/src/retry.rs`
- `crates/agent-providers/src/lib.rs`
- `crates/agent-providers/src/types/request.rs`
- `crates/agent-types/src/chat_target.rs`
- `apps/desktop/src-tauri/src/commands/providers.rs`
- `crates/agent-server/src/cron_runner.rs`
- 文本 Provider 请求入口

**步骤：**

- [ ] 实现默认 `request_max_retries=4`、base 200ms、factor 2、jitter 0.9..1.1。
- [ ] 仅重试 connection/timeout/network/5xx；不重试 400/401/403/429/cancel。
- [ ] 每次尝试重新构建 request，禁止复用已消费 body/stream。
- [ ] 将 `request_max_retries`、`stream_max_retries`、`stream_idle_timeout_ms` 作为单个 Provider 的可选配置，
  经 `providers.json → ChatTarget → providers::ProviderConfig` 传递；不新增全局 `[provider_retry]`。
- [ ] 默认值为 4/5/300000，前两项上限 100；覆盖 Desktop 与 Cron 两条解析路径。
- [ ] 记录 `codex.retry` 等价结构化 telemetry。
- [ ] 使用暂停时间可控的测试时钟覆盖次数、退避、最终错误。

**验证：**

```bash
cargo test -p providers retry
cargo check -p providers --all-targets
```

### Task 3：删除 Google 私有兼容实现

**文件：**

- `crates/agent-providers/src/impls/google.rs`

**步骤：**

- [ ] 删除 `retry_connect`、`send_interactions` 与对应私有常量/测试。
- [ ] Google Interactions 改用共享 request retry。
- [ ] 验证 stale `previous_interaction_id` 的 404 重放仍只发生一次且不受网络 retry 影响。

**验证：**

```bash
cargo test -p providers impls::google::tests
rg -n "retry_connect|MAX_CONNECT_ATTEMPTS" crates/agent-providers
```

预期：测试通过，`rg` 无结果。

## Batch B：Sampling 恢复状态机

### Task 4：从 Provider fallback 中移除网络错误

**文件：**

- `crates/agent-core/src/streaming/fallback.rs`
- `crates/agent-core/src/streaming/provider.rs`

**步骤：**

- [ ] `is_failover_eligible` 改为 typed match，删除字符串匹配。
- [ ] connection/TLS/DNS 不得切换 `ChatTarget`。
- [ ] 将现有“一次 helper 内遍历整链”拆为单目标 sampling primitive 与外层单向 target 游标。
- [ ] 保留 Astro 明确配置的 429、401/403、5xx 首包前 fallback 矩阵，但本 Task 不自行重试。
- [ ] 单目标失败或不可 failover 错误直接返回原 typed error；只有实际尝试多个目标才生成聚合错误。
- [ ] 流已经产生有效内容后继续禁止 Provider 切换。

**验证：**

```bash
cargo test -p agent streaming::fallback
```

### Task 5：实现可取消的连接恢复循环

**文件：**

- 新建 `crates/agent-core/src/streaming/retry.rs`
- `crates/agent-core/src/streaming/mod.rs`
- `crates/agent-core/src/streaming/multi_turn.rs`
- `crates/agent-core/src/streaming/maintenance.rs`
- `crates/agent-core/src/runtime/turn_context.rs`
- `crates/agent-core/src/exec/background.rs`
- `crates/agent-core/src/exec/cron.rs`
- `ThreadTurnTaskArgs` 与前台/后台 `RunTurnArgs` 构造点

**步骤：**

- [ ] 新增 `SamplingRetryState`：普通 retry 与 connection retry 分开计数。
- [ ] 新增 `NetworkRecoveryMode::{WaitUntilRecovered, Bounded}` 并由根任务来源确定。
- [ ] 策略经 `ThreadTurnTaskArgs → RunTurnArgs → TurnContext` 显式传递；Cron/内部任务有界，
  Desktop 用户 Turn 及其子 Agent 持续恢复。
- [ ] `exec::background` 由调用方传入策略，不以 adapter 名称猜测任务是否无人值守。
- [ ] 前台 connection failure 使用 5/10/20/40/60 秒退避，60 秒封顶且不设次数上限。
- [ ] `NetworkRecoveryMode::Bounded` 不携带全局次数；每个当前 target 的有界上限取其
  Provider 有效 `stream_max_retries`（默认 5），不引入单独的后台重试配置。
- [ ] 对每个 target 创建独立 retry state：当前目标先耗尽 Codex request/stream retry，再对 5xx
  进入 Astro 显式 fallback；429、401/403 可在首包前直接 fallback。
- [ ] target 游标只能向后移动，切换后不得因外层 retry 重新从 primary 开始。
- [ ] 等待和请求都响应 `CancellationToken` / `PauseControl`。
- [ ] 重试时从 Session 权威历史重建 prompt，保持同一 `turn_id`。
- [ ] 普通 stream error 默认最多 5 次；不得吞掉已产生的 partial output。
- [ ] `run_sampling_request` 返回 typed error，在 retry/fallback 决策完成前禁止 `.to_string()`。
- [ ] `collect_background_events` 不再缓存 `StreamError` 并在成功终态时误报失败，只以 `TurnComplete.error`
  或 `TurnAborted` 决定后台结果。

**验证：**

```bash
cargo test -p agent network_recovery
cargo test -p agent streaming::
```

## Batch C：事件协议与桌面交互

### Task 6：硬切专用 StreamError 协议

**文件：**

- `crates/agent-protocol/src/event.rs`
- `crates/agent-rollout/src/policy.rs`
- `crates/agent-proto/proto/astro.proto`
- `crates/agent-server/src/thread_listener.rs`
- `crates/agent-server/src/grpc/astro_service.rs`
- 协议生成物与映射测试

**步骤：**

- [ ] 按锁定 Codex 基线引入 canonical `CodexErrorInfo`，使 `ErrorEvent` 改为
  `{ message, codex_error_info }`，并新增只含 `message` 的 `WarningEvent`。
- [ ] 新增与 Codex 字段集一致的 `StreamErrorEvent { message, codex_error_info, additional_details }`。
- [ ] `EventMsg::StreamError` 不再复用 `ErrorEvent`，`EventMsg::Warning` 也不再复用 `ErrorEvent`。
- [ ] 对齐 Codex app-server：统一投影为 `ErrorNotification`，普通错误 `will_retry=false`，
  `StreamError` 为 `will_retry=true`。
- [ ] gRPC 层将旧 `ThreadError` payload 硬切为 `ThreadErrorNotification { error, will_retry }`；
  `thread_id` 和 `turn_id` 使用现有 `ThreadEvent` 外层字段，不新造 `ThreadStreamError`。
- [ ] `ThreadError` 保留 `message`、`codex_error_info`、`additional_details`，删除无结构语义的
  `error_type` 兼容字段。
- [ ] `additional_details` 在进入协议前脱敏，测试 API Key、Authorization header、proxy 凭据和
  URL query secret 不会出现在事件、日志或 UI。
- [ ] 保持 `StreamError` 非终态、非 rollout 持久事件。
- [ ] 保证最终失败仍为 `Error → TurnComplete(error)`，取消为 `TurnAborted`。

**验证：**

```bash
cargo test -p agent-protocol
cargo test -p agent-rollout
cargo test -p server thread_listener
```

### Task 7：接通桌面重连状态

**文件：**

- `apps/desktop/src-tauri/src/commands/chat.rs`
- `apps/desktop/src-tauri/src/infra/thread_events.rs`
- `apps/desktop/src/hooks/chat/useSend.ts`
- 当前 Turn 状态行所在组件、CSS、i18n 与前端契约测试

**步骤：**

- [ ] 将 `ErrorNotification.will_retry=true` 投影为当前 Turn 的可更新状态，而非最终 error。
- [ ] 硬切为
  `ChatStreamEvent::Error { message, will_retry, codex_error_info, additional_details }`；`useSend`
  只在 `will_retry=false` 时设置 `terminalError` 或改写助手消息。
- [ ] 展示网络图标、等待文案并复用现有停止操作；不在协议未提供 delay 时虚构倒计时。
- [ ] 状态只在瞬时状态槽显示，不写入消息时间线或恢复历史。
- [ ] 连续重试只更新一个状态项；恢复后收起并继续输出。
- [ ] 深/浅色与窄窗口均验证，不遮挡回答末尾和输入框。

**验证：**

```bash
cd apps/desktop && npm test
cd apps/desktop && npx tsc --noEmit
cd apps/desktop && npm run build
```

## Batch D：端到端策略与回归

### Task 8：前台恢复、后台耗尽与取消测试

**文件：**

- `crates/agent-core/tests/` 新增网络恢复集成测试
- `crates/agent-server/tests/` 协议投影测试
- Desktop UI/Storybook 测试

**场景：**

- [ ] 端口不可达 → 发出等待网络事件 → 同址启动 server → 原 Turn 成功。
- [ ] 等待期间中断 → 唯一 `TurnAborted`，无 `TurnComplete(error)`。
- [ ] 连接失败不触发第二个 `ChatTarget`。
- [ ] 仅有一个目标或首个错误不可 failover 时，错误不带“全部模型尝试失败”前缀。
- [ ] 后台任务达到边界后产生唯一终态错误。
- [ ] 后台先收到 `StreamError`、再收到 `TurnComplete(success)` 时必须返回成功。
- [ ] 5xx/429/401 的现有显式 fallback 行为不回归。
- [ ] 多目标失败时每个目标只消费自身 retry 预算，不从 primary 循环重启 fallback 链。
- [ ] 首包后断流不产生重复 assistant 文本或重复工具调用。
- [ ] `StreamError` 不进入回答正文和恢复历史。
- [ ] 全仓搜索确认无 `StreamChunk::Error`、无 `is_failover_eligible(&anyhow::Error)`、无流错误
  `.to_string()` 后再分类的路径。

**最终验证：**

```bash
cargo fmt --all -- --check
cargo test -p providers
cargo test -p agent
cargo test -p agent-protocol
cargo test -p agent-rollout
cargo test -p server
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets
cd apps/desktop && npx tsc --noEmit
cd apps/desktop && npm test
cd apps/desktop && npm run build
```

## 完成定义

- [ ] 前台断网不再出现“全部模型尝试失败”。
- [ ] 网络恢复无需用户重新发送消息或点击恢复。
- [ ] Google 不存在私有重试路径。
- [ ] Core、Server、gRPC、Tauri、React 对 `StreamError` 的非终态语义一致。
- [ ] 所有新增配置有默认值、上限、序列化和升级测试。
- [ ] 需求、设计、实现和测试文档使用同一错误矩阵与默认参数。
