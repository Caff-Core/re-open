#![allow(clippy::unwrap_used)] // Test harness: a panic is the failure report.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use reopen_agent::scheduler::{self, DEFAULT_INTERVAL};
use reopen_core::CaptureReason::{self, Manual, Scheduled};
use tokio::time::{Instant, sleep};

type Log = Rc<RefCell<Vec<(CaptureReason, u64)>>>;

/// A cycle that records when it started and then takes `cycle_time`.
fn recorder(
    log: &Log,
    start: Instant,
    cycle_time: Duration,
) -> impl FnMut(CaptureReason) -> std::pin::Pin<Box<dyn Future<Output = ()>>> {
    let log = Rc::clone(log);
    move |reason| {
        log.borrow_mut().push((reason, start.elapsed().as_secs()));
        Box::pin(sleep(cycle_time))
    }
}

#[tokio::test(start_paused = true)]
async fn scheduled_cycles_run_every_interval() {
    let (_trigger, shutdown, control) = scheduler::control();
    let log: Log = Rc::default();
    let start = Instant::now();
    tokio::join!(
        scheduler::run(
            DEFAULT_INTERVAL,
            control,
            recorder(&log, start, Duration::from_secs(1))
        ),
        async {
            sleep(Duration::from_secs(185)).await;
            shutdown.stop();
        }
    );
    assert_eq!(
        *log.borrow(),
        [
            (Scheduled, 0),
            (Scheduled, 60),
            (Scheduled, 120),
            (Scheduled, 180)
        ]
    );
}

#[tokio::test(start_paused = true)]
async fn a_slow_cycle_skips_missed_ticks_instead_of_queueing_them() {
    let (_trigger, shutdown, control) = scheduler::control();
    let log: Log = Rc::default();
    let start = Instant::now();
    tokio::join!(
        scheduler::run(
            DEFAULT_INTERVAL,
            control,
            recorder(&log, start, Duration::from_secs(150))
        ),
        async {
            sleep(Duration::from_secs(400)).await;
            shutdown.stop();
        }
    );
    // Ticks due at 60 and 120 fell inside the first cycle: they coalesce into
    // one catch-up cycle when it ends, and cycles never overlap.
    assert_eq!(
        *log.borrow(),
        [(Scheduled, 0), (Scheduled, 150), (Scheduled, 300)]
    );
}

#[tokio::test(start_paused = true)]
async fn save_now_requests_coalesce_and_restart_the_interval() {
    let (trigger, shutdown, control) = scheduler::control();
    let log: Log = Rc::default();
    let start = Instant::now();
    let accepted = tokio::join!(
        scheduler::run(
            DEFAULT_INTERVAL,
            control,
            recorder(&log, start, Duration::from_secs(10))
        ),
        async {
            sleep(Duration::from_secs(1)).await;
            let accepted = [trigger.request(), trigger.request(), trigger.request()];
            sleep(Duration::from_secs(100)).await;
            shutdown.stop();
            accepted
        }
    )
    .1;
    assert_eq!(accepted, [true, false, false]);
    assert_eq!(
        *log.borrow(),
        [(Scheduled, 0), (Manual, 10), (Scheduled, 80)]
    );
}

#[tokio::test(start_paused = true)]
async fn shutdown_ends_the_loop_after_the_current_cycle() {
    let (_trigger, shutdown, control) = scheduler::control();
    let log: Log = Rc::default();
    let start = Instant::now();
    tokio::join!(
        scheduler::run(
            DEFAULT_INTERVAL,
            control,
            recorder(&log, start, Duration::from_secs(5))
        ),
        async {
            sleep(Duration::from_secs(2)).await;
            shutdown.stop();
        }
    );
    assert_eq!(start.elapsed(), Duration::from_secs(5));
    assert_eq!(log.borrow().len(), 1);
}
