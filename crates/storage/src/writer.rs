//! The single storage writer.
//!
//! One dedicated thread owns the [`Store`]; every read and write is a job sent
//! to it over a bounded queue, so SQLite work never blocks async tasks and
//! there is exactly one writer. A full queue is reported immediately instead of
//! growing without bound.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};

use tokio::sync::oneshot;

use crate::Store;

type Job = Box<dyn FnOnce(&mut Store) + Send>;

/// Default number of queued jobs before callers get [`WriterError::Busy`].
pub const DEFAULT_QUEUE_CAPACITY: usize = 64;

/// Why a job did not run or did not reply.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriterError {
    /// The queue is full; retry later or coalesce.
    Busy,
    /// The writer thread has stopped.
    Stopped,
    /// The job was accepted but aborted before replying (it panicked).
    Lost,
}

impl fmt::Display for WriterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Busy => "the storage writer queue is full",
            Self::Stopped => "the storage writer has stopped",
            Self::Lost => "the storage request was aborted",
        })
    }
}

impl std::error::Error for WriterError {}

/// A cloneable handle for sending jobs to the writer thread.
#[derive(Debug, Clone)]
pub struct WriterHandle {
    jobs: SyncSender<Job>,
}

impl WriterHandle {
    /// Queues a job and returns a receiver for its result. Await the receiver
    /// from async code, or use [`WriterHandle::call`] from a plain thread.
    ///
    /// # Errors
    ///
    /// [`WriterError::Busy`] when the queue is full, [`WriterError::Stopped`]
    /// when the writer has exited.
    pub fn submit<T, F>(&self, job: F) -> Result<oneshot::Receiver<T>, WriterError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Store) -> T + Send + 'static,
    {
        let (reply, receiver) = oneshot::channel();
        let job: Job = Box::new(move |store| {
            // The caller may have stopped waiting; that is not an error here.
            let _ = reply.send(job(store));
        });
        self.jobs.try_send(job).map_err(|error| match error {
            TrySendError::Full(_) => WriterError::Busy,
            TrySendError::Disconnected(_) => WriterError::Stopped,
        })?;
        Ok(receiver)
    }

    /// Queues a job and blocks until it finishes. Do not call from an async
    /// runtime thread; await [`WriterHandle::submit`] instead.
    ///
    /// # Errors
    ///
    /// See [`WriterHandle::submit`]; [`WriterError::Lost`] if the job panicked.
    pub fn call<T, F>(&self, job: F) -> Result<T, WriterError>
    where
        T: Send + 'static,
        F: FnOnce(&mut Store) -> T + Send + 'static,
    {
        self.submit(job)?
            .blocking_recv()
            .map_err(|_| WriterError::Lost)
    }
}

/// Starts the writer thread. It runs until every handle is dropped, then
/// returns the store from its join handle.
///
/// # Errors
///
/// Returns an error if the thread cannot be spawned.
pub fn spawn(store: Store, capacity: usize) -> std::io::Result<(WriterHandle, JoinHandle<Store>)> {
    let (jobs, queue) = mpsc::sync_channel::<Job>(capacity.max(1));
    let thread = thread::Builder::new()
        .name("reopen-store-writer".to_owned())
        .spawn(move || {
            let mut store = store;
            for job in queue {
                // A panicking job drops its open transaction (rolling it back) and
                // its reply sender; the writer keeps serving later jobs.
                let _ = catch_unwind(AssertUnwindSafe(|| job(&mut store)));
            }
            store
        })?;
    Ok((WriterHandle { jobs }, thread))
}
