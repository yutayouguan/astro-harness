# Provider 故障转移设计

> 版本：v1.0 | 日期：2026-08-07 | 状态：草稿
> 对应需求：F-14 错误处理与容错、F-02 多模态 Provider
> 补充文档：[03-错误处理与容错.md](../../03-系统设计阶段/03-基础设施/03-错误处理与容错.md)（错误分类体系概述）

本文档专注于 13 号文档未展开的故障转移细节：错误可重试性分类、指数退避算法、熔断器模式、Provider 自动切换、限流（Rate Limit）专项处理、流式响应中断恢复、以及全链路降级策略。

---

## 1. 错误可重试性分类

> **统一定义**：`ProviderError` 的权威定义在 `agent-types` crate（见 `01-agent-providers详细设计.md`）。本文档引用的错误分类与权威定义保持一致。

本节基于权威 `ProviderError` 枚举，按可重试性对错误进行分类：

| 分类 | 错误变体 | 行为 |
| --- | --- | --- |
| 可立即重试 | `Network`, `ConnectionTimeout`, `ServerError { code >= 500 }` | 同 Provider 立即重试（指数退避） |
| 可退避重试 | `RateLimited` | 同 Provider 退避等待后重试，**不触发 failover**（等待退避更经济） |
| 可切换 Provider | `ModelNotAvailable` | 直接切换到 fallback Provider |
| 不可重试 | `AuthenticationFailed`, `InvalidRequest`, `ContentFiltered`, `ContextTooLong`, `Unsupported` | 直接返回错误 |
| 内部/解析 | `ParseError`, `Internal` | 直接返回错误 |

对应权威定义中的两个判断方法：

```rust
// is_retryable(): Network, ConnectionTimeout, ServerError(5xx), RateLimited → true
// should_failover(): Network, ConnectionTimeout, ServerError(5xx), ModelNotAvailable → true
// 注意：RateLimited 可重试但不触发 failover
```

---

## 2. 指数退避算法

```rust
// crates/agent-core/src/provider/retry.rs

pub struct RetryPolicy {
    pub max_attempts: u32,          // 最大尝试次数（含首次）默认 3
    pub base_delay_ms: u64,         // 基础延迟 1000ms
    pub max_delay_ms: u64,          // 最大延迟 30s
    pub jitter_ratio: f32,          // 随机抖动比例 0.25（±25%）
    pub backoff_multiplier: f32,    // 退避倍数 2.0（指数）
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self { max_attempts: 3, base_delay_ms: 1000, max_delay_ms: 30_000,
               jitter_ratio: 0.25, backoff_multiplier: 2.0 }
    }
}

impl RetryPolicy {
    /// 计算第 attempt 次（从 1 开始）的等待时间
    pub fn delay_for(&self, attempt: u32, rate_limit_retry_after: Option<u64>) -> Duration {
        // Rate Limit 的 retry-after 优先
        if let Some(secs) = rate_limit_retry_after {
            return Duration::from_secs(secs + 1);  // +1 作为安全余量
        }

        // 指数退避：base × multiplier^(attempt-1)
        let exp_delay = (self.base_delay_ms as f32
            * self.backoff_multiplier.powi(attempt as i32 - 1)) as u64;
        let capped = exp_delay.min(self.max_delay_ms);

        // Full Jitter：[0, capped × jitter_ratio] 的随机量
        let jitter = (capped as f32 * self.jitter_ratio * rand::random::<f32>()) as u64;
        Duration::from_millis(capped + jitter)
    }
}

// 重试延迟示意（base=1000ms, multiplier=2.0, jitter=25%）：
// attempt 1 失败 → 等待 ~1000ms（±250ms）
// attempt 2 失败 → 等待 ~2000ms（±500ms）
// attempt 3 失败 → 终止（或切换 Provider）
```

---

## 3. 熔断器（Circuit Breaker）

防止持续向故障 Provider 发送请求，减少无效等待。

