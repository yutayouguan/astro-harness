use serde::{Deserialize, Serialize};

/// Controls how the compact token limit is measured.
///
/// - `Total`: the limit applies to the entire conversation (default).
/// - `BodyAfterPrefix`: the limit applies only to the body after the
///   system-prompt prefix, allowing the prefix to be excluded from
///   budget accounting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum CompactTokenLimitScope {
    #[default]
    Total,
    BodyAfterPrefix,
}
