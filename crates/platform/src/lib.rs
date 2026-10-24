//! Platform adapter traits and a configurable fake for tests.
//!
//! Real adapters (`platform-linux`, later others) implement these traits; the
//! agent only sees the traits. A provider reports what it actually observed,
//! with an explicit state, and never guesses missing data.

use reopen_core::checkpoint::ProviderError;
use reopen_core::{AdapterRef, CapabilitySet, ResourcePayload};

pub mod fake;
pub mod memory;

/// The state a provider reports for a capture it finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    /// Everything the provider is responsible for was observed.
    Complete,
    /// Some data was observed, some was not; the error says why.
    Partial(ProviderError),
}

/// What one provider observed in one cycle.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderCapture {
    pub state: CaptureState,
    pub resources: Vec<ResourcePayload>,
    /// The adapter that produced this capture, when a provider switches
    /// between adapters (for example a primary and a fallback). Defaults to
    /// [`CaptureProvider::adapter`].
    pub adapter: Option<AdapterRef>,
}

/// A source of captured resources, such as visible desktop apps.
///
/// `capture` may block (it talks to the OS); the agent runs it on a bounded
/// worker under a deadline and treats an overrun as a timeout.
pub trait CaptureProvider: Send + Sync {
    /// Stable source key, for example `desktop.apps`.
    fn source_key(&self) -> &str;
    /// Whether this provider must succeed for a checkpoint to be complete.
    fn required(&self) -> bool;
    fn adapter(&self) -> AdapterRef;
    fn capabilities(&self) -> CapabilitySet;

    /// # Errors
    ///
    /// A [`ProviderError`] when nothing usable could be observed.
    fn capture(&self) -> Result<ProviderCapture, ProviderError>;
}
