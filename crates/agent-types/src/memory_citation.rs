use serde::{Deserialize, Serialize};

/// A citation pointing to a specific range in a memory file.
///
/// Used to trace which lines of `MEMORY.md`, `USER.md`, or other
/// knowledge sources influenced an agent response or decision.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryCitation {
    /// Relative or absolute path to the cited file.
    pub path: String,
    /// First cited line (1-indexed, inclusive).
    pub line_start: u32,
    /// Last cited line (1-indexed, inclusive).
    pub line_end: u32,
    /// Free-form note explaining why this range is relevant.
    pub note: String,
}
