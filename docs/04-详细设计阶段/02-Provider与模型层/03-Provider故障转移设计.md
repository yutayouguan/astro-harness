# Provider 故障转移与网络恢复设计

> 版本：v2.0 | 更新日期：2026-08-28 | 状态：网络恢复待实施，显式 fallback 已实现
>
> 对应需求：F-02 Provider 管理、F-14 错误处理与容错、F-38 人工接管与执行控制
> 权威增量设计：[Codex 网络恢复对齐设计](../../superpowers/specs/2026-08-28-codex-network-recovery-alignment-design.md)

## 1. 职责边界

Provider fallback 与网络恢复是两个不同机制：

| 机制 | 负责层 | 目的 | 是否切换模型 |
| --- | --- | --- | --- |
| Request retry | `agent-providers` | 吸收瞬时传输、超时和 5xx | 否 |
| Network recovery | `agent-core::streaming` | 前台断网时保持 Turn，等待网络恢复 | 否 |
| Stream retry | `agent-core::streaming` | 恢复可安全重放的中断 sampling | 否 |
| Provider fallback | `agent-core::streaming::fallback` | 按用户显式链处理限流、鉴权或服务端不可用 | 是 |
| Managed network | `agent-network-proxy` / `agent-sandbox` | 限制工具子进程的出站目标 | 不适用 |

不得用 Provider fallback 处理本机断网。全局断网时切换供应商只会重复失败，并把可恢复状态错误地
收敛成“全部模型尝试失败”。

## 2. 结构化错误分类

`agent-providers` 在 HTTP/stream 边界产生 typed error；Core 不解析错误字符串。

| 类型 | 典型来源 | Request retry | Network recovery | Provider fallback |
| --- | --- | ---: | ---: | ---: |
| `ConnectionFailed` | DNS、TCP、TLS、CONNECT | 是 | 前台持续、后台有界 | 否 |
| `RequestTimeout` | 请求或响应头超时 | 是 | 否 | 否 |
| `ResponseStreamFailed` | 首包后 SSE/HTTP body 中断 | 否 | 否 | 否 |
| `HttpStatus(5xx)` | 服务暂不可用 | 是 | 否 | 显式链可用 |
| `HttpStatus(429)` | 限流 | 否 | 否 | 显式链可用 |
| `AuthenticationFailed` | 401/403 | 否 | 否 | 保留现有显式链策略 |
| `InvalidRequest` | 400、上下文或参数错误 | 否 | 否 | 否 |
| `Cancelled` | 用户中断或 Turn 替换 | 否 | 否 | 否 |

`ConnectionFailed` 是唯一进入前台持续等待的错误。TLS handshake EOF 必须归入此类。

## 3. Request retry

默认与 Codex 对齐：

```rust
pub struct RequestRetryPolicy {
    pub request_max_retries: u64,       // default: 4，不含首次
    pub base_delay: Duration,           // default: 200ms
    pub retry_transport: bool,          // default: true
    pub retry_5xx: bool,                // default: true
    pub retry_429: bool,                // default: false
}
```

退避公式：

```text
delay = 200ms × 2^(retry_attempt - 1) × jitter(0.9..1.1)
```

因此默认最多发起五次请求。每次重试必须重新构建 HTTP request；日志只记录 provider、model、错误
类别、attempt 和 delay，不记录凭据或用户请求正文。

此策略由所有文本 Provider 共享，禁止 Google、OpenAI 等实现各自维护另一套次数和退避。

## 4. 前台网络恢复

Request retry 耗尽后，如果错误仍为 `ConnectionFailed` 且 Turn 属于前台交互来源：

```text
Sampling
  → ConnectionFailed
  → emit StreamError("Reconnecting... waiting for network")
  → wait 5s / 10s / 20s / 40s / 60s / 60s ...
  → rebuild request from authoritative Session history
  → retry same ChatTarget in same turn_id
```

约束：

- 连接恢复次数不设上限，也不占用 `stream_max_retries`；
- 不进入 fallback 链；
- 等待和请求都必须响应 cancellation；
- 不写 assistant 消息或 tool result；
- 网络恢复后自动继续，不要求用户点击“恢复”；
- 用户中断产生唯一 `TurnAborted`。

