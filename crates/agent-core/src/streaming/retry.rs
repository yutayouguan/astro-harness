use std::time::Duration;

const DEFAULT_STREAM_MAX_RETRIES: usize = 5;
const INITIAL_DELAY_MS: u64 = 200;
const BACKOFF_FACTOR: f64 = 2.0;
const INITIAL_CONNECTION_RETRY_DELAY: Duration = Duration::from_secs(5);
const MAX_CONNECTION_RETRY_DELAY: Duration = Duration::from_secs(60);

pub(super) const MAX_STREAM_RETRIES: usize = DEFAULT_STREAM_MAX_RETRIES;

fn jitter_factor() -> f64 {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    0.9 + (nanos % 200) as f64 / 1000.0
}

pub(super) fn backoff(attempt: usize) -> Duration {
    let exp = BACKOFF_FACTOR.powi(attempt.saturating_sub(1) as i32);
    let base = (INITIAL_DELAY_MS as f64 * exp) as u64;
    Duration::from_millis((base as f64 * jitter_factor()) as u64)
}

pub(super) fn connection_backoff(attempt: usize) -> Duration {
    let exp = 2u64.saturating_pow(attempt.saturating_sub(1) as u32);
    let raw = INITIAL_CONNECTION_RETRY_DELAY
        .as_millis()
        .saturating_mul(exp as u128) as u64;
    Duration::from_millis(raw.min(MAX_CONNECTION_RETRY_DELAY.as_millis() as u64))
}

const RETRYABLE_PATTERNS: &[&str] = &[
    "connection refused",
    "connection reset",
    "connection closed",
    "connection error",
    "connection timed out",
    "connect error",
    "broken pipe",
    "network",
    "timeout",
    "timed out",
    "unexpected eof",
    "early eof",
    "incomplete message",
    "reset by peer",
    "速率限制",
    "rate limit",
    "429",
    "500 internal",
    "502 bad gateway",
    "503 service",
    "504 gateway",
];

const PERMANENT_PATTERNS: &[&str] = &[
    "认证失败",
    "auth",
    "unauthorized",
    "forbidden",
    "api key",
    "invalid_api_key",
    "quota",
    "billing",
    "not found",
    "未知 provider",
    "不支持",
    "unsupported",
];

const CONNECTION_PATTERNS: &[&str] = &[
    "connection refused",
    "connection reset",
    "connection closed",
    "connection error",
    "connection timed out",
    "connect error",
    "network is unreachable",
    "network unreachable",
    "no route to host",
    "dns",
    "name resolution",
    "resolve",
];

pub(super) fn is_retryable(err: &str) -> bool {
    let lower = err.to_lowercase();
    if PERMANENT_PATTERNS.iter().any(|p| lower.contains(p)) {
        return false;
    }
    RETRYABLE_PATTERNS.iter().any(|p| lower.contains(p))
}

pub(super) fn is_connection_failure(err: &str) -> bool {
    let lower = err.to_lowercase();
    CONNECTION_PATTERNS.iter().any(|p| lower.contains(p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_increases_exponentially() {
        let d1 = backoff(1);
        let d3 = backoff(3);
        assert!(d3 > d1, "d3={d3:?} should be > d1={d1:?}");
    }

    #[test]
    fn connection_backoff_caps_at_max() {
        let d = connection_backoff(100);
        assert!(d <= MAX_CONNECTION_RETRY_DELAY);
    }

    #[test]
    fn classifies_retryable_errors() {
        assert!(is_retryable("connection refused"));
        assert!(is_retryable("request timeout after 30s"));
        assert!(is_retryable("HTTP 429 Too Many Requests"));
        assert!(is_retryable("502 Bad Gateway"));
        assert!(is_retryable("速率限制，建议 1000ms 后重试"));
    }

    #[test]
    fn classifies_permanent_errors() {
        assert!(!is_retryable("认证失败: invalid key"));
        assert!(!is_retryable("unauthorized"));
        assert!(!is_retryable("quota exceeded"));
        assert!(!is_retryable("unsupported capability"));
    }

    #[test]
    fn classifies_connection_failures() {
        assert!(is_connection_failure("connection refused"));
        assert!(is_connection_failure("dns resolution failed"));
        assert!(is_connection_failure("network is unreachable"));
        assert!(!is_connection_failure("timeout"));
        assert!(!is_connection_failure("rate limit"));
    }
}
