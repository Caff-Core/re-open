//! A scriptable fake provider for engine tests and the fake-only agent.

use std::collections::VecDeque;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use reopen_core::checkpoint::ProviderError;
use reopen_core::{AdapterRef, Capability, CapabilitySet, ResourcePayload};

use crate::{CaptureProvider, CaptureState, ProviderCapture};

/// What the fake does on one call to `capture`.
#[derive(Debug, Clone)]
pub enum FakeBehavior {
    Complete(Vec<ResourcePayload>),
    Partial(Vec<ResourcePayload>, ProviderError),
    /// A complete capture that found nothing; distinct from failure.
    Empty,
    Fail(ProviderError),
    /// Blocks until the matching [`HangRelease`] is triggered or dropped, then
    /// returns the inner behavior. Simulates a probe that overruns its deadline.
    Hang(Arc<Mutex<Option<Receiver<()>>>>, Box<FakeBehavior>),
    /// Sleeps for real before returning the inner behavior.
    Delay(Duration, Box<FakeBehavior>),
}

/// Releases a [`FakeBehavior::Hang`]. Dropping it releases too.
#[derive(Debug)]
pub struct HangRelease(Sender<()>);

impl HangRelease {
    pub fn release(self) {
        let _ = self.0.send(());
    }
}

impl FakeBehavior {
    /// A hanging behavior and the handle that releases it.
    #[must_use]
    pub fn hang(then: Self) -> (Self, HangRelease) {
        let (release, gate) = mpsc::channel();
        (
            Self::Hang(Arc::new(Mutex::new(Some(gate))), Box::new(then)),
            HangRelease(release),
        )
    }

    fn run(self) -> Result<ProviderCapture, ProviderError> {
        match self {
            Self::Complete(resources) => Ok(ProviderCapture {
                state: CaptureState::Complete,
                resources,
                adapter: None,
            }),
            Self::Partial(resources, error) => Ok(ProviderCapture {
                state: CaptureState::Partial(error),
                resources,
                adapter: None,
            }),
            Self::Empty => Ok(ProviderCapture {
                state: CaptureState::Complete,
                resources: Vec::new(),
                adapter: None,
            }),
            Self::Fail(error) => Err(error),
            Self::Hang(gate, then) => {
                let gate = gate.lock().unwrap_or_else(PoisonError::into_inner).take();
                if let Some(gate) = gate {
                    let _ = gate.recv();
                }
                then.run()
            }
            Self::Delay(duration, then) => {
                std::thread::sleep(duration);
                then.run()
            }
        }
    }
}

/// A provider that plays back scripted behaviors, then repeats a default.
#[derive(Debug)]
pub struct FakeProvider {
    key: String,
    required: bool,
    script: Mutex<VecDeque<FakeBehavior>>,
    default: FakeBehavior,
    calls: Mutex<usize>,
}

impl FakeProvider {
    #[must_use]
    pub fn new(key: &str, required: bool, default: FakeBehavior) -> Self {
        Self {
            key: key.to_owned(),
            required,
            script: Mutex::new(VecDeque::new()),
            default,
            calls: Mutex::new(0),
        }
    }

    /// Queues a behavior for the next call; queued behaviors run in order.
    pub fn push(&self, behavior: FakeBehavior) {
        self.script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(behavior);
    }

    #[must_use]
    pub fn calls(&self) -> usize {
        *self.calls.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl CaptureProvider for FakeProvider {
    fn source_key(&self) -> &str {
        &self.key
    }

    fn required(&self) -> bool {
        self.required
    }

    fn adapter(&self) -> AdapterRef {
        AdapterRef {
            id: format!("fake.{}", self.key),
            version: 1,
        }
    }

    fn capabilities(&self) -> CapabilitySet {
        CapabilitySet::empty().with(Capability::DiscoverApp)
    }

    fn capture(&self) -> Result<ProviderCapture, ProviderError> {
        *self.calls.lock().unwrap_or_else(PoisonError::into_inner) += 1;
        let next = self
            .script
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pop_front();
        next.unwrap_or_else(|| self.default.clone()).run()
    }
}

#[cfg(test)]
mod tests {
    use reopen_core::resource::FolderV1;

    use super::*;

    fn folder() -> ResourcePayload {
        ResourcePayload::FolderV1(FolderV1 {
            path: "/home/u".to_owned(),
        })
    }

    #[test]
    fn script_plays_in_order_then_default_repeats() {
        let fake = FakeProvider::new("desktop.apps", true, FakeBehavior::Empty);
        fake.push(FakeBehavior::Complete(vec![folder()]));
        fake.push(FakeBehavior::Fail(ProviderError::Unavailable));

        assert_eq!(fake.capture().unwrap().resources, [folder()]);
        assert_eq!(fake.capture(), Err(ProviderError::Unavailable));
        let empty = fake.capture().unwrap();
        assert_eq!(
            (empty.state, empty.resources.len()),
            (CaptureState::Complete, 0)
        );
        assert_eq!(fake.calls(), 3);
    }

    #[test]
    fn partial_carries_its_error() {
        let fake = FakeProvider::new(
            "desktop.apps",
            true,
            FakeBehavior::Partial(vec![folder()], ProviderError::LimitExceeded),
        );
        assert_eq!(
            fake.capture().unwrap().state,
            CaptureState::Partial(ProviderError::LimitExceeded)
        );
    }

    #[test]
    fn hang_blocks_until_released() {
        let (behavior, release) = FakeBehavior::hang(FakeBehavior::Empty);
        let fake = Arc::new(FakeProvider::new("desktop.apps", true, FakeBehavior::Empty));
        fake.push(behavior);
        let worker = {
            let fake = Arc::clone(&fake);
            std::thread::spawn(move || fake.capture())
        };
        std::thread::sleep(Duration::from_millis(20));
        assert!(!worker.is_finished());
        release.release();
        assert!(worker.join().unwrap().is_ok());
    }
}
