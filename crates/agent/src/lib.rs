//! Re-Open background agent.
//!
//! The agent owns scheduling, capture and storage. This library holds its
//! logic so it can be tested with fake providers; the binary (task 1.8)
//! composes it with real adapters and local IPC.

pub mod capture;
pub mod scheduler;
