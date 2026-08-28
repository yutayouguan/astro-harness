# Codex 网络恢复对齐设计

**日期：** 2026-08-28

**状态：** 已确认，待实施

**范围：** 模型请求传输重试、交互式 Turn 断网等待、流中断恢复、事件协议与桌面状态展示
**Codex 基线：** `/Users/iswm/CodeRope/codex` commit `e24190caa9ee355044a7d70177d48a556d766d35`

## 1. 背景

Astro 当前在 `ProviderStreamer` 的首包探测阶段把连接、TLS、DNS、超时与 HTTP 错误一起交给
Provider fallback。链为空或全部目标失败时立即返回“全部模型尝试失败”，终止当前 Turn。
Google Interactions 另有最多三次的 provider 私有建连重试。这两个行为都不符合 Codex：

- Codex 的 provider HTTP 层先执行有限、通用的传输重试；
- 明确的连接失败进入 sampling 层的持续恢复循环，不消耗普通 stream retry 预算；
- UI 收到非终态 `StreamError`，显示“等待网络恢复”；
- 网络恢复后重建当前 sampling 请求并继续原 Turn；
- 用户中断始终可以结束等待；
- WebSocket 的 fallback 是传输降级到 HTTPS，不是切换模型或 Provider。

本设计只对齐“模型调用断网恢复”语义。Astro 已有、由用户显式配置的多 Provider fallback
仍作为产品扩展保留，但连接/TLS/DNS 错误不再进入该链。

## 2. 目标

1. 前台交互式 Turn 在网络不可达时保持 active，直到网络恢复或用户中断。
2. 建立统一的结构化错误分类，不再通过错误字符串判断连接、状态码或取消。
3. 将有限 HTTP 请求重试与持续连接恢复分层，默认值和退避行为对齐 Codex。
4. `StreamError` 是可恢复状态事件，不是助手文本，也不是 Turn 终态。
5. 网络恢复后继续同一个 `turn_id`，不重复已完成工具调用，不创建额外用户消息。
6. Cron、自动化和其他无人值守运行不得无限占用执行槽，使用有界策略并形成明确终态。
7. 删除 Google 专用连接重试；所有文本 Provider 使用同一个传输策略。

## 3. 非目标

- 不引入 Codex Responses WebSocket；Astro 当前没有对应传输时，不模拟 WebSocket → HTTPS 降级。
- 不改变工具子进程的 managed network、域名策略或审批模型。
- 不为 400、401、403、上下文超限、内容策略拒绝等不可恢复错误等待网络。
- 不静默重放已经产生可见文本、reasoning 或完整 tool call 的响应。
- 不把“无网络”误判成“模型不可用”，也不自动切换到能力不同的本地模型。

## 4. Codex 对齐基线

### 4.1 请求层

默认配置：

| 字段 | 默认值 | 语义 |
| --- | ---: | --- |
| `request_max_retries` | 4 | 首次请求之外最多重试 4 次，共最多 5 次尝试 |
| `request_retry_base_delay_ms` | 200 | 指数退避基数 |
| `stream_max_retries` | 5 | 非纯连接失败的 sampling/stream 重试次数 |
| `stream_idle_timeout_ms` | 300000 | 5 分钟无流事件视为连接丢失 |
| `unbounded_connection_retries` | `true` | 前台 sampling 对明确连接失败持续等待 |

请求层退避为 `base × 2^(attempt-1)`，乘 `0.9..1.1` jitter。仅下列错误可在请求层重试：

- `Connection`：DNS、TCP、TLS handshake/CONNECT 建立失败；
- `Timeout`；
- `Network`：发送或读取响应头前的传输错误；
- HTTP 5xx。

HTTP 400/401/403 不重试。429 不在 Codex 默认 HTTP retry policy 内；Astro 可在请求层退出后按
既有显式 fallback 策略处理，但不得归类为断网。

### 4.2 Sampling 层

请求层耗尽并返回 `ConnectionFailed` 后，前台交互式 sampling 进入独立恢复循环：

```text
5s → 10s → 20s → 40s → 60s → 60s → ...
```

每次等待前发送：

```text
EventMsg::StreamError {
  message: "Reconnecting... waiting for network",
  error_info: ResponseStreamDisconnected { http_status_code },
  additional_details,
}
```

该循环：

- 不增加 `stream_retries`；
- 不推进到下一个 `ChatTarget`；
- 不写入 assistant 消息；
- 不结束 Turn；
- 必须响应 Turn cancellation 和显式暂停/中断；
- 成功建立新流后清零连接退避，并继续同一个 Turn。

