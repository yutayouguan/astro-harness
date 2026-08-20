use serde::{Deserialize, Serialize};

/// Per-session memory write policy.
///
/// When `Disabled`, the agent runtime skips `try_append_decision` and
/// `try_append_permission_audit` writes so that ephemeral or background
/// threads do not pollute the persistent decision log.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThreadMemoryMode {
    #[default]
    Enabled,
    Disabled,
}
