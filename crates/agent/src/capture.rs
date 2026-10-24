//! One capture cycle: run every provider under deadlines and turn the results
//! into source statuses and resources.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use reopen_core::checkpoint::ProviderError;
use reopen_core::{
    CaptureReason, Clock, Resource, ResourceId, SessionId, SourceState, SourceStatus, WorkspaceId,
};
use reopen_platform::{CaptureProvider, CaptureState, ProviderCapture};
use reopen_storage::{CommitOutcome, NewCheckpoint, StoreError, WriterError, WriterHandle};
use tokio::time::{Instant, timeout_at};

/// Provider and cycle time budgets (docs/CAPTURE_RESTORE.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CapturePolicy {
    pub provider_timeout: Duration,
    pub cycle_timeout: Duration,
}

impl Default for CapturePolicy {
    fn default() -> Self {
        Self {
            provider_timeout: Duration::from_secs(5),
            cycle_timeout: Duration::from_secs(10),
        }
    }
}

/// Sources and resources observed in one cycle, ready to commit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CycleOutput {
    pub sources: Vec<SourceStatus>,
    pub resources: Vec<Resource>,
}

struct Slot {
    provider: Arc<dyn CaptureProvider>,
    /// Set while a probe is running. A probe that overran its deadline keeps
    /// running on its worker; until it finishes, later cycles report the
    /// provider as timed out instead of starting another probe.
    in_flight: Arc<AtomicBool>,
}

/// Clears the in-flight flag when the probe finishes, even if it panics.
struct InFlight(Arc<AtomicBool>);

impl Drop for InFlight {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

/// Runs capture cycles over a fixed set of providers.
pub struct CaptureEngine {
    slots: Vec<Slot>,
    policy: CapturePolicy,
    clock: Arc<dyn Clock>,
}

impl std::fmt::Debug for CaptureEngine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CaptureEngine")
            .field("providers", &self.slots.len())
            .field("policy", &self.policy)
            .finish()
    }
}

impl CaptureEngine {
    #[must_use]
    pub fn new(
        providers: Vec<Arc<dyn CaptureProvider>>,
        policy: CapturePolicy,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let slots = providers
            .into_iter()
            .map(|provider| Slot {
                provider,
                in_flight: Arc::new(AtomicBool::new(false)),
            })
            .collect();
        Self {
            slots,
            policy,
            clock,
        }
    }

    /// Runs every provider concurrently on the blocking pool. Each gets the
    /// provider budget, capped by the cycle budget; an overrun becomes an
    /// `unavailable` source with a `timeout` error, never a missing source.
    pub async fn run_cycle(&self) -> CycleOutput {
        let started = Instant::now();
        let cycle_deadline = started + self.policy.cycle_timeout;
        let provider_deadline = (started + self.policy.provider_timeout).min(cycle_deadline);

        let mut probes = Vec::with_capacity(self.slots.len());
        for slot in &self.slots {
            if slot.in_flight.swap(true, Ordering::AcqRel) {
                probes.push(None);
                continue;
            }
            let provider = Arc::clone(&slot.provider);
            let guard = InFlight(Arc::clone(&slot.in_flight));
            probes.push(Some(tokio::task::spawn_blocking(move || {
                let _guard = guard;
                provider.capture()
            })));
        }

        let mut results = Vec::with_capacity(probes.len());
        for probe in probes {
            let result = match probe {
                None => Err(ProviderError::Timeout),
                Some(handle) => match timeout_at(provider_deadline, handle).await {
                    Ok(Ok(result)) => result,
                    Ok(Err(_panic)) => Err(ProviderError::Failed),
                    Err(_elapsed) => Err(ProviderError::Timeout),
                },
            };
            results.push(result);
        }

        let captured_at = self.clock.now_utc();
        let mut sources = Vec::with_capacity(self.slots.len());
        let mut resources = Vec::new();
        for (slot, result) in self.slots.iter().zip(results) {
            let provider = &slot.provider;
            let (state, error, captured, used_adapter) = match result {
                Ok(ProviderCapture {
                    state: CaptureState::Complete,
                    resources: found,
                    adapter,
                }) => (SourceState::Complete, None, Some(found), adapter),
                Ok(ProviderCapture {
                    state: CaptureState::Partial(error),
                    resources: found,
                    adapter,
                }) => (SourceState::Partial, Some(error), Some(found), adapter),
                Err(error) => (SourceState::Unavailable, Some(error), None, None),
            };
            let observed = captured.is_some().then_some(captured_at);
            if let Some(found) = captured {
                let adapter = used_adapter.unwrap_or_else(|| provider.adapter());
                for payload in found {
                    let ordinal = u32::try_from(resources.len()).unwrap_or(u32::MAX);
                    resources.push(Resource {
                        id: ResourceId::new(),
                        adapter: adapter.clone(),
                        ordinal,
                        payload,
                    });
                }
            }
            sources.push(SourceStatus {
                source_key: provider.source_key().to_owned(),
                state,
                required: provider.required(),
                captured_at: observed,
                last_seen_at: observed,
                error,
            });
        }
        CycleOutput { sources, resources }
    }
}

/// Why a capture did not commit.
#[derive(Debug)]
pub enum CaptureError {
    Writer(WriterError),
    Store(StoreError),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Writer(error) => write!(f, "capture not committed: {error}"),
            Self::Store(error) => write!(f, "capture not committed: {error}"),
        }
    }
}

impl std::error::Error for CaptureError {}

/// Where committed captures go.
#[derive(Debug, Clone)]
pub struct CaptureTarget {
    pub writer: WriterHandle,
    pub workspace_id: WorkspaceId,
    pub session_id: SessionId,
}

impl CaptureEngine {
    /// Runs one cycle and commits it. Success is reported only after commit.
    ///
    /// # Errors
    ///
    /// [`CaptureError`] when the writer is busy or stopped, or the commit fails;
    /// existing checkpoints are untouched in every case.
    pub async fn capture_and_commit(
        &self,
        target: &CaptureTarget,
        reason: CaptureReason,
    ) -> Result<CommitOutcome, CaptureError> {
        let output = self.run_cycle().await;
        let checkpoint = NewCheckpoint {
            workspace_id: target.workspace_id,
            session_id: target.session_id,
            captured_at: self.clock.now_utc(),
            reason,
            sources: output.sources,
            resources: output.resources,
        };
        let reply = target
            .writer
            .submit(move |store| store.commit_checkpoint(&checkpoint))
            .map_err(CaptureError::Writer)?;
        reply
            .await
            .map_err(|_| CaptureError::Writer(WriterError::Lost))?
            .map_err(CaptureError::Store)
    }
}