### 4.3 普通流错误

`Stream`、`Timeout`、`ResponseStreamFailed`、可重试 5xx 等非纯连接失败走有界
`stream_max_retries`。每次发出 `Reconnecting... n/N`。若流已产生用户可见内容或已形成工具调用，
不得切换 Provider；实现期必须先完成“partial output 清理或可安全重建”的测试再允许整轮重试。

Astro 不具备 WebSocket Responses transport，因此 Codex 的 WebSocket → HTTPS fallback 当前标记为
“不适用”，不得用 Provider 切换冒充传输 fallback。

### 4.4 源码证据索引

| Codex 路径 | 对齐事实 |
| --- | --- |
| `codex-rs/model-provider-info/src/lib.rs` | request/stream retry 默认值、idle timeout 与 request retry policy |
| `codex-rs/codex-client/src/retry.rs` | connection/timeout/network/5xx 分类与指数退避 jitter |
| `codex-rs/http-client/src/transport.rs` | `reqwest` connect/timeout/network 到 typed transport error 的转换 |
| `codex-rs/core/src/responses_retry.rs` | 5 秒起步、60 秒封顶的持续连接恢复与非终态事件 |
| `codex-rs/features/src/lib.rs` | `UnboundedConnectionRetries` 为默认开启的 stable feature |
| `codex-rs/core/tests/suite/stream_no_completed.rs` | 端口不可达后启动同地址 server，原 Turn 自动完成的集成测试 |

OpenAI 公开 Codex 配置参考只定义对外可配置边界，未承诺上述内部重连时序。
因此本设计的精确行为以锁定 commit 的本地源码和测试为准；更新 Codex 基线时必须重新核对该表。

## 5. Astro 策略矩阵

| 错误 | 请求层 | Sampling 层 | Provider fallback | Turn 结果 |
| --- | --- | --- | --- | --- |
| DNS/TCP/TLS/CONNECT | 最多 4 次重试 | 前台无限等待；后台有界 | 否 | 恢复或用户中断；后台耗尽后失败 |
| 请求超时 | 最多 4 次重试 | 有界 stream retry | 否 | 成功或失败 |
| HTTP 5xx | 最多 4 次重试 | 有界 stream retry | 可按显式链 | 成功或链耗尽失败 |
| HTTP 429 | 不在通用请求重试 | 尊重服务端 delay 的产品策略 | 可按显式链 | 成功或链耗尽失败 |
| HTTP 401/403 | 否 | 否 | 保留现有显式链策略 | 成功或链耗尽失败 |
| HTTP 400/上下文/内容拒绝 | 否 | 否 | 否 | 立即失败 |
| 用户取消/中断 | 否 | 否 | 否 | `TurnAborted` |
| 首包后的流断开 | 否 | 有界且须避免重复内容 | 否 | 成功或失败 |

## 6. 结构化错误契约

`ProviderError` 必须覆盖以下稳定语义，provider 适配器只负责转换，不决定 Turn 生命周期：

```rust
pub enum ProviderError {
    ConnectionFailed { provider: String, detail: String },
    RequestTimeout { provider: String, detail: String },
    ResponseStreamFailed { provider: String, detail: String },
    HttpStatus { provider: String, status: u16, detail: String, retry_after_ms: Option<u64> },
    InvalidRequest { provider: String, detail: String },
    AuthenticationFailed { provider: String, detail: String },
    Cancelled,
    // 现有能力/模型错误继续保留。
}
```

硬切要求：

- 删除 `is_failover_eligible(&anyhow::Error)` 的字符串匹配；
- `dispatch::chat_stream`、`ProviderStreamer` 和 sampling loop 传递 typed error；
- 删除 `google.rs::retry_connect` / `send_interactions` 私有重试；
- 不保留旧错误字符串分类兼容路径。

## 7. 运行时状态机

```text
Sampling
  ├─ success ───────────────────────────────→ ConsumingStream
  ├─ ConnectionFailed + interactive ───────→ WaitingForNetwork
  │                                             ├─ retry success → ConsumingStream
  │                                             ├─ retry failed  → WaitingForNetwork
  │                                             └─ cancel        → TurnAborted
  ├─ retryable non-connection ──────────────→ BoundedStreamRetry
  └─ non-retryable ─────────────────────────→ Error → TurnComplete(error)

ConsumingStream
  ├─ complete ──────────────────────────────→ 下一工具轮或 TurnComplete
  ├─ retryable stream failure ──────────────→ BoundedStreamRetry
  └─ cancel ────────────────────────────────→ TurnAborted
```

