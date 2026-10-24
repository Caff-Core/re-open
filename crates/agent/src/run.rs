//! Agent startup, steady state and shutdown.
//!
//! Order matters: take the singleton lock, open and migrate storage, protect
//! the previous session's recovery candidate and record the new session, and
//! only then bind the socket (which is the readiness signal) and enable capture.

use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use reopen_core::checkpoint::ProviderError;
use reopen_core::resource::ApplicationV1;
use reopen_core::{CaptureReason, Clock, ResourcePayload, SystemClock};
use reopen_platform::CaptureProvider;
use reopen_platform::fake::{FakeBehavior, FakeProvider};
use reopen_platform::launch::{AppLauncher, FakeLauncher};
use reopen_platform::memory::SystemMemorySampler;
use reopen_storage::writer::{self, DEFAULT_QUEUE_CAPACITY};
use reopen_storage::{CommitOutcome, RetentionPolicy, Store, StoreError};
use tokio::sync::watch;

use crate::capture::{CaptureEngine, CapturePolicy, CaptureTarget};
use crate::ipc::{handler::DesktopHandler, server};
use crate::lock::{AgentLock, LockError};
use crate::paths::{AgentPaths, PathError};
use crate::restore::{RestorePolicy, Restorer};
use crate::{memory, scheduler};

/// What the fake provider reports (Phase 1 runs on fakes only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FakeScenario {
    Complete,
    Partial,
    Unavailable,
    Empty,
}

impl FakeScenario {
    fn provider(self) -> FakeProvider {
        let sample = || {
            vec![
                ResourcePayload::ApplicationV1(ApplicationV1 {
                    app_id: "org.example.Editor".to_owned(),
                    display_name: "Example Editor".to_owned(),
                }),
                ResourcePayload::ApplicationV1(ApplicationV1 {
                    app_id: "org.example.Browser".to_owned(),
                    display_name: "Example Browser".to_owned(),
                }),
            ]
        };
        let behavior = match self {
            Self::Complete => FakeBehavior::Complete(sample()),
            Self::Partial => FakeBehavior::Partial(sample(), ProviderError::LimitExceeded),
            Self::Unavailable => FakeBehavior::Fail(ProviderError::Unavailable),
            Self::Empty => FakeBehavior::Empty,
        };
        FakeProvider::new("desktop.apps", true, behavior)
    }
}

/// Which desktop provider captures applications.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderChoice {
    /// GNOME window list via the extension, with the systemd-unit fallback (ADR-016).
    Linux,
    /// Scripted fake data, for development and tests.
    Fake(FakeScenario),
}

#[derive(Debug, Clone, Copy)]
pub struct RunOptions {
    pub provider: ProviderChoice,
    pub capture_interval: Duration,
}

/// The platform pieces the agent composes for one provider choice.
struct Adapters {
    providers: Vec<Arc<dyn CaptureProvider>>,
    launcher: Arc<dyn AppLauncher>,
    /// Answers "is this app already open?" for restore.
    detector: Arc<dyn CaptureProvider>,
}

impl Adapters {
    fn for_choice(choice: ProviderChoice) -> Self {
        let exclusions = reopen_platform_linux::ExclusionPolicy::default();
        match choice {
            ProviderChoice::Linux => Self {
                providers: vec![Arc::new(
                    reopen_platform_linux::GnomeAppsProvider::from_env(exclusions.clone()),
                )],
                launcher: Arc::new(reopen_platform_linux::LinuxLauncher::from_env(
                    exclusions.clone(),
                )),
                detector: Arc::new(reopen_platform_linux::GnomeAppsProvider::from_env(
                    exclusions,
                )),
            },
            // Fakes never launch anything, and nothing is reported as already open.
            ProviderChoice::Fake(scenario) => Self {
                providers: vec![Arc::new(scenario.provider())],
                launcher: Arc::new(FakeLauncher::default()),
                detector: Arc::new(FakeProvider::new("desktop.apps", true, FakeBehavior::Empty)),
            },
        }
    }
}

/// Why the agent stopped abnormally. Each maps to a distinct exit code.
#[derive(Debug)]
pub enum RunError {
    Paths(PathError),
    Lock(LockError),
    /// Storage is damaged or from a newer version; nothing was written.
    Storage(StoreError),
    Io(std::io::Error),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Paths(error) => error.fmt(f),
            Self::Lock(error) => error.fmt(f),
            Self::Storage(error) => write!(f, "{error}; capture is disabled"),
            Self::Io(error) => write!(f, "agent I/O error: {}", error.kind()),
        }
    }
}

impl std::error::Error for RunError {}

impl RunError {
    #[must_use]
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Paths(_) | Self::Io(_) => 1,
            Self::Lock(_) => 3,
            Self::Storage(_) => 4,
        }
    }
}

