//! 渐进式信任模型：同一 agent 连续被批准同类能力后，自动提升信任等级。
//!
//! 信任分数仅在 session 内有效，不跨会话持久化。

use std::collections::HashMap;

use tokio::sync::Mutex;

const UPGRADE_TO_SMART: u32 = 5;
const UPGRADE_TO_AUTO: u32 = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TrustLevel {
    #[default]
    AlwaysAsk,
    SmartReview,
    AutoApprove,
}

/// 按命令前缀索引的信任分数。
#[derive(Debug, Clone, Default)]
pub struct TrustScore {
    pub consecutive_approvals: u32,
    pub level: TrustLevel,
}

/// 会话级信任评估器。
pub struct TrustEvaluator {
    scores: Mutex<HashMap<String, TrustScore>>,
}

impl Default for TrustEvaluator {
    fn default() -> Self {
        Self::new()
    }
}

impl TrustEvaluator {
    pub fn new() -> Self {
        Self {
            scores: Mutex::new(HashMap::new()),
        }
    }

    /// 查询当前命令前缀的信任等级。
    pub async fn trust_level(&self, command_prefix: &str) -> TrustLevel {
        self.scores
            .lock()
            .await
            .get(command_prefix)
            .map(|s| s.level)
            .unwrap_or_default()
    }

    /// 记录一次审批批准，可能升级信任等级。
    pub async fn record_approval(&self, command_prefix: &str) -> TrustLevel {
        let mut scores = self.scores.lock().await;
        let score = scores.entry(command_prefix.to_string()).or_default();
        score.consecutive_approvals += 1;
        score.level = match score.consecutive_approvals {
            n if n >= UPGRADE_TO_AUTO => TrustLevel::AutoApprove,
            n if n >= UPGRADE_TO_SMART => TrustLevel::SmartReview,
            _ => TrustLevel::AlwaysAsk,
        };
        score.level
    }

    /// 记录一次审批拒绝，立即回退到 AlwaysAsk。
    pub async fn record_denial(&self, command_prefix: &str) {
        let mut scores = self.scores.lock().await;
        let score = scores.entry(command_prefix.to_string()).or_default();
        score.consecutive_approvals = 0;
        score.level = TrustLevel::AlwaysAsk;
    }

    /// 重置指定命令前缀的信任。
    pub async fn reset(&self, command_prefix: &str) {
        self.scores.lock().await.remove(command_prefix);
    }

    /// 重置所有信任分数。
    pub async fn reset_all(&self) {
        self.scores.lock().await.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn starts_at_always_ask() {
        let eval = TrustEvaluator::new();
        assert_eq!(eval.trust_level("cargo test").await, TrustLevel::AlwaysAsk);
    }

    #[tokio::test]
    async fn upgrades_to_smart_after_threshold() {
        let eval = TrustEvaluator::new();
        for _ in 0..UPGRADE_TO_SMART - 1 {
            eval.record_approval("cargo test").await;
        }
        assert_eq!(eval.trust_level("cargo test").await, TrustLevel::AlwaysAsk);
        eval.record_approval("cargo test").await;
        assert_eq!(
            eval.trust_level("cargo test").await,
            TrustLevel::SmartReview
        );
    }

    #[tokio::test]
    async fn upgrades_to_auto_after_threshold() {
        let eval = TrustEvaluator::new();
        for _ in 0..UPGRADE_TO_AUTO {
            eval.record_approval("cargo test").await;
        }
        assert_eq!(
            eval.trust_level("cargo test").await,
            TrustLevel::AutoApprove
        );
    }

    #[tokio::test]
    async fn denial_resets_to_always_ask() {
        let eval = TrustEvaluator::new();
        for _ in 0..UPGRADE_TO_SMART + 1 {
            eval.record_approval("cargo test").await;
        }
        assert_eq!(
            eval.trust_level("cargo test").await,
            TrustLevel::SmartReview
        );
        eval.record_denial("cargo test").await;
        assert_eq!(eval.trust_level("cargo test").await, TrustLevel::AlwaysAsk);
    }

    #[tokio::test]
    async fn reset_clears_score() {
        let eval = TrustEvaluator::new();
        for _ in 0..UPGRADE_TO_SMART {
            eval.record_approval("cargo test").await;
        }
        eval.reset("cargo test").await;
        assert_eq!(eval.trust_level("cargo test").await, TrustLevel::AlwaysAsk);
    }

    #[tokio::test]
    async fn different_prefixes_are_independent() {
        let eval = TrustEvaluator::new();
        for _ in 0..UPGRADE_TO_SMART {
            eval.record_approval("cargo test").await;
        }
        assert_eq!(
            eval.trust_level("cargo test").await,
            TrustLevel::SmartReview
        );
        assert_eq!(eval.trust_level("npm run").await, TrustLevel::AlwaysAsk);
    }

    #[tokio::test]
    async fn reset_all_clears_everything() {
        let eval = TrustEvaluator::new();
        for _ in 0..UPGRADE_TO_SMART {
            eval.record_approval("cargo test").await;
            eval.record_approval("npm run").await;
        }
        eval.reset_all().await;
        assert_eq!(eval.trust_level("cargo test").await, TrustLevel::AlwaysAsk);
        assert_eq!(eval.trust_level("npm run").await, TrustLevel::AlwaysAsk);
    }
}
