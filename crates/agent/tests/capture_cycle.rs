#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::sync::Arc;
use std::time::Duration;

use reopen_agent::capture::{CaptureEngine, CapturePolicy, CaptureTarget};
use reopen_core::checkpoint::ProviderError;
use reopen_core::resource::FolderV1;
use reopen_core::{
    CaptureReason, Clock, Completeness, MonotonicInstant, ResourcePayload, SourceState, Timestamp,
};
use reopen_platform::CaptureProvider;
use reopen_platform::fake::{FakeBehavior, FakeProvider};
use reopen_storage::writer::{self, DEFAULT_QUEUE_CAPACITY};
use reopen_storage::{CommitOutcome, Store};
use tempfile::TempDir;

// Probes run on the blocking pool, which stops tokio's paused clock from
// auto-advancing, so these tests use real time with millisecond budgets.
const SHORT: CapturePolicy = CapturePolicy {
    provider_timeout: Duration::from_millis(200),
    cycle_timeout: Duration::from_secs(1),
};

struct FixedClock;

impl Clock for FixedClock {
    fn now_utc(&self) -> Timestamp {
        Timestamp::parse("2026-10-23T12:00:00Z").unwrap()
    }
    fn now_monotonic(&self) -> MonotonicInstant {
        MonotonicInstant::from_origin(Duration::ZERO)
    }
}

fn folder(path: &str) -> ResourcePayload {
    ResourcePayload::FolderV1(FolderV1 {
        path: path.to_owned(),
    })
}

fn engine(providers: &[&Arc<FakeProvider>], policy: CapturePolicy) -> CaptureEngine {
    let providers = providers
        .iter()
        .map(|p| Arc::clone(p) as Arc<dyn CaptureProvider>)
        .collect();
    CaptureEngine::new(providers, policy, Arc::new(FixedClock))
}

#[tokio::test]
async fn complete_providers_produce_complete_sources_and_ordered_resources() {
    let apps = Arc::new(FakeProvider::new(
        "desktop.apps",
        true,
        FakeBehavior::Complete(vec![folder("/a"), folder("/b")]),
    ));
    let files = Arc::new(FakeProvider::new(
        "desktop.files",
        true,
        FakeBehavior::Complete(vec![folder("/c")]),
    ));
    let output = engine(&[&apps, &files], CapturePolicy::default())
        .run_cycle()
        .await;

    assert!(
        output
            .sources
            .iter()
            .all(|s| s.state == SourceState::Complete && s.error.is_none())
    );
    assert!(output.sources.iter().all(|s| s.captured_at.is_some()));
    let ordinals: Vec<_> = output.resources.iter().map(|r| r.ordinal).collect();
    assert_eq!(ordinals, [0, 1, 2]);
    assert_eq!(output.resources[2].payload, folder("/c"));
    assert_eq!(output.resources[2].adapter.id, "fake.desktop.files");
    assert_eq!(
        Completeness::evaluate(&output.sources),
        Completeness::Complete
    );
}

#[tokio::test]
async fn partial_failed_and_empty_providers_are_reported_explicitly() {
    let partial = Arc::new(FakeProvider::new(
        "a",
        true,
        FakeBehavior::Partial(vec![folder("/a")], ProviderError::LimitExceeded),
    ));
    let failed = Arc::new(FakeProvider::new(
        "b",
        true,
        FakeBehavior::Fail(ProviderError::Unavailable),
    ));
    let empty = Arc::new(FakeProvider::new("c", true, FakeBehavior::Empty));
    let output = engine(&[&partial, &failed, &empty], CapturePolicy::default())
        .run_cycle()
        .await;

    let states: Vec<_> = output
        .sources
        .iter()
        .map(|s| (s.source_key.as_str(), s.state, s.error))
        .collect();
    assert_eq!(
        states,
        [
            (
                "a",
                SourceState::Partial,
                Some(ProviderError::LimitExceeded)
            ),
            (
                "b",
                SourceState::Unavailable,
                Some(ProviderError::Unavailable)
            ),
            ("c", SourceState::Complete, None),
        ]
    );
    assert_eq!(
        output.sources[1].captured_at, None,
        "a failed source has no capture time"
    );
    assert_eq!(output.resources.len(), 1);
    assert!(output.sources.iter().all(|s| s.validate().is_ok()));
}

