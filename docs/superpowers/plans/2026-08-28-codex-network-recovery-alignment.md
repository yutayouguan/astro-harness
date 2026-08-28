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
- 各文本 Provider 的 HTTP/stream 适配器
- `crates/agent-providers/src/dispatch.rs`

**步骤：**

- [ ] 为 connection、request timeout、response stream、HTTP status、cancel 增加稳定变体。
- [ ] 将 reqwest `is_connect`、`is_timeout` 和 HTTP status 在 provider 边界转换一次。
- [ ] 删除调用方对 `anyhow::Error` 文本的 TLS/DNS/status 判断。
- [ ] 测试每个文本 Provider 的关键错误映射。

**验证：**

```bash
cargo test -p providers error
cargo check -p providers --all-targets
```

### Task 2：实现共享 request retry

**文件：**

- 新建 `crates/agent-providers/src/retry.rs`
- `crates/agent-providers/src/lib.rs`
- 文本 Provider 请求入口

**步骤：**

- [ ] 实现默认 `request_max_retries=4`、base 200ms、factor 2、jitter 0.9..1.1。
- [ ] 仅重试 connection/timeout/network/5xx；不重试 400/401/403/429/cancel。
- [ ] 每次尝试重新构建 request，禁止复用已消费 body/stream。
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
- [ ] 保留 Astro 明确配置的 429、401/403、5xx 首包前 fallback 扩展。
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
- 前台/后台 `RunTurnArgs` 构造点

**步骤：**

- [ ] 新增 `SamplingRetryState`：普通 retry 与 connection retry 分开计数。
- [ ] 新增 `NetworkRecoveryMode::{WaitUntilRecovered, Bounded}` 并由根任务来源确定。
- [ ] 前台 connection failure 使用 5/10/20/40/60 秒退避，60 秒封顶且不设次数上限。
- [ ] 等待和请求都响应 `CancellationToken` / `PauseControl`。
- [ ] 重试时从 Session 权威历史重建 prompt，保持同一 `turn_id`。
- [ ] 普通 stream error 默认最多 5 次；不得吞掉已产生的 partial output。

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

- [ ] 新增 `StreamErrorEvent` 与结构化 `ErrorInfo`。
- [ ] `EventMsg::StreamError` 不再复用 `ErrorEvent`。
- [ ] gRPC 新增 `ThreadStreamError` payload，删除映射到 `ThreadError` 的兼容路径。
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

- `apps/desktop/src-tauri/src/infra/thread_events.rs`
- `apps/desktop/src/lib/` 中 Thread 事件 reducer
- `apps/desktop/src/components/chat/` 中当前回答活动区
- 对应 CSS、i18n 与前端契约测试

**步骤：**

- [ ] 把 `ThreadStreamError` 投影为当前 Turn 的可更新状态，而非最终 error。
- [ ] 展示网络图标、等待文案、倒计时和停止操作。
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
- [ ] 后台任务达到边界后产生唯一终态错误。
- [ ] 5xx/429/401 的现有显式 fallback 行为不回归。
- [ ] 首包后断流不产生重复 assistant 文本或重复工具调用。
- [ ] `StreamError` 不进入回答正文和恢复历史。

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