/// Runs the agent until `shutdown_signal` resolves, then shuts down cleanly:
/// finishes the current cycle, marks the session clean and removes the socket.
///
/// # Errors
///
/// [`RunError`] when startup fails; existing data is never modified then.
pub async fn run(
    paths: &AgentPaths,
    options: RunOptions,
    shutdown_signal: impl Future<Output = ()>,
) -> Result<(), RunError> {
    paths.prepare().map_err(RunError::Paths)?;
    let _lock = AgentLock::acquire(&paths.lock_file()).map_err(RunError::Lock)?;
    // A second logger install (tests running several agents) is harmless.
    let _ = crate::logging::init(&paths.log_dir());

    let clock: Arc<dyn Clock> = Arc::new(SystemClock::new());
    let mut store = Store::open(&paths.database(), clock.now_utc()).map_err(|error| {
        tracing::error!(event = "storage.unusable", reason = %error);
        RunError::Storage(error)
    })?;
    let workspace_id = store
        .ensure_default_workspace(clock.now_utc())
        .map_err(RunError::Storage)?;
    let start = store
        .start_session(clock.now_utc())
        .map_err(RunError::Storage)?;
    tracing::info!(
        event = "agent.session_started",
        session = %start.session_id,
        previous_clean_exit = start.previous.map(|previous| previous.clean_exit),
        new_candidate = start.new_candidate.is_some(),
    );
    // Before any new restore can start: runs left unfinished by a crash are
    // marked interrupted and mid-launch actions uncertain, never replayed.
    let reconciled = store
        .reconcile_interrupted_restores(clock.now_utc())
        .map_err(RunError::Storage)?;
    if reconciled > 0 {
        tracing::warn!(event = "restore.reconciled", runs = reconciled);
    }

    let (writer, writer_thread) =
        writer::spawn(store, DEFAULT_QUEUE_CAPACITY).map_err(RunError::Io)?;
    let (trigger, capture_shutdown, control) = scheduler::control();
    let (stop_tx, stop_rx) = watch::channel(false);

    let Adapters {
        providers,
        launcher,
        detector,
    } = Adapters::for_choice(options.provider);
    let (memory_tx, memory_rx) = watch::channel(None);
    let restorer = Arc::new(Restorer::new(
        writer.clone(),
        launcher,
        detector,
        memory_rx,
        Arc::clone(&clock),
        RestorePolicy::default(),
    ));

    let handler = Arc::new(DesktopHandler {
        writer: writer.clone(),
        trigger,
        session_id: start.session_id,
        restorer,
    });
    let listener = server::bind(&paths.desktop_socket()).map_err(RunError::Io)?;
    tracing::info!(event = "agent.ready");

    let engine = Arc::new(CaptureEngine::new(
        providers,
        CapturePolicy::default(),
        Arc::clone(&clock),
    ));
    let target = CaptureTarget {
        writer: writer.clone(),
        workspace_id,
        session_id: start.session_id,
    };
    let capture_loop = scheduler::run(options.capture_interval, control, |reason| {
        let engine = Arc::clone(&engine);
        let target = target.clone();
        let clock = Arc::clone(&clock);
        async move { capture_cycle(&engine, &target, clock.as_ref(), reason).await }
    });

    let memory_loop = memory::run(
        Arc::new(SystemMemorySampler::new()),
        memory::DEFAULT_SAMPLE_INTERVAL,
        memory_tx,
        stop_rx.clone(),
    );
    let server_loop = server::serve(listener, handler, stop_rx);

    let stop = async {
        shutdown_signal.await;
        tracing::info!(event = "agent.stopping");
        capture_shutdown.stop();
        let _ = stop_tx.send(true);
    };
    tokio::join!(capture_loop, memory_loop, server_loop, stop);

    let _ = std::fs::remove_file(paths.desktop_socket());
    let session = start.session_id;
    let now = clock.now_utc();
    let marked = match writer.submit(move |store| store.mark_clean_exit(session, now)) {
        Ok(reply) => matches!(reply.await, Ok(Ok(()))),
        Err(_) => false,
    };
    if marked {
        tracing::info!(event = "agent.stopped", clean_exit = true);
    } else {
        tracing::warn!(event = "agent.stopped", clean_exit = false);
    }
    drop(target);
    drop(writer);
    drop(engine);
    let _ = writer_thread.join();
    Ok(())
}

async fn capture_cycle(
    engine: &CaptureEngine,
    target: &CaptureTarget,
    clock: &dyn Clock,
    reason: CaptureReason,
) {
    match engine.capture_and_commit(target, reason).await {
        Ok(CommitOutcome::Created { completeness, .. }) => {
            tracing::info!(
                event = "capture.committed",
                outcome = "created",
                ?reason,
                ?completeness
            );
            let now = clock.now_utc();
            let policy = RetentionPolicy::default();
            if let Ok(reply) = target
                .writer
                .submit(move |store| store.apply_retention(&policy, now))
            {
                match reply.await {
                    Ok(Ok(report)) => tracing::info!(
                        event = "retention.applied",
                        deleted = report.deleted,
                        over_budget = report.over_budget,
                        budget_blocked = report.budget_blocked,
                    ),
                    _ => tracing::warn!(event = "retention.failed"),
                }
            }
        }
        Ok(CommitOutcome::Unchanged { .. }) => {
            tracing::info!(event = "capture.committed", outcome = "unchanged", ?reason);
        }
        Err(error) => tracing::warn!(event = "capture.failed", ?reason, reason_code = %error),
    }
}