#[tokio::test]
async fn an_overrunning_provider_times_out_without_blocking_the_cycle() {
    let fast = Arc::new(FakeProvider::new(
        "fast",
        true,
        FakeBehavior::Complete(vec![folder("/a")]),
    ));
    let slow = Arc::new(FakeProvider::new("slow", true, FakeBehavior::Empty));
    let (hang, release) = FakeBehavior::hang(FakeBehavior::Empty);
    slow.push(hang);
    let engine = engine(&[&fast, &slow], SHORT);

    let started = tokio::time::Instant::now();
    let output = engine.run_cycle().await;
    let elapsed = started.elapsed();
    assert!(
        elapsed >= SHORT.provider_timeout && elapsed < SHORT.cycle_timeout,
        "{elapsed:?}"
    );
    assert_eq!(output.sources[0].state, SourceState::Complete);
    assert_eq!(
        (output.sources[1].state, output.sources[1].error),
        (SourceState::Unavailable, Some(ProviderError::Timeout))
    );

    // Still stuck: the next cycle reports a timeout without starting a second probe.
    let output = engine.run_cycle().await;
    assert_eq!(output.sources[1].error, Some(ProviderError::Timeout));
    assert_eq!(slow.calls(), 1);

    release.release();
    for _ in 0..100 {
        if engine.run_cycle().await.sources[1].state == SourceState::Complete {
            assert_eq!(slow.calls(), 2);
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("provider never recovered after release");
}

#[tokio::test]
async fn the_cycle_budget_caps_every_provider() {
    let slow = Arc::new(FakeProvider::new("slow", true, FakeBehavior::Empty));
    let (hang, release) = FakeBehavior::hang(FakeBehavior::Empty);
    slow.push(hang);
    let policy = CapturePolicy {
        provider_timeout: Duration::from_secs(5),
        cycle_timeout: Duration::from_millis(200),
    };

    let started = tokio::time::Instant::now();
    let output = engine(&[&slow], policy).run_cycle().await;
    let elapsed = started.elapsed();
    assert!(
        elapsed >= policy.cycle_timeout && elapsed < policy.provider_timeout,
        "{elapsed:?}"
    );
    assert_eq!(output.sources[0].error, Some(ProviderError::Timeout));
    release.release();
}

#[tokio::test]
async fn a_panicking_provider_is_reported_as_failed_and_can_run_again() {
    #[derive(Debug)]
    struct Panics;
    impl CaptureProvider for Panics {
        fn source_key(&self) -> &str {
            "panics"
        }
        fn required(&self) -> bool {
            true
        }
        fn adapter(&self) -> reopen_core::AdapterRef {
            reopen_core::AdapterRef {
                id: "panics".to_owned(),
                version: 1,
            }
        }
        fn capabilities(&self) -> reopen_core::CapabilitySet {
            reopen_core::CapabilitySet::empty()
        }
        fn capture(&self) -> Result<reopen_platform::ProviderCapture, ProviderError> {
            panic!("probe bug")
        }
    }
    let engine = CaptureEngine::new(
        vec![Arc::new(Panics)],
        CapturePolicy::default(),
        Arc::new(FixedClock),
    );
    for _ in 0..2 {
        assert_eq!(
            engine.run_cycle().await.sources[0].error,
            Some(ProviderError::Failed)
        );
    }
}

#[tokio::test]
async fn capture_and_commit_persists_through_the_writer() {
    let dir = TempDir::new().unwrap();
    let mut store = Store::open(&dir.path().join("reopen.sqlite"), FixedClock.now_utc()).unwrap();
    let workspace = store
        .ensure_default_workspace(FixedClock.now_utc())
        .unwrap();
    let session = store
        .start_session(FixedClock.now_utc())
        .unwrap()
        .session_id;
    let (writer, thread) = writer::spawn(store, DEFAULT_QUEUE_CAPACITY).unwrap();
    let target = CaptureTarget {
        writer: writer.clone(),
        workspace_id: workspace,
        session_id: session,
    };

    let apps = Arc::new(FakeProvider::new(
        "desktop.apps",
        true,
        FakeBehavior::Complete(vec![folder("/a")]),
    ));
    let engine = engine(&[&apps], CapturePolicy::default());
    let first = engine
        .capture_and_commit(&target, CaptureReason::Scheduled)
        .await
        .unwrap();
    assert!(matches!(
        first,
        CommitOutcome::Created {
            completeness: Completeness::Complete,
            ..
        }
    ));
    let second = engine
        .capture_and_commit(&target, CaptureReason::Scheduled)
        .await
        .unwrap();
    assert!(matches!(second, CommitOutcome::Unchanged { .. }));

    drop(target);
    drop(writer);
    thread.join().unwrap();
}