```rust
// crates/agent-runtime/src/provider/circuit_breaker.rs

#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    Closed,               // 正常：所有请求通过
    Open(Instant),        // 熔断：请求直接拒绝，等待冷却
    HalfOpen,             // 探测：放行一个请求测试是否恢复
}

pub struct CircuitBreaker {
    state: Arc<Mutex<CircuitState>>,
    failure_threshold: u32,     // 连续失败次数阈值（默认 5）
    success_threshold: u32,     // HalfOpen 状态下成功次数阈值（默认 2）
    open_duration: Duration,    // 熔断持续时间（默认 60s）
    consecutive_failures: Arc<AtomicU32>,
    consecutive_successes: Arc<AtomicU32>,
}

impl CircuitBreaker {
    /// 请求前调用，判断是否允许通过
    pub fn allow_request(&self) -> Result<(), ProviderError> {
        let mut state = self.state.lock().unwrap();
        match *state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open(opened_at) => {
                if opened_at.elapsed() >= self.open_duration {
                    // 冷却时间到，进入半开状态
                    *state = CircuitState::HalfOpen;
                    self.consecutive_successes.store(0, Ordering::SeqCst);
                    Ok(())  // 放行探测请求
                } else {
                    Err(ProviderError::Internal("熔断中，Provider 暂不可用".into()))
                }
            }
            CircuitState::HalfOpen => Ok(()),  // 探测请求通过
        }
    }

    /// 请求成功后调用
    pub fn on_success(&self) {
        let successes = self.consecutive_successes.fetch_add(1, Ordering::SeqCst) + 1;
        self.consecutive_failures.store(0, Ordering::SeqCst);
        if successes >= self.success_threshold {
            let mut state = self.state.lock().unwrap();
            if *state == CircuitState::HalfOpen {
                *state = CircuitState::Closed;
                tracing::info!("熔断器恢复 Closed 状态");
            }
        }
    }

    /// 请求失败后调用
    pub fn on_failure(&self) {
        let failures = self.consecutive_failures.fetch_add(1, Ordering::SeqCst) + 1;
        self.consecutive_successes.store(0, Ordering::SeqCst);
        if failures >= self.failure_threshold {
            let mut state = self.state.lock().unwrap();
            // 使用 matches! 检查是否已处于 Open 状态，而非比较具体的 Instant 值
            // （两个不同时间点的 Instant 永远不相等，直接比较会导致每次失败都重置冷却计时器）
            if !matches!(*state, CircuitState::Open(_)) {
                *state = CircuitState::Open(Instant::now());
                tracing::warn!("熔断器触发 Open 状态（连续失败 {} 次）", failures);
            }
        }
    }
}
```

---

## 4. Provider 自动切换

```rust
// crates/agent-runtime/src/provider/failover.rs

pub struct FailoverClient {
    primary: Arc<dyn TextClient>,
    fallbacks: Vec<Arc<dyn TextClient>>,  // 按优先级排列
    circuit_breakers: HashMap<String, CircuitBreaker>,
    retry_policy: RetryPolicy,
    app: AppHandle,  // 用于通知用户 Provider 已切换
}

impl FailoverClient {
    pub async fn chat_stream_with_failover(
        &self,
        req: ChatRequest,
    ) -> anyhow::Result<ChatStream> {
        let providers: Vec<_> = std::iter::once(&self.primary)
            .chain(self.fallbacks.iter())
            .collect();

        for (provider_idx, provider) in providers.iter().enumerate() {
            let provider_name = provider.provider_name();
            let cb = self.circuit_breakers.get(provider_name).unwrap();

            // 熔断器检查
            if cb.allow_request().is_err() {
                tracing::debug!("{} 熔断中，跳过", provider_name);
                continue;
            }

            // 重试循环（针对当前 Provider）
            let mut attempt = 0u32;
            loop {
                attempt += 1;
                match provider.chat_stream(req.clone()).await {
                    Ok(stream) => {
                        cb.on_success();
                        if provider_idx > 0 {
                            // 通知用户已切换到备用 Provider
                            self.app.emit("provider_switched", json!({
                                "from": self.primary.provider_name(),
                                "to": provider_name,
                                "reason": "主 Provider 不可用",
                            })).ok();
                        }
                        return Ok(stream);
                    }
                    Err(e) => {
                        cb.on_failure();
                        let err: ProviderError = e.downcast().unwrap_or(ProviderError::Internal("unknown".into()));

                        if !err.is_retryable() && !err.should_failover() {
                            return Err(err.into()); // Fatal：不可重试也不可切换
                        }
                        if err.should_failover() && !err.is_retryable() {
                            break; // 跳出重试，换下一个 Provider
                        }
                        // is_retryable() == true → 退避后重试
                        if attempt >= self.retry_policy.max_attempts {
                            break; // 当前 Provider 重试耗尽，换下一个
                        }
                        let retry_after = if let ProviderError::RateLimited { retry_after_secs, .. } = &err {
                            *retry_after_secs
                        } else { None };
                        let delay = self.retry_policy.delay_for(attempt, retry_after);
                        tracing::warn!("{} 第 {} 次失败，{:?} 后重试: {:?}",
                            provider_name, attempt, delay, err);
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }

        Err(anyhow::anyhow!("所有 Provider 均不可用，请检查网络和 API Key"))
    }
}
```

