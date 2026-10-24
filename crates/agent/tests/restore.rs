#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use reopen_agent::memory::LatestSample;
use reopen_agent::restore::{Disposition, RestoreError, RestorePolicy, Restorer};
use reopen_core::checkpoint::ProviderError;
use reopen_core::resource::{ApplicationV1, FolderV1};
use reopen_core::{
    ActionState, AdapterRef, CaptureReason, CheckpointId, Clock, Resource, ResourceId,
    ResourcePayload, RestoreRunId, RunState, SourceState, SourceStatus, SystemClock, Timestamp,
};
use reopen_platform::fake::{FakeBehavior, FakeProvider};
use reopen_platform::launch::{AppLauncher, LaunchError, LaunchReceipt};
use reopen_platform::memory::MemorySample;
use reopen_storage::writer::{self, DEFAULT_QUEUE_CAPACITY};
use reopen_storage::{CommitOutcome, NewCheckpoint, RunRecord, Store};
use tempfile::TempDir;
use tokio::sync::watch;

/// Records each launch with its time; fails apps listed in `failing`.
#[derive(Default)]
struct TimedLauncher {
    launches: Mutex<Vec<(String, Instant)>>,
    failing: Vec<(String, LaunchError)>,
}

impl AppLauncher for TimedLauncher {
    fn check(&self, app_id: &str) -> Result<(), LaunchError> {
        self.failing
            .iter()
            .find(|(id, _)| id == app_id)
            .map_or(Ok(()), |(_, e)| Err(*e))
    }

    fn launch(&self, app_id: &str) -> Result<LaunchReceipt, LaunchError> {
        if let Some((_, error)) = self
            .failing
            .iter()
            .find(|(id, _)| id.starts_with("runtime:") && &id[8..] == app_id)
        {
            return Err(*error);
        }
        self.check(app_id)?;
        self.launches
            .lock()
            .unwrap()
            .push((app_id.to_owned(), Instant::now()));
        Ok(LaunchReceipt {
            handle: app_id.to_owned(),
        })
    }
}

impl TimedLauncher {
    fn launched(&self) -> Vec<String> {
        self.launches
            .lock()
            .unwrap()
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }
}

fn app(id: &str) -> ResourcePayload {
    ResourcePayload::ApplicationV1(ApplicationV1 {
        app_id: id.to_owned(),
        display_name: id.to_owned(),
    })
}

fn policy() -> RestorePolicy {
    RestorePolicy {
        min_gap: Duration::from_millis(150),
        plan_ttl: Duration::from_secs(60),
        pause_below: 0.10,
        resume_above: 0.15,
        pause_poll: Duration::from_millis(20),
    }
}

struct Harness {
    _dir: TempDir,
    restorer: Arc<Restorer>,
    launcher: Arc<TimedLauncher>,
    memory: watch::Sender<LatestSample>,
    checkpoint: CheckpointId,
}

fn harness(
    payloads: Vec<ResourcePayload>,
    running: FakeBehavior,
    launcher: TimedLauncher,
    policy: RestorePolicy,
) -> Harness {
    let dir = TempDir::new().unwrap();
    let now = || Timestamp::parse("2026-10-24T10:00:00Z").unwrap();
    let mut store = Store::open(&dir.path().join("reopen.sqlite"), now()).unwrap();
    let workspace = store.ensure_default_workspace(now()).unwrap();
    let session = store.start_session(now()).unwrap().session_id;
    let resources = payloads
        .into_iter()
        .zip(0..)
        .map(|(payload, ordinal)| Resource {
            id: ResourceId::new(),
            adapter: AdapterRef {
                id: "fake".to_owned(),
                version: 1,
            },
            ordinal,
            payload,
        })
        .collect();
    let CommitOutcome::Created { id: checkpoint, .. } = store
        .commit_checkpoint(&NewCheckpoint {
            workspace_id: workspace,
            session_id: session,
            captured_at: now(),
            reason: CaptureReason::Manual,
            sources: vec![SourceStatus {
                source_key: "desktop.apps".to_owned(),
                state: SourceState::Complete,
                required: true,
                captured_at: Some(now()),
                last_seen_at: Some(now()),
                error: None,
            }],
            resources,
        })
        .unwrap()
    else {
        panic!()
    };
    let (writer, _thread) = writer::spawn(store, DEFAULT_QUEUE_CAPACITY).unwrap();
    let launcher = Arc::new(launcher);
    let (memory, memory_rx) = watch::channel(None);
    let detector = Arc::new(FakeProvider::new("desktop.apps", true, running));
    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let restorer = Arc::new(Restorer::new(
        writer,
        launcher.clone(),
        detector,
        memory_rx,
        clock,
        policy,
    ));
    Harness {
        _dir: dir,
        restorer,
        launcher,
        memory,
        checkpoint,
    }
}

