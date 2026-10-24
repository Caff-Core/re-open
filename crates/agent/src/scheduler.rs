//! The capture loop: periodic ticks plus explicit "Save now" requests, run one
//! at a time.

use std::future::Future;
use std::time::Duration;

use reopen_core::CaptureReason;
use tokio::sync::{mpsc, watch};
use tokio::time::{MissedTickBehavior, interval};

/// Default time between scheduled captures.
pub const DEFAULT_INTERVAL: Duration = Duration::from_mins(1);

/// Requests an immediate manual capture. Requests made while one is already
/// pending coalesce into it.
#[derive(Debug, Clone)]
pub struct CaptureTrigger(mpsc::Sender<()>);

impl CaptureTrigger {
    /// Returns `false` when a request was already pending or the loop stopped.
    pub fn request(&self) -> bool {
        self.0.try_send(()).is_ok()
    }
}

/// Stops the capture loop after its current cycle.
#[derive(Debug)]
pub struct Shutdown(watch::Sender<bool>);

impl Shutdown {
    pub fn stop(&self) {
        let _ = self.0.send(true);
    }
}

/// The loop's inputs, created together.
#[derive(Debug)]
pub struct LoopControl {
    requests: mpsc::Receiver<()>,
    shutdown: watch::Receiver<bool>,
}

#[must_use]
pub fn control() -> (CaptureTrigger, Shutdown, LoopControl) {
    let (request_tx, requests) = mpsc::channel(1);
    let (shutdown_tx, shutdown) = watch::channel(false);
    (
        CaptureTrigger(request_tx),
        Shutdown(shutdown_tx),
        LoopControl { requests, shutdown },
    )
}

/// Runs captures until shutdown. Cycles never overlap: a cycle is awaited
/// before the next starts, ticks that fall due meanwhile are skipped rather
/// than queued, and a manual capture restarts the interval.
pub async fn run<F, Fut>(period: Duration, mut control: LoopControl, mut cycle: F)
where
    F: FnMut(CaptureReason) -> Fut,
    Fut: Future<Output = ()>,
{
    let mut ticker = interval(period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        if *control.shutdown.borrow() {
            return;
        }
        tokio::select! {
            biased;
            _ = control.shutdown.changed() => return,
            request = control.requests.recv() => {
                if request.is_none() {
                    return;
                }
                cycle(CaptureReason::Manual).await;
                ticker.reset();
            }
            _ = ticker.tick() => cycle(CaptureReason::Scheduled).await,
        }
    }
}
