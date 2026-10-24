//! Restore: preview a plan, then execute it serially with a durable journal.
//!
//! A preview is an in-memory plan with an ID and an expiry; starting it
//! journals every action before anything launches. Each launch is preceded by
//! a committed attempt marker, paced at least `min_gap` apart, preceded by a
//! memory check, and skipped if the app is already open. Cancellation stops
//! future launches and reports completed ones honestly.

use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use reopen_core::checkpoint::ProviderError;
use reopen_core::{
    ActionState, AttemptId, CheckpointId, Clock, ResourceId, ResourceKind, ResourcePayload,
    RestoreActionId, RestoreRunId, RunState,
};
use reopen_platform::launch::AppLauncher;
use reopen_platform::{CaptureProvider, CaptureState};
use reopen_storage::{NewAction, RunRecord, Store, StoreError, WriterHandle};
use tokio::sync::watch;
use tokio::time::Instant;
use uuid::Uuid;

use crate::memory::LatestSample;

/// Restore pacing and limits (docs/CAPTURE_RESTORE.md, "Execution policy").
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestorePolicy {
    /// Minimum time between two launches.
    pub min_gap: Duration,
    /// How long a preview stays valid.
    pub plan_ttl: Duration,
    /// Pause when available memory falls below this fraction of total…
    pub pause_below: f64,
    /// …and resume only once it is back above this one (hysteresis).
    pub resume_above: f64,
    /// How often a paused run rechecks memory.
    pub pause_poll: Duration,
}

impl Default for RestorePolicy {
    fn default() -> Self {
        Self {
            min_gap: Duration::from_secs(2),
            plan_ttl: Duration::from_mins(5),
            pause_below: 0.10,
            resume_above: 0.15,
            pause_poll: Duration::from_secs(1),
        }
    }
}

/// Which apps are open now, and whether that view can be trusted for
/// duplicate detection.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RunningApps {
    pub app_ids: HashSet<String>,
    /// Running apps are known but not their windows (limited visibility), so
    /// "already open" is only a possibility.
    pub limited: bool,
}

/// Reads running apps through a capture provider.
#[must_use]
pub fn running_apps(provider: &dyn CaptureProvider) -> Option<RunningApps> {
    let capture = provider.capture().ok()?;
    let app_ids = capture
        .resources
        .iter()
        .filter_map(|payload| match payload {
            ResourcePayload::ApplicationV1(app) => Some(app.app_id.clone()),
            _ => None,
        })
        .collect();
    let limited = capture.state == CaptureState::Partial(ProviderError::LimitedVisibility);
    Some(RunningApps { app_ids, limited })
}

/// What a plan will do with one resource.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposition {
    Launch,
    AlreadyOpen,
    /// Will not launch, with a stable reason.
    Skip(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedAction {
    pub ordinal: u32,
    pub resource_id: ResourceId,
    pub kind: ResourceKind,
    /// The app ID (or path/address) shown to the user; never logged.
    pub label: String,
    pub disposition: Disposition,
    /// Open-ness could not be ruled out; launched only if the user allows it.
    pub possible_duplicate: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub id: Uuid,
    pub checkpoint_id: CheckpointId,
    pub actions: Vec<PlannedAction>,
    /// Stable warning codes, for example `duplicate_detection_limited`.
    pub warnings: Vec<&'static str>,
    pub expires_at: Instant,
}

/// Why a restore request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RestoreError {
    /// The plan does not exist, was used, or expired.
    StalePlan,
    /// The checkpoint or a selected resource does not exist.
    NotFound,
    /// Another restore is still running.
    InProgress,
    /// Storage could not be reached.
    Storage,
}

fn label(payload: &ResourcePayload) -> &str {
    match payload {
        ResourcePayload::ApplicationV1(app) => &app.app_id,
        ResourcePayload::ProjectV1(project) => &project.path,
        ResourcePayload::FolderV1(folder) => &folder.path,
        ResourcePayload::BrowserTabV1(tab) => &tab.url,
    }
}