---

## 5. 流式响应中断恢复

流式响应（SSE）在接收过程中可能中断，处理策略：

> **注意**：流式中断在权威 `ProviderError` 中映射为 `Network` 错误。`StreamInterrupted` 是 `agent-runtime` 内部的运行时扩展结构，携带已接收的部分内容用于续传决策，不属于 `agent-types` 的公共 API。

```rust
// crates/agent-runtime/src/provider/stream_recovery.rs

pub struct StreamRecoveryHandler {
    partial_content: String,
    received_chunks: usize,
    min_chunks_to_resume: usize,  // 至少收到 N 个 chunk 才尝试续传
}

impl StreamRecoveryHandler {
    pub async fn handle_stream_interrupted(
        &mut self,
        err: &ProviderError,
        provider: &Arc<dyn TextClient>,
        original_req: &ChatRequest,
    ) -> anyhow::Result<ChatStream> {
        if let ProviderError::StreamInterrupted { received_chunks, partial_content } = err {
            if *received_chunks >= self.min_chunks_to_resume
                && !partial_content.is_empty()
            {
                // 续传：将已接收内容作为 assistant 消息附加到历史，请求继续生成
                let mut resume_req = original_req.clone();
                resume_req.messages.push(Message::assistant(partial_content.clone()));
                resume_req.messages.push(Message::user(
                    "[系统：上一条回复因网络中断而截断，请从断点处继续完成，\
                     不要重复已有内容，直接接续]".into()
                ));
                resume_req.max_tokens = original_req.max_tokens
                    .map(|t| t.saturating_sub(estimate_tokens_str(partial_content)));

                tracing::info!("流式响应中断后续传（已收 {} chunk）", received_chunks);
                return provider.chat_stream(resume_req).await;
            }
        }

        // 无法续传（内容太少）→ 完整重试
        provider.chat_stream(original_req.clone()).await
    }
}
```

---

## 6. Rate Limit 专项处理

各 Provider 的 Rate Limit 响应格式各异，统一解析为 `RateLimited` 错误：

```rust
// crates/agent-runtime/src/providers/rate_limit_parser.rs

pub fn parse_rate_limit(status: u16, headers: &HeaderMap, body: &str) -> Option<ProviderError> {
    if status != 429 { return None; }

    // 解析 retry-after（秒数或 HTTP 日期）
    let retry_after = headers.get("retry-after")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok().or_else(|| parse_http_date_to_secs(s)));

    Some(ProviderError::RateLimited { retry_after_secs: retry_after })
}

// 各 Provider 的 429 响应体格式映射
// Anthropic: { "error": { "type": "rate_limit_error", "message": "..." } }
// OpenAI:    { "error": { "code": "rate_limit_exceeded", "message": "..." } }
// Google:    { "error": { "status": "RESOURCE_EXHAUSTED", "message": "..." } }
// MiniMax:   base_resp.status_code = 1002（特殊处理，见 memory 中的 MiniMax 文档）
```

---

## 7. Provider 健康探测

