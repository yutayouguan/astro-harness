//! 委派 / 编排嵌套深度（task_local）。

use std::future::Future;

/// 默认：仅顶层（depth=0）可再派一层；子 Agent 为叶子。
pub const DEFAULT_MAX_SPAWN_DEPTH: u32 = 1;

/// 当前执行上下文的嵌套深度。
#[derive(Debug, Clone, Copy)]
pub struct SpawnDepthCtx {
    /// 当前 Agent 深度（顶层聊天为 0）。
    pub depth: u32,
    /// 允许发起嵌套的最大 caller depth。
    pub max_depth: u32,
}

impl SpawnDepthCtx {
    pub fn for_child(parent: &SpawnDepthCtx) -> Self {
        Self {
            depth: parent.depth.saturating_add(1),
            max_depth: parent.max_depth.max(1),
        }
    }

    pub fn from_caller(caller_depth: u32, max_spawn_depth: u32) -> Self {
        Self {
            depth: caller_depth.saturating_add(1),
            max_depth: if max_spawn_depth == 0 {
                DEFAULT_MAX_SPAWN_DEPTH
            } else {
                max_spawn_depth
            },
        }
    }

    /// 当前深度已达上限，不可再嵌套。
    pub fn is_leaf(&self) -> bool {
        self.depth >= self.max_depth
    }
}

tokio::task_local! {
    static SPAWN_DEPTH_CTX: SpawnDepthCtx;
}

/// 当前深度；未注入时视为顶层 0。
pub fn current_spawn_depth() -> u32 {
    SPAWN_DEPTH_CTX.try_with(|c| c.depth).unwrap_or(0)
}

/// 有效 max；未注入时用默认。
pub fn effective_max_spawn_depth() -> u32 {
    SPAWN_DEPTH_CTX
        .try_with(|c| c.max_depth)
        .unwrap_or(DEFAULT_MAX_SPAWN_DEPTH)
}

/// 是否允许再调用 delegate / orchestration_run。
pub fn can_spawn_nested() -> bool {
    current_spawn_depth() < effective_max_spawn_depth()
}

/// 在子 Agent / 编排步内注入深度上下文。
pub async fn scope_spawn_depth<F, R>(ctx: SpawnDepthCtx, f: F) -> R
where
    F: Future<Output = R>,
{
    SPAWN_DEPTH_CTX.scope(ctx, f).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_can_spawn_with_default() {
        assert_eq!(current_spawn_depth(), 0);
        assert!(can_spawn_nested());
    }

    #[tokio::test]
    async fn leaf_cannot_spawn() {
        let ctx = SpawnDepthCtx {
            depth: 1,
            max_depth: 1,
        };
        let allowed = scope_spawn_depth(ctx, async { can_spawn_nested() }).await;
        assert!(!allowed);
        assert!(ctx.is_leaf());
    }

    #[tokio::test]
    async fn mid_depth_can_spawn_when_max_gt_1() {
        let ctx = SpawnDepthCtx {
            depth: 1,
            max_depth: 2,
        };
        let allowed = scope_spawn_depth(ctx, async { can_spawn_nested() }).await;
        assert!(allowed);
        assert!(!ctx.is_leaf());
    }
}
