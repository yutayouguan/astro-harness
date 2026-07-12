//! 权限引擎：封装危险命令检测，对外提供「是否可执行」判断。

use crate::dangerous::DangerousCommandDetector;

/// 命令级权限检查入口。
pub struct PermissionEngine {
    /// 危险命令检测器。
    detector: DangerousCommandDetector,
}

impl PermissionEngine {
    /// 创建带默认危险模式的引擎。
    pub fn new() -> Self {
        PermissionEngine { detector: DangerousCommandDetector::new() }
    }

    /// 命令不匹配危险模式时返回 `true`（允许）。
    pub fn check_command(&self, cmd: &str) -> bool {
        !self.detector.is_dangerous(cmd)
    }
}

impl Default for PermissionEngine {
    /// 等价于 [`PermissionEngine::new`]。
    fn default() -> Self { Self::new() }
}