`TurnContext` 增加明确的运行来源/恢复策略，不用调用栈或字符串猜测：

```rust
pub enum NetworkRecoveryMode {
    WaitUntilRecovered,      // Desktop 前台交互
    Bounded { max_retries: u64 }, // Cron、自动化、无 UI 后台任务
}
```

子 Agent 是否无限等待由其所属根 Turn 的交互性决定；后台根任务派生的子 Agent 必须继承有界策略。

## 8. 事件、持久化与 UI

### 8.1 协议硬切

将 `EventMsg::StreamError(ErrorEvent)` 改为专用 `StreamErrorEvent`：

```rust
pub struct StreamErrorEvent {
    pub message: String,
    pub error_info: Option<ErrorInfo>,
    pub additional_details: Option<String>,
    pub retrying: bool,
    pub retry_attempt: u64,
    pub next_retry_ms: Option<u64>,
}
```

gRPC 增加独立 `ThreadStreamError` payload。Server 不再把 `StreamError` 投影成终态
`ThreadError`，不保留兼容映射。

### 8.2 持久化

- `StreamError` 是瞬时状态，不写 rollout；
- 当前 Turn 继续保持 active；
- `Error` 后必须仍有且只有一个 `TurnComplete { error }`；
- 用户取消等待产生 `TurnAborted { Interrupted }`；
- 重连不会新增用户消息、assistant 消息或工具结果。

### 8.3 桌面表现

- 在当前回答时间线显示单条可更新状态：“正在重新连接，等待网络恢复”；
- 展示下一次重试倒计时和“停止”操作；
- 不弹永久错误 Toast，不创建助手错误气泡；
- 重试时更新原状态项，不逐次堆叠；
- 恢复后自动收起/标记完成，随后继续流式输出；
- 只有非重试错误或后台重试耗尽才进入终态错误展示。

## 9. 并发、取消与幂等

1. 等待必须使用 cancellation-aware `select!`，不能裸 `sleep` 阻塞取消。
2. 每个 Turn 只有一个 `SamplingRetryState`，连接退避不得跨 Turn 共享。
3. 重试前重新从 Session 权威历史构建 prompt；不得复制已落库消息。
4. 工具只在完整 tool call 被记录后执行；sampling 重试不能重放已完成工具。
5. 同一 Turn 的 steering 输入在安全边界保留，网络恢复后参与下一次 prompt 构建。
6. 后台任务达到有界上限后写入明确错误和唯一 Turn 终态，不永久占用 worker。

## 10. 可观测性

每次重试记录结构化字段：

- `turn_id`、`provider_id`、`model`；
- `retry.layer = http | stream`；
- `retry.operation = request | sampling`；
- `retry.attempt`、`retry.delay_ms`；
- `error.kind`，不记录 API Key、完整请求体或用户内容。

指标至少包含：连接恢复次数、恢复耗时、请求重试次数、stream retry 次数、用户取消次数和后台耗尽次数。

## 11. 验收标准

1. 模拟端口不可达时，前台 Turn 发出 `Reconnecting... waiting for network`，不发终态错误。
2. 在同一地址启动测试 server 后，原 Turn 自动完成且 `turn_id` 不变。
3. 等待期间用户中断在一个调度周期内结束，产生唯一 `TurnAborted`。
4. 连接失败不进入第二个 `ChatTarget`，也不出现“全部模型尝试失败”。
5. 默认请求层为首次 + 4 次重试，退避基数 200ms、倍数 2、jitter 0.9..1.1。
6. 普通 stream retry 默认最多 5 次，并显示 `Reconnecting... n/5`。
7. Cron/自动化在配置的上限后失败，不无限等待。
8. `StreamError` 经 Core → Server → Tauri → React 保持非终态语义，且不会进入最终回答正文。
9. Google、OpenAI、Claude、DeepSeek、MiniMax 至少各有一条 typed transport error 映射测试。
10. 删除 Google 私有连接重试和字符串错误分类后，全仓无兼容路径残留。

## 12. 相关文档

- [实施计划](../plans/2026-08-28-codex-network-recovery-alignment.md)
- [Agent Loop Codex 对齐设计](./2026-08-18-agent-loop-codex-alignment-design.md)
- [聊天 Fallback 链设计](./2026-07-14-chat-fallback-chain-design.md)
- [Provider 故障转移设计](../../04-详细设计阶段/02-Provider与模型层/03-Provider故障转移设计.md)
- [agent-runtime 详细设计](../../04-详细设计阶段/01-核心引擎层/02-agent-runtime详细设计.md)
