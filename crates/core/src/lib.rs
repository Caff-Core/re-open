//! Domain types and policies for Re-Open.
//!
//! This crate has no Tauri or OS-specific dependencies. Real domain types
//! arrive in task 1.1; this module is the bootstrap smoke slice.

use std::fmt;
use std::str::FromStr;

/// Capture state of one provider or source within a checkpoint.
///
/// See `docs/DATA_MODEL.md`, "Source states". Missing, stale and unknown
/// data stay distinct: a disconnected browser is not an empty browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SourceState {
    Complete,
    Partial,
    Unavailable,
    Stale,
    Excluded,
    Unsupported,
}

impl SourceState {
    pub const ALL: [Self; 6] = [
        Self::Complete,
        Self::Partial,
        Self::Unavailable,
        Self::Stale,
        Self::Excluded,
        Self::Unsupported,
    ];

    /// Stable wire and storage name.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Partial => "partial",
            Self::Unavailable => "unavailable",
            Self::Stale => "stale",
            Self::Excluded => "excluded",
            Self::Unsupported => "unsupported",
        }
    }

    /// Whether this state prevents a checkpoint from being labelled complete.
    ///
    /// Disabled or explicitly excluded providers do not count as failures.
    #[must_use]
    pub const fn blocks_completeness(self) -> bool {
        !matches!(self, Self::Complete | Self::Excluded)
    }
}

impl fmt::Display for SourceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Error returned when parsing an unknown source state name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownSourceState(pub String);

impl fmt::Display for UnknownSourceState {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "unknown source state: {:?}", self.0)
    }
}

impl std::error::Error for UnknownSourceState {}

impl FromStr for SourceState {
    type Err = UnknownSourceState;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|state| state.as_str() == s)
            .ok_or_else(|| UnknownSourceState(s.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip() {
        for state in SourceState::ALL {
            assert_eq!(state.as_str().parse::<SourceState>(), Ok(state));
        }
    }

    #[test]
    fn unknown_name_is_rejected() {
        assert_eq!(
            "Complete".parse::<SourceState>(),
            Err(UnknownSourceState("Complete".to_owned()))
        );
    }

    #[test]
    fn only_complete_and_excluded_allow_completeness() {
        let allowed: Vec<_> = SourceState::ALL
            .into_iter()
            .filter(|state| !state.blocks_completeness())
            .collect();
        assert_eq!(allowed, [SourceState::Complete, SourceState::Excluded]);
    }
}