```rust
// crates/agent-runtime/src/provider/health.rs

pub struct ProviderHealthPoller {
    providers: Vec<Arc<dyn TextClient>>,
    health: Arc<DashMap<String, ProviderHealth>>,
    interval: Duration,  // 默认 60s
}

#[derive(Debug, Clone)]
pub struct ProviderHealth {
    pub status: HealthStatus,
    pub latency_p50_ms: u64,   // 最近 10 次成功请求的 P50 延迟
    pub error_rate_5m: f32,    // 最近 5 分钟错误率
    pub last_checked: Instant,
}

impl ProviderHealthPoller {
    pub async fn run(self: Arc<Self>) {
        loop {
            tokio::time::sleep(self.interval).await;
            for provider in &self.providers {
                let name = provider.provider_name().to_string();
                let health = self.probe(provider).await;
                self.health.insert(name, health);
            }
        }
    }

    async fn probe(&self, provider: &Arc<dyn TextClient>) -> ProviderHealth {
        let start = Instant::now();
        // 发送最小化探测请求（1 token 输入，1 token 输出）
        let result = tokio::time::timeout(
            Duration::from_secs(10),
            provider.complete(CompletionRequest {
                model: provider.model_id().into(),
                messages: vec![Message::user("hi".into())],
                max_tokens: 1,
                ..Default::default()
            })
        ).await;

        let latency_ms = start.elapsed().as_millis() as u64;
        match result {
            Ok(Ok(_)) => ProviderHealth {
                status: HealthStatus::Ok,
                latency_p50_ms: latency_ms,
                error_rate_5m: 0.0,
                last_checked: Instant::now(),
            },
            Ok(Err(e)) => ProviderHealth {
                status: HealthStatus::Degraded(e.to_string()),
                latency_p50_ms: latency_ms,
                error_rate_5m: 1.0,
                last_checked: Instant::now(),
            },
            Err(_) => ProviderHealth {
                status: HealthStatus::Unavailable("探测超时".into()),
                latency_p50_ms: 10_000,
                error_rate_5m: 1.0,
                last_checked: Instant::now(),
            },
        }
    }
}
```

---

## 8. 全链路降级策略

当所有在线 Provider 均失败时，按顺序尝试降级：

```text
全部 Provider 失败
    │
    ├─ 有 Ollama sidecar 且当前任务可用小模型 →  降级到本地 Ollama
    │   （见 06-离线能力.md）
    │
    ├─ 任务是纯文本生成（无工具）→  尝试降级模型
    │   （如 claude-sonnet-5 → claude-haiku-4-5）
    │
    ├─ 任务在对话中途 →  向用户展示错误，保留已有上下文
    │   等待用户决定是否重试或切换 Provider
    │
    └─ 完全无法继续 →  emit "provider_all_failed" 事件
       前端展示详细错误 + 快速跳转到 Provider 设置的按钮
```

```rust
pub async fn degraded_fallback(
    req: &ChatRequest,
    providers: &ProviderRegistry,
    ollama: Option<&OllamaClient>,
    app: &AppHandle,
) -> anyhow::Result<ChatStream> {
    // 1. 尝试 Ollama 本地模型
    if let Some(ollama) = ollama {
        if ollama.health_check().await == HealthStatus::Ok {
            tracing::warn!("所有云端 Provider 失败，降级到 Ollama 本地模型");
            app.emit("provider_degraded", json!({
                "reason": "所有云端 Provider 不可用",
                "fallback": "本地 Ollama 模型（能力受限）"
            })).ok();
            return ollama.chat_stream(req.clone()).await;
        }
    }

    // 2. 降级模型（如果当前模型有降级映射）
    if let Some(fallback_model) = FALLBACK_MODEL_MAP.get(req.model.as_str()) {
        if let Ok(client) = providers.text_client(fallback_model) {
            let mut fallback_req = req.clone();
            fallback_req.model = fallback_model.to_string();
            if let Ok(stream) = client.chat_stream(fallback_req).await {
                return Ok(stream);
            }
        }
    }

    // 3. 全部失败
    app.emit("provider_all_failed", json!({
        "message": "所有 AI Provider 均不可用，请检查网络连接和 API Key 配置"
    })).ok();
    Err(anyhow::anyhow!("所有 Provider 均不可用"))
}

static FALLBACK_MODEL_MAP: Lazy<HashMap<&str, &str>> = Lazy::new(|| {
    [
        ("claude-sonnet-5",  "claude-haiku-4-5-20251001"),
        ("gpt-4o",           "gpt-4o-mini"),
        ("gemini-2.5-pro",   "gemini-2.5-flash"),
    ].iter().cloned().collect()
});
```