async fn wait_terminal(restorer: &Restorer, run: RestoreRunId) -> RunRecord {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let record = restorer.status(run).await.unwrap();
        if record.state.is_terminal() && !restorer.cancel(run) {
            return record;
        }
        assert!(
            Instant::now() < deadline,
            "run never finished: {:?}",
            record.state
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

fn states(record: &RunRecord) -> Vec<(ActionState, Option<String>)> {
    record
        .actions
        .iter()
        .map(|a| (a.state, a.result_code.clone()))
        .collect()
}

#[tokio::test]
async fn preview_classifies_every_resource_honestly() {
    let h = harness(
        vec![
            app("a.desktop"),
            app("open.desktop"),
            app("gone.desktop"),
            ResourcePayload::FolderV1(FolderV1 {
                path: "/home/u".to_owned(),
            }),
        ],
        FakeBehavior::Complete(vec![app("open.desktop")]),
        TimedLauncher {
            failing: vec![("gone.desktop".to_owned(), LaunchError::NotInstalled)],
            ..TimedLauncher::default()
        },
        policy(),
    );
    let plan = h.restorer.preview(h.checkpoint, None).await.unwrap();
    let dispositions: Vec<_> = plan
        .actions
        .iter()
        .map(|a| {
            (
                a.label.as_str(),
                a.disposition.clone(),
                a.possible_duplicate,
            )
        })
        .collect();
    assert_eq!(
        dispositions,
        [
            ("a.desktop", Disposition::Launch, false),
            ("open.desktop", Disposition::AlreadyOpen, false),
            ("gone.desktop", Disposition::Skip("not_installed"), false),
            ("/home/u", Disposition::Skip("unsupported_kind"), false),
        ]
    );
    assert!(plan.warnings.is_empty());

    let only = h
        .restorer
        .preview(h.checkpoint, Some(vec![0, 2]))
        .await
        .unwrap();
    assert_eq!(
        only.actions.iter().map(|a| a.ordinal).collect::<Vec<_>>(),
        [0, 2]
    );
    assert_eq!(
        h.restorer.preview(h.checkpoint, Some(vec![9])).await,
        Err(RestoreError::NotFound)
    );
    assert_eq!(
        h.restorer.preview(CheckpointId::new(), None).await,
        Err(RestoreError::NotFound)
    );
}

#[tokio::test]
async fn selected_apps_launch_serially_with_the_minimum_gap_and_others_do_not() {
    let h = harness(
        vec![
            app("a.desktop"),
            app("b.desktop"),
            app("c.desktop"),
            app("d.desktop"),
        ],
        FakeBehavior::Empty,
        TimedLauncher::default(),
        policy(),
    );
    let plan = h
        .restorer
        .preview(h.checkpoint, Some(vec![0, 2, 3]))
        .await
        .unwrap();
    let run = h.restorer.go(plan.id).await.unwrap();
    let record = wait_terminal(&h.restorer, run).await;
    assert_eq!(record.state, RunState::Completed);
    assert_eq!(
        h.launcher.launched(),
        ["a.desktop", "c.desktop", "d.desktop"],
        "b was not selected"
    );
    let times: Vec<_> = h
        .launcher
        .launches
        .lock()
        .unwrap()
        .iter()
        .map(|(_, at)| *at)
        .collect();
    for pair in times.windows(2) {
        assert!(
            pair[1] - pair[0] >= policy().min_gap,
            "launches closer than the minimum gap"
        );
    }
    assert!(
        record
            .actions
            .iter()
            .all(|a| a.state == ActionState::Restored && a.attempt_id.is_some())
    );
    assert_eq!(
        h.restorer.go(plan.id).await,
        Err(RestoreError::StalePlan),
        "a plan is single use"
    );
}

trait StartDefault {
    async fn go(&self, plan: uuid::Uuid) -> Result<RestoreRunId, RestoreError>;
}

impl StartDefault for Arc<Restorer> {
    async fn go(&self, plan: uuid::Uuid) -> Result<RestoreRunId, RestoreError> {
        Restorer::start(self, plan, false).await
    }
}

#[tokio::test]
async fn expired_plans_are_stale() {
    let h = harness(
        vec![app("a.desktop")],
        FakeBehavior::Empty,
        TimedLauncher::default(),
        RestorePolicy {
            plan_ttl: Duration::ZERO,
            ..policy()
        },
    );
    let plan = h.restorer.preview(h.checkpoint, None).await.unwrap();
    assert_eq!(h.restorer.go(plan.id).await, Err(RestoreError::StalePlan));
    assert!(h.launcher.launched().is_empty());
}

#[tokio::test]
async fn limited_duplicate_detection_needs_the_users_permission() {
    let limited =
        FakeBehavior::Partial(vec![app("maybe.desktop")], ProviderError::LimitedVisibility);
    let h = harness(
        vec![app("maybe.desktop"), app("other.desktop")],
        limited.clone(),
        TimedLauncher::default(),
        policy(),
    );
    let plan = h.restorer.preview(h.checkpoint, None).await.unwrap();
    assert_eq!(plan.warnings, ["duplicate_detection_limited"]);
    assert!(plan.actions[0].possible_duplicate && !plan.actions[1].possible_duplicate);

    let run = h.restorer.go(plan.id).await.unwrap();
    let record = wait_terminal(&h.restorer, run).await;
    assert_eq!(
        states(&record)[0],
        (ActionState::Skipped, Some("possible_duplicate".to_owned()))
    );
    assert_eq!(h.launcher.launched(), ["other.desktop"]);

    let h = harness(
        vec![app("maybe.desktop")],
        limited,
        TimedLauncher::default(),
        policy(),
    );
    let plan = h.restorer.preview(h.checkpoint, None).await.unwrap();
    let run = Restorer::start(&h.restorer, plan.id, true).await.unwrap();
    wait_terminal(&h.restorer, run).await;
    assert_eq!(
        h.launcher.launched(),
        ["maybe.desktop"],
        "allowed explicitly"
    );
}

#[tokio::test]
async fn cancellation_stops_future_launches_and_reports_completed_ones() {
    let h = harness(
        vec![app("a.desktop"), app("b.desktop"), app("c.desktop")],
        FakeBehavior::Empty,
        TimedLauncher::default(),
        RestorePolicy {
            min_gap: Duration::from_secs(5),
            ..policy()
        },
    );
    let plan = h.restorer.preview(h.checkpoint, None).await.unwrap();
    let run = h.restorer.go(plan.id).await.unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while h.launcher.launched().is_empty() {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(h.restorer.cancel(run));
    let record = wait_terminal(&h.restorer, run).await;
    assert_eq!(record.state, RunState::Cancelled);
    assert_eq!(
        states(&record),
        [
            (ActionState::Restored, None),
            (ActionState::Skipped, Some("cancelled".to_owned())),
            (ActionState::Skipped, Some("cancelled".to_owned())),
        ]
    );
    assert_eq!(h.launcher.launched(), ["a.desktop"]);
}

fn memory(available: u64) -> LatestSample {
    Some(Ok(MemorySample {
        total_bytes: 100,
        available_bytes: available,
        pressure: None,
    }))
}

#[tokio::test]
async fn memory_pressure_pauses_until_it_eases_past_the_resume_threshold() {
    let h = harness(
        vec![app("a.desktop")],
        FakeBehavior::Empty,
        TimedLauncher::default(),
        policy(),
    );
    h.memory.send(memory(5)).unwrap();
    let plan = h.restorer.preview(h.checkpoint, None).await.unwrap();
    let run = h.restorer.go(plan.id).await.unwrap();

    let deadline = Instant::now() + Duration::from_secs(5);
    while h.restorer.status(run).await.unwrap().state != RunState::Paused {
        assert!(Instant::now() < deadline, "run never paused");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    h.memory.send(memory(12)).unwrap(); // Above pause, below resume: stays paused.
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(
        h.restorer.status(run).await.unwrap().state,
        RunState::Paused
    );
    assert!(h.launcher.launched().is_empty());

    h.memory.send(memory(40)).unwrap();
    let record = wait_terminal(&h.restorer, run).await;
    assert_eq!(record.state, RunState::Completed);
    assert_eq!(h.launcher.launched(), ["a.desktop"]);
}

#[tokio::test]
async fn one_restore_runs_at_a_time_and_launch_failures_are_recorded() {
    let h = harness(
        vec![app("a.desktop"), app("broken.desktop")],
        FakeBehavior::Empty,
        TimedLauncher {
            failing: vec![("runtime:broken.desktop".to_owned(), LaunchError::Failed)],
            ..TimedLauncher::default()
        },
        RestorePolicy {
            min_gap: Duration::from_millis(300),
            ..policy()
        },
    );
    let first = h.restorer.preview(h.checkpoint, None).await.unwrap();
    let second = h.restorer.preview(h.checkpoint, None).await.unwrap();
    let run = h.restorer.go(first.id).await.unwrap();
    assert_eq!(
        h.restorer.go(second.id).await,
        Err(RestoreError::InProgress)
    );
    let record = wait_terminal(&h.restorer, run).await;
    assert_eq!(
        states(&record)[1],
        (ActionState::Failed, Some("failed".to_owned()))
    );
    assert_eq!(record.state, RunState::Completed);
}
