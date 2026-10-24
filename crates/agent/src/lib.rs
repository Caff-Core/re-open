//! Re-Open background agent.
//!
//! The agent owns scheduling, capture and storage. This library holds its
//! logic so it can be tested with fake providers; `main.rs` is a thin CLI.

pub mod capture;
pub mod ipc;
pub mod lock;
pub mod logging;
pub mod memory;
pub mod paths;
pub mod render;
pub mod restore;
pub mod run;
pub mod scheduler;
