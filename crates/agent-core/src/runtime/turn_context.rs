//! Turn-scoped immutable runtime state.
//!
//! The naming and ownership boundary follow Codex's `TurnContext`: values in
//! this structure remain fixed for one user turn, while request-scoped values
//! belong to [`super::StepContext`].

use std::path::{Path, PathBuf};

/// Immutable state shared by every sampling step in one user turn.
#[derive(Debug)]
pub struct TurnContext {
    /// Stable identifier for the active turn.
    pub(crate) sub_id: String,
    /// One-based user-turn ordinal within the session.
    pub(crate) turn: usize,
    /// Interaction mode admitted when the turn started.
    pub(crate) mode: types::InteractionMode,
    /// Permission profile admitted when the turn started.
    pub(crate) permission_profile: Option<String>,
    /// Project root admitted when the turn started.
    pub(crate) project_root: Option<PathBuf>,
}

impl TurnContext {
    pub(crate) fn new(
        sub_id: String,
        turn: usize,
        mode: types::InteractionMode,
        permission_profile: Option<String>,
        project_root: Option<PathBuf>,
    ) -> Self {
        Self {
            sub_id,
            turn,
            mode,
            permission_profile,
            project_root,
        }
    }

    pub fn sub_id(&self) -> &str {
        &self.sub_id
    }

    pub fn turn(&self) -> usize {
        self.turn
    }

    pub fn mode(&self) -> types::InteractionMode {
        self.mode
    }

    pub fn permission_profile(&self) -> Option<&str> {
        self.permission_profile.as_deref()
    }

    pub fn project_root(&self) -> Option<&Path> {
        self.project_root.as_deref()
    }
}
