#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::sync::Arc;
use std::time::Duration;

use reopen_agent::memory::{self, LatestSample};
use reopen_core::checkpoint::ProviderError;
use reopen_platform::memory::{FakeMemorySampler, MemorySample};
use tokio::sync::watch;

fn sample(available: u64) -> MemorySample {
    MemorySample {
        total_bytes: 100,
        available_bytes: available,
        pressure: None,
    }
}

#[tokio::test]
async fn publishes_each_sample_and_stops_on_shutdown() {
    let sampler = Arc::new(FakeMemorySampler::new(vec![
        Ok(sample(50)),
        Err(ProviderError::Unavailable),
        Ok(sample(10)),
    ]));
    let (publish, mut latest) = watch::channel::<LatestSample>(None);
    let (stop, shutdown) = watch::channel(false);
    let task = tokio::spawn(memory::run(
        sampler,
        Duration::from_millis(20),
        publish,
        shutdown,
    ));

    let mut seen = Vec::new();
    while seen.len() < 3 {
        latest.changed().await.unwrap();
        seen.push(*latest.borrow_and_update());
    }
    assert_eq!(
        seen,
        [
            Some(Ok(sample(50))),
            Some(Err(ProviderError::Unavailable)),
            Some(Ok(sample(10)))
        ]
    );

    stop.send(true).unwrap();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn stops_when_no_reader_remains() {
    let sampler = Arc::new(FakeMemorySampler::new(vec![Ok(sample(50))]));
    let (publish, latest) = watch::channel::<LatestSample>(None);
    let (_stop, shutdown) = watch::channel(false);
    drop(latest);
    tokio::time::timeout(
        Duration::from_secs(1),
        memory::run(sampler, Duration::from_millis(5), publish, shutdown),
    )
    .await
    .unwrap();
}
