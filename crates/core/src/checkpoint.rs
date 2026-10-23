//! Checkpoint-level status: why it was captured, and how complete it is.

use serde::{Deserialize, Serialize};

use crate::source::SourceState;
use crate::time::Timestamp;

/// Why a checkpoint was captured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaptureReason {
    /// The periodic capture cycle.
    Scheduled,
    /// The user asked for it ("Save now"); retained until explicit deletion.
    Manual,
    /// A graceful agent shutdown.
    Shutdown,
}

/// Stable, log-safe reason a provider did not produce complete data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderError {
    Timeout,
    Unavailable,
    LimitExceeded,
    MemoryPressure,
    Failed,
}

/// The state of one provider or source within a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceStatus {
    /// Stable key, for example `desktop.apps` or `browser:<source id>`.
    pub source_key: String,
    pub state: SourceState,
    /// Whether this provider must succeed for the checkpoint to be complete.
    pub required: bool,
    /// When this source's data was captured. For `stale` sources this is the
    /// original capture time of the retained data, never the checkpoint time.
    pub captured_at: Option<Timestamp>,
    /// When the source was last observed alive, if ever.
    pub last_seen_at: Option<Timestamp>,
    pub error: Option<ProviderError>,
}

/// Why a [`SourceStatus`] is internally inconsistent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidSourceStatus {
    /// Complete, partial and stale data must say when it was captured.
    MissingCaptureTime,
    /// A complete source cannot carry a provider error.
    ErrorOnComplete,
}

impl SourceStatus {
    /// # Errors
    ///
    /// Returns [`InvalidSourceStatus`] when the fields contradict the state.
    pub const fn validate(&self) -> Result<(), InvalidSourceStatus> {
        let has_data = matches!(
            self.state,
            SourceState::Complete | SourceState::Partial | SourceState::Stale
        );
        if has_data && self.captured_at.is_none() {
            return Err(InvalidSourceStatus::MissingCaptureTime);
        }
        if matches!(self.state, SourceState::Complete) && self.error.is_some() {
            return Err(InvalidSourceStatus::ErrorOnComplete);
        }
        Ok(())
    }
}

/// Whether a checkpoint may be advertised as complete.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Completeness {
    Complete,
    Partial,
}

impl Completeness {
    /// A checkpoint is complete only when every required provider succeeded
    /// and at least one source actually produced complete data. Excluded
    /// sources never count as failures; optional sources do not block.
    #[must_use]
    pub fn evaluate(sources: &[SourceStatus]) -> Self {
        let required_ok = sources
            .iter()
            .filter(|source| source.required)
            .all(|source| !source.state.blocks_completeness());
        let any_complete = sources
            .iter()
            .any(|source| source.state == SourceState::Complete);
        if required_ok && any_complete {
            Self::Complete
        } else {
            Self::Partial
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts() -> Timestamp {
        Timestamp::parse("2026-10-23T12:00:00Z").unwrap()
    }

    fn source(key: &str, state: SourceState, required: bool) -> SourceStatus {
        let captured_at = match state {
            SourceState::Complete | SourceState::Partial | SourceState::Stale => Some(ts()),
            _ => None,
        };
        SourceStatus {
            source_key: key.to_owned(),
            state,
            required,
            captured_at,
            last_seen_at: captured_at,
            error: None,
        }
    }

    #[test]
    fn all_required_complete_is_complete() {
        let sources = [
            source("desktop.apps", SourceState::Complete, true),
            source("browser:a", SourceState::Complete, true),
        ];
        assert_eq!(Completeness::evaluate(&sources), Completeness::Complete);
    }

    #[test]
    fn required_failure_or_staleness_is_partial() {
        for state in [
            SourceState::Partial,
            SourceState::Unavailable,
            SourceState::Stale,
            SourceState::Unsupported,
        ] {
            let sources = [
                source("desktop.apps", SourceState::Complete, true),
                source("browser:a", state, true),
            ];
            assert_eq!(
                Completeness::evaluate(&sources),
                Completeness::Partial,
                "{state}"
            );
        }
    }

    #[test]
    fn excluded_and_optional_sources_do_not_block() {
        let sources = [
            source("desktop.apps", SourceState::Complete, true),
            source("browser:private", SourceState::Excluded, true),
            source("browser:b", SourceState::Unavailable, false),
        ];
        assert_eq!(Completeness::evaluate(&sources), Completeness::Complete);
    }

    #[test]
    fn nothing_captured_is_never_complete() {
        assert_eq!(Completeness::evaluate(&[]), Completeness::Partial);
        let excluded_only = [source("browser:private", SourceState::Excluded, true)];
        assert_eq!(
            Completeness::evaluate(&excluded_only),
            Completeness::Partial
        );
    }

    #[test]
    fn source_status_validation() {
        let mut stale = source("browser:a", SourceState::Stale, true);
        assert_eq!(stale.validate(), Ok(()));
        stale.captured_at = None;
        assert_eq!(
            stale.validate(),
            Err(InvalidSourceStatus::MissingCaptureTime)
        );

        let mut complete = source("desktop.apps", SourceState::Complete, true);
        complete.error = Some(ProviderError::Timeout);
        assert_eq!(
            complete.validate(),
            Err(InvalidSourceStatus::ErrorOnComplete)
        );

        assert_eq!(
            source("browser:b", SourceState::Unavailable, true).validate(),
            Ok(())
        );
    }
}
