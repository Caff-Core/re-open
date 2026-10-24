//! Restore run and action states (docs/CAPTURE_RESTORE.md, "Restore lifecycle").

use serde::{Deserialize, Serialize};

/// Lifecycle of a restore run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Running,
    /// Waiting for memory pressure to ease; resumes or is cancelled.
    Paused,
    Completed,
    Cancelled,
    /// The agent exited mid-run; reconcile before any retry.
    Interrupted,
}

impl RunState {
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Completed | Self::Cancelled | Self::Interrupted)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Paused => "paused",
            Self::Completed => "completed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

/// State of one action. `launching` is written before the side effect and
/// replaced by the result afterwards; a `launching` action found after a
/// restart becomes `uncertain` and is never replayed automatically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Pending,
    Launching,
    Restored,
    AlreadyOpen,
    Failed,
    Skipped,
    Uncertain,
}

impl ActionState {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Launching => "launching",
            Self::Restored => "restored",
            Self::AlreadyOpen => "already_open",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
            Self::Uncertain => "uncertain",
        }
    }

    /// Whether the action has a final outcome.
    #[must_use]
    pub const fn is_final(self) -> bool {
        !matches!(self, Self::Pending | Self::Launching)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_serde() {
        for state in [
            ActionState::Pending,
            ActionState::AlreadyOpen,
            ActionState::Uncertain,
        ] {
            assert_eq!(
                serde_json::to_string(&state).unwrap(),
                format!("\"{}\"", state.as_str())
            );
        }
        for state in [RunState::Queued, RunState::Interrupted] {
            assert_eq!(
                serde_json::to_string(&state).unwrap(),
                format!("\"{}\"", state.as_str())
            );
        }
        assert!(RunState::Cancelled.is_terminal() && !RunState::Paused.is_terminal());
        assert!(!ActionState::Launching.is_final() && ActionState::Uncertain.is_final());
    }
}