Cron、自动化和其他无人值守任务使用 `NetworkRecoveryMode::Bounded`，耗尽后形成
`Error → TurnComplete(error)`，避免永久占用 worker。

## 5. Stream retry

普通流错误默认最多重试 5 次，状态文案为 `Reconnecting... n/5`。`stream_idle_timeout_ms`
默认 300000。

首包后的重试必须满足：

1. 不切换 Provider；
2. 不重复已显示内容；
3. 不重复执行已完成工具调用；
4. 无法证明可安全重建时宁可终止，也不拼接提示词猜测续写。

Astro 当前没有 Responses WebSocket transport，因此 Codex 的 WebSocket → HTTPS fallback 不适用。
未来引入 WebSocket 后，只允许传输协议降级，不能把模型切换称为 transport fallback。

## 6. 显式 Provider fallback

`ProviderStreamer` 持有 `Vec<ChatTarget>`，每次 sampling 从 primary 开始。只有用户配置了
fallback 链时才允许切换，且仅限首个有效业务 chunk 之前。

可切换：

- HTTP 429；
- HTTP 5xx 在 request retry 耗尽后；
- 401/403（保留 Astro 现有产品策略）；
- Provider 明确报告模型不可用。

不可切换：

- DNS/TCP/TLS/CONNECT；
- 400、上下文超限、内容拒绝；
- 用户取消；
- 已产生文本、reasoning 或 tool call 后的流错误；
- 工具、HITL、Hook 或本地存储错误。

切换只影响当前 sampling，不写回 `active_provider_id`。Usage 必须记录实际命中的
provider/model/base URL。

## 7. 事件协议

网络等待使用专用非终态事件：

```rust
EventMsg::StreamError(StreamErrorEvent {
    message,
    error_info,
    additional_details,
    retrying,
    retry_attempt,
    next_retry_ms,
})
```

Server 必须将其映射为独立 `ThreadStreamError`，不能复用 `ThreadError`。`StreamError` 不写入
rollout，也不进入最终助手回答。前端更新同一个网络状态项；恢复后收起，只有真正失败才显示终态错误。

## 8. 配置

```toml
[provider_retry]
request_max_retries = 4
stream_max_retries = 5
stream_idle_timeout_ms = 300000
unbounded_connection_retries = true

[provider_retry.background]
connection_max_retries = 5
```

配置约束：

- `request_max_retries` 与 `stream_max_retries` 上限均为 100；
- `unbounded_connection_retries` 只影响前台交互 Turn；
- 后台上限不可被配置为无限；
- 关闭前台无限恢复后，连接失败按 `stream_max_retries` 有界处理，但仍不得触发 Provider fallback。

## 9. 可观测性

结构化 retry 事件至少包含：

```text
turn_id
provider_id
model
retry.layer = http | stream
retry.operation = request | sampling
retry.attempt
retry.delay_ms
error.kind
```

指标：连接恢复次数、恢复耗时、请求/stream 重试次数、用户取消次数、后台耗尽次数、fallback
切换次数。网络等待不是 Provider 健康熔断信号；不得因本机断网把所有 Provider 标为故障。

## 10. 测试与验收

1. 不可达端口触发等待事件，不产生“全部模型尝试失败”。
2. 同址服务恢复后原 Turn 自动成功，`turn_id` 不变。
3. 连接失败不会访问第二个 `ChatTarget`。
4. 用户中断等待后只产生一个 `TurnAborted`。
5. 后台任务有界耗尽并释放 worker。
6. 5xx/429/401 的显式 fallback 行为保持可测试。
7. `StreamError` 不持久化、不进入回答正文。
8. 删除 Google 私有 retry 与字符串错误分类后无残留引用。

## 11. 相关文档

- [Codex 网络恢复对齐设计](../../superpowers/specs/2026-08-28-codex-network-recovery-alignment-design.md)
- [Codex 网络恢复对齐实施计划](../../superpowers/plans/2026-08-28-codex-network-recovery-alignment.md)
- [聊天 Fallback 链设计](../../superpowers/specs/2026-07-14-chat-fallback-chain-design.md)
- [Agent Loop Codex 对齐设计](../../superpowers/specs/2026-08-18-agent-loop-codex-alignment-design.md)
