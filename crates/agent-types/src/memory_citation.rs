use serde::{Deserialize, Serialize};

/// 指向记忆文件中特定行范围的引用。
///
/// 用于追溯 `MEMORY.md`、`USER.md` 或其他知识源中哪些行
/// 影响了 agent 的回复或决策。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCitation {
    /// 被引用文件的相对或绝对路径。
    pub path: String,
    /// 引用起始行（从 1 开始，包含）。
    pub line_start: u32,
    /// 引用结束行（从 1 开始，包含）。
    pub line_end: u32,
    /// 自由格式说明，解释此范围为何相关。
    pub note: String,
}