---

## 9. 超时配置

```rust
pub struct TimeoutConfig {
    pub connect_timeout: Duration,         // 建立 TCP 连接：5s
    pub first_token_timeout: Duration,     // 等待第一个 token：30s
    pub inter_token_timeout: Duration,     // token 间最大间隔：15s（流式响应心跳检测）
    pub total_request_timeout: Duration,   // 整个请求总超时：300s
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            connect_timeout: Duration::from_secs(5),
            first_token_timeout: Duration::from_secs(30),
            inter_token_timeout: Duration::from_secs(15),
            total_request_timeout: Duration::from_secs(300),
        }
    }
}
```

---

## 10. 可观测性集成

```rust
// 所有重试/切换/熔断事件写入 audit_logs
pub async fn log_failover_event(pool: &SqlitePool, event: &FailoverEvent) {
    sqlx::query!(
        "INSERT INTO audit_logs (id, action, metadata, created_at)
         VALUES (?, 'provider_failover', ?, ?)",
        Uuid::new_v4().to_string(),
        serde_json::to_string(&json!({
            "event_type": event.event_type,  // "retry" | "switch" | "circuit_open" | "degraded"
            "provider": event.provider,
            "attempt": event.attempt,
            "error": event.error,
            "fallback_provider": event.fallback_provider,
            "delay_ms": event.delay_ms,
        })).unwrap(),
        now_ms(),
    ).execute(pool).await.ok();
}
```

Provider 健康仪表板（设置界面）：

```typescript
// apps/desktop/src/components/settings/ProviderHealth.tsx

export function ProviderHealthDashboard() {
  const providers = useProviderStore(s => s.healthMap);

  return (
    <div className="provider-health">
      {Object.entries(providers).map(([name, health]) => (
        <div key={name} className="provider-row">
          <HealthDot status={health.status} />
          <span className="name">{name}</span>
          <span className="latency">P50: {health.latencyP50Ms}ms</span>
          <span className="error-rate">错误率: {(health.errorRate5m * 100).toFixed(1)}%</span>
          {health.status !== "ok" && (
            <span className="reason">{health.statusMessage}</span>
          )}
        </div>
      ))}
    </div>
  );
}
```

---

## 11. 设计约束

- **重试不超过 3 次**：含首次请求，最多 3 次尝试（2 次重试）；超出则切换 Provider 或终止
- **切换 Provider 时重用相同 Context**：切换时不丢失已压缩的上下文，直接用新 Provider 发送相同的 messages
- **熔断不影响 UI 响应性**：熔断器判断是同步内存操作（μs 级），不阻塞
- **Rate Limit 不触发 Provider 切换**：Rate Limit 是配额问题而非 Provider 故障，优先等待退避而非切换（避免同一任务在多个 Provider 上消费费用）
- **探测请求费用**：每 60s 探测一次，每次约 1 token 输入 + 1 token 输出，全年费用 < $1（所有 Provider 合计）

---

## 相关文档

- [03-错误处理与容错.md](../../03-系统设计阶段/03-基础设施/03-错误处理与容错.md) — 错误分类体系总览，本文补充实现算法
- [01-多模态Provider.md](../../03-系统设计阶段/02-核心功能模块/01-多模态Provider.md) — Provider 接入实现（`TextClient` trait）
- [06-离线能力.md](../../03-系统设计阶段/06-桌面端/06-离线能力.md) — Ollama sidecar 降级方案（全链路降级的最后一步）
- [08-成本预算控制.md](../../03-系统设计阶段/03-基础设施/08-成本预算控制.md) — 切换 Provider 时费用记录到正确的 model 字段
- [02-可观测性.md](../../03-系统设计阶段/03-基础设施/02-可观测性.md) — failover 事件写入 audit_logs，P99 延迟追踪
