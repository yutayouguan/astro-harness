//! Provider 层结构化错误类型。
//!
//! 调用方可按枚举变体精确匹配错误类别（如重试 vs 直接失败），
//! 而非对 `anyhow::Error` 做字符串匹配。

use thiserror::Error;

/// Provider 操作错误。
#[derive(Debug, Error)]
pub enum ProviderError {
    /// 未知或未注册的 provider id。
    #[error("未知 provider: {0}")]
    UnknownProvider(String),

    /// provider 不支持请求的能力（TTS / 视频 / 音乐 / 嵌入等）。
    #[error("{provider} 不支持{capability}")]
    UnsupportedCapability {
        provider: String,
        capability: String,
    },

    /// 认证失败（API key 无效或过期）。
    #[error("{provider} 认证失败: {detail}")]
    AuthFailed { provider: String, detail: String },

    /// 速率限制，建议在 `retry_after_ms` 后重试。
    #[error("{provider} 速率限制，建议 {retry_after_ms}ms 后重试")]
    RateLimited {
        provider: String,
        retry_after_ms: u64,
    },

    /// 请求超时。
    #[error("{operation} 超时: {detail}")]
    Timeout { operation: String, detail: String },

    /// 模型返回异常（非结构化错误、空响应等）。
    #[error("{provider} 模型错误: {detail}")]
    ModelError { provider: String, detail: String },

    /// 其他透传错误。
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

/// Provider 操作结果。
pub type ProviderResult<T> = Result<T, ProviderError>;