struct ActiveRun {
    id: RestoreRunId,
    cancel: Arc<AtomicBool>,
}

/// Plans and executes restores. One run at a time.
pub struct Restorer {
    writer: WriterHandle,
    launcher: Arc<dyn AppLauncher>,
    detector: Arc<dyn CaptureProvider>,
    memory: watch::Receiver<LatestSample>,
    clock: Arc<dyn Clock>,
    policy: RestorePolicy,
    plans: Mutex<HashMap<Uuid, Plan>>,
    active: Mutex<Option<ActiveRun>>,
}

impl std::fmt::Debug for Restorer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Restorer")
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl Restorer {
    #[must_use]
    pub fn new(
        writer: WriterHandle,
        launcher: Arc<dyn AppLauncher>,
        detector: Arc<dyn CaptureProvider>,
        memory: watch::Receiver<LatestSample>,
        clock: Arc<dyn Clock>,
        policy: RestorePolicy,
    ) -> Self {
        Self {
            writer,
            launcher,
            detector,
            memory,
            clock,
            policy,
            plans: Mutex::new(HashMap::new()),
            active: Mutex::new(None),
        }
    }

    async fn store<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Store) -> Result<T, StoreError> + Send + 'static,
    ) -> Result<T, RestoreError> {
        let reply = self.writer.submit(job).map_err(|_| RestoreError::Storage)?;
        match reply.await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(StoreError::NotFound)) => Err(RestoreError::NotFound),
            _ => Err(RestoreError::Storage),
        }
    }

    async fn running(&self) -> Option<RunningApps> {
        let detector = Arc::clone(&self.detector);
        tokio::task::spawn_blocking(move || running_apps(detector.as_ref()))
            .await
            .ok()
            .flatten()
    }

    /// Builds a plan for the selected ordinals (all resources when `None`).
    ///
    /// # Errors
    ///
    /// [`RestoreError::NotFound`] for an unknown checkpoint or ordinal.
    pub async fn preview(
        &self,
        checkpoint: CheckpointId,
        only: Option<Vec<u32>>,
    ) -> Result<Plan, RestoreError> {
        let detail = self
            .store(move |store| store.checkpoint_detail(checkpoint))
            .await?;
        if let Some(only) = &only
            && only
                .iter()
                .any(|ordinal| detail.resources.iter().all(|r| r.ordinal != *ordinal))
        {
            return Err(RestoreError::NotFound);
        }
        let running = self.running().await;
        let mut warnings = Vec::new();
        match &running {
            None => warnings.push("duplicate_detection_unavailable"),
            Some(running) if running.limited => warnings.push("duplicate_detection_limited"),
            Some(_) => {}
        }

        let mut actions = Vec::new();
        for resource in &detail.resources {
            if only
                .as_ref()
                .is_some_and(|only| !only.contains(&resource.ordinal))
            {
                continue;
            }
            let label = label(&resource.payload).to_owned();
            let mut possible_duplicate = false;
            let disposition = if resource.kind() != ResourceKind::Application {
                Disposition::Skip("unsupported_kind")
            } else if let Err(error) = self.launcher.check(&label) {
                Disposition::Skip(error.as_str())
            } else {
                match &running {
                    Some(running) if running.app_ids.contains(&label) && !running.limited => {
                        Disposition::AlreadyOpen
                    }
                    Some(running) if !running.app_ids.contains(&label) => Disposition::Launch,
                    // Seen with limited visibility, or no detection at all.
                    _ => {
                        possible_duplicate = true;
                        Disposition::Launch
                    }
                }
            };
            actions.push(PlannedAction {
                ordinal: resource.ordinal,
                resource_id: resource.id,
                kind: resource.kind(),
                label,
                disposition,
                possible_duplicate,
            });
        }

        let plan = Plan {
            id: Uuid::new_v4(),
            checkpoint_id: checkpoint,
            actions,
            warnings,
            expires_at: Instant::now() + self.policy.plan_ttl,
        };
        let mut plans = self.plans.lock().unwrap_or_else(PoisonError::into_inner);
        let now = Instant::now();
        plans.retain(|_, plan| plan.expires_at > now);
        plans.insert(plan.id, plan.clone());
        Ok(plan)
    }

    /// Journals and starts a previewed plan. Possible duplicates launch only
    /// when `allow_possible_duplicates` is set; otherwise they are skipped.
    ///
    /// # Errors
    ///
    /// [`RestoreError::StalePlan`], [`RestoreError::InProgress`] or storage errors.
    pub async fn start(
        self: &Arc<Self>,
        plan_id: Uuid,
        allow_possible_duplicates: bool,
    ) -> Result<RestoreRunId, RestoreError> {
        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
            if active.is_some() {
                return Err(RestoreError::InProgress);
            }
            // Reserve the slot; the run ID is set once the run is journaled.
            *active = Some(ActiveRun {
                id: RestoreRunId::from_uuid(Uuid::nil()),
                cancel: Arc::clone(&cancel),
            });
        }
        let release = || *self.active.lock().unwrap_or_else(PoisonError::into_inner) = None;
        let plan = {
            let mut plans = self.plans.lock().unwrap_or_else(PoisonError::into_inner);
            match plans.remove(&plan_id) {
                Some(plan) if plan.expires_at > Instant::now() => plan,
                _ => {
                    drop(plans);
                    release();
                    return Err(RestoreError::StalePlan);
                }
            }
        };

        let journal: Vec<NewAction> = plan
            .actions
            .iter()
            .map(|action| {
                let (initial, result_code) = match action.disposition {
                    Disposition::Launch
                        if action.possible_duplicate && !allow_possible_duplicates =>
                    {
                        (ActionState::Skipped, Some("possible_duplicate".to_owned()))
                    }
                    Disposition::Launch => (ActionState::Pending, None),
                    Disposition::AlreadyOpen => (ActionState::AlreadyOpen, None),
                    Disposition::Skip(reason) => (ActionState::Skipped, Some(reason.to_owned())),
                };
                NewAction {
                    ordinal: action.ordinal,
                    resource_id: action.resource_id,
                    initial,
                    result_code,
                }
            })
            .collect();
        let checkpoint = plan.checkpoint_id;
        let now = self.clock.now_utc();
        let entries = journal.clone();
        let created = self
            .store(move |store| store.create_restore_run(checkpoint, &entries, now))
            .await;
        let (run, ids) = match created {
            Ok(created) => created,
            Err(error) => {
                release();
                return Err(error);
            }
        };
        if let Some(active) = self
            .active
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_mut()
        {
            active.id = run;
        }

        let work: Vec<(RestoreActionId, String)> = ids
            .into_iter()
            .zip(journal)
            .zip(plan.actions)
            .filter(|((_, action), _)| action.initial == ActionState::Pending)
            .map(|((id, _), planned)| (id, planned.label))
            .collect();
        tracing::info!(event = "restore.started", run = %run, actions = work.len());
        let this = Arc::clone(self);
        tokio::spawn(async move {
            let state = this.execute(run, work, &cancel).await;
            tracing::info!(event = "restore.finished", run = %run, state = state.as_str());
            *this.active.lock().unwrap_or_else(PoisonError::into_inner) = None;
        });
        Ok(run)
    }

    /// Requests cancellation of the running restore. Returns whether `run` was active.
    pub fn cancel(&self, run: RestoreRunId) -> bool {
        let active = self.active.lock().unwrap_or_else(PoisonError::into_inner);
        match active.as_ref() {
            Some(active) if active.id == run => {
                active.cancel.store(true, Ordering::Release);
                true
            }
            _ => false,
        }
    }

    /// The journaled state of a run.
    ///
    /// # Errors
    ///
    /// [`RestoreError::NotFound`] for an unknown run.
    pub async fn status(&self, run: RestoreRunId) -> Result<RunRecord, RestoreError> {
        self.store(move |store| store.restore_run(run)).await
    }

    fn memory_low(&self, threshold: f64) -> bool {
        matches!(&*self.memory.borrow(), Some(Ok(sample)) if sample.available_ratio() < threshold)
    }

    async fn set_state(&self, run: RestoreRunId, state: RunState) -> Result<(), RestoreError> {
        let now = self.clock.now_utc();
        self.store(move |store| store.set_run_state(run, state, now))
            .await
    }

    async fn finish(
        &self,
        action: RestoreActionId,
        state: ActionState,
        code: Option<&'static str>,
    ) -> Result<(), RestoreError> {
        self.store(move |store| store.finish_action(action, state, code))
            .await
    }

    /// Waits while memory is low; returns `false` if cancelled meanwhile.
    async fn wait_for_memory(
        &self,
        run: RestoreRunId,
        cancel: &AtomicBool,
    ) -> Result<bool, RestoreError> {
        if !self.memory_low(self.policy.pause_below) {
            return Ok(true);
        }
        tracing::warn!(event = "restore.paused", run = %run, reason = "memory_pressure");
        self.set_state(run, RunState::Paused).await?;
        while self.memory_low(self.policy.resume_above) {
            if cancel.load(Ordering::Acquire) {
                return Ok(false);
            }
            tokio::time::sleep(self.policy.pause_poll).await;
        }
        self.set_state(run, RunState::Running).await?;
        tracing::info!(event = "restore.resumed", run = %run);
        Ok(true)
    }

    async fn execute(
        &self,
        run: RestoreRunId,
        work: Vec<(RestoreActionId, String)>,
        cancel: &AtomicBool,
    ) -> RunState {
        let result: Result<RunState, RestoreError> = async {
            self.set_state(run, RunState::Running).await?;
            let mut last_launch: Option<Instant> = None;
            let mut cancelled = false;
            for (action, app_id) in work {
                if !cancelled && !self.wait_for_memory(run, cancel).await? {
                    cancelled = true;
                }
                if !cancelled && let Some(last) = last_launch {
                    let ready = last + self.policy.min_gap;
                    while Instant::now() < ready && !cancel.load(Ordering::Acquire) {
                        let wait = ready.saturating_duration_since(Instant::now());
                        tokio::time::sleep(wait.min(Duration::from_millis(100))).await;
                    }
                }
                cancelled |= cancel.load(Ordering::Acquire);
                if cancelled {
                    self.finish(action, ActionState::Skipped, Some("cancelled"))
                        .await?;
                    continue;
                }
                if let Some(running) = self.running().await
                    && !running.limited
                    && running.app_ids.contains(&app_id)
                {
                    self.finish(action, ActionState::AlreadyOpen, None).await?;
                    continue;
                }

                let attempt = AttemptId::new();
                self.store(move |store| store.mark_action_launching(action, attempt))
                    .await?;
                let launcher = Arc::clone(&self.launcher);
                let outcome = tokio::task::spawn_blocking(move || launcher.launch(&app_id)).await;
                last_launch = Some(Instant::now());
                match outcome {
                    Ok(Ok(_receipt)) => self.finish(action, ActionState::Restored, None).await?,
                    Ok(Err(error)) => {
                        self.finish(action, ActionState::Failed, Some(error.as_str()))
                            .await?;
                    }
                    // The launcher panicked: whether anything started is unknown.
                    Err(_) => {
                        self.finish(action, ActionState::Uncertain, Some("launcher_failed"))
                            .await?;
                    }
                }
            }
            let state = if cancelled {
                RunState::Cancelled
            } else {
                RunState::Completed
            };
            self.set_state(run, state).await?;
            Ok(state)
        }
        .await;
        // On a storage failure the run stays non-terminal and is reconciled
        // as interrupted at the next start.
        result.unwrap_or(RunState::Interrupted)
    }
}
