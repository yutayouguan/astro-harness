//! 危险 shell / SQL 命令启发式检测。

use regex::Regex;

/// 基于预置正则判断命令是否高危。
pub struct DangerousCommandDetector {
    /// 编译后的危险模式列表。
    patterns: Vec<Regex>,
}

impl DangerousCommandDetector {
    /// 加载内置危险模式（`rm -rf`、管道到 shell、`DROP TABLE` 等）。
    pub fn new() -> Self {
        let patterns = vec![
            r"rm\s+-[rRf]*f[rR]*\s+",
            r"mkfs\.",
            r"dd\s+.*of=/dev/",
            r"DROP\s+TABLE|TRUNCATE\s+TABLE",
            r">\s*/etc/",
            r"systemctl\s+(stop|disable|mask)",
            r"(curl|wget)[^|]*\|\s*(bash|sh|zsh)",
        ].iter().map(|p| Regex::new(p).unwrap()).collect();
        DangerousCommandDetector { patterns }
    }

    /// 若任一模式匹配则视为危险。
    pub fn is_dangerous(&self, command: &str) -> bool {
        self.patterns.iter().any(|p| p.is_match(command))
    }
}

impl Default for DangerousCommandDetector {
    /// 等价于 [`DangerousCommandDetector::new`]。
    fn default() -> Self { Self::new() }
}
