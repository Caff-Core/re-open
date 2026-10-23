//! The memory sampling loop, on its own schedule independent of capture.

use std::sync::Arc;
use std::time::Duration;

use reopen_core::checkpoint::ProviderError;
use reopen_platform::memory::{MemorySample, MemorySampler};
use tokio::sync::watch;
use tokio::time::{MissedTickBehavior, interval};

/// Default time between memory samples.
pub const DEFAULT_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);

/// The most recent sampling result, or `None` before the first sample.
pub type LatestSample = Option<Result<MemorySample, ProviderError>>;

/// Samples memory every `period` on the blocking pool and publishes the latest
/// result. Readers always see the newest sample; nothing accumulates. Returns
/// when `shutdown` becomes true or every reader is gone.
pub async fn run(
    sampler: Arc<dyn MemorySampler>,
    period: Duration,
    publish: watch::Sender<LatestSample>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut ticker = interval(period);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = shutdown.changed() => return,
            _ = ticker.tick() => {}
        }
        if *shutdown.borrow() {
            return;
        }
        let sampler = Arc::clone(&sampler);
        let result = tokio::task::spawn_blocking(move || sampler.sample())
            .await
            .unwrap_or(Err(ProviderError::Failed));
        if publish.send(Some(result)).is_err() {
            return;
        }
    }
}
