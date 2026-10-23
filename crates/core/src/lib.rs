//! Domain types and policies for Re-Open.
//!
//! This crate has no Tauri, storage or OS-specific dependencies. It defines
//! identifiers, time, checkpoints, resources and adapter capabilities that the
//! agent, storage and platform crates share. See `docs/DATA_MODEL.md`.

pub mod capability;
pub mod checkpoint;
pub mod id;
pub mod resource;
pub mod source;
pub mod time;

pub use capability::{AdapterRef, Capability, CapabilitySet, Unsupported};
pub use checkpoint::{CaptureReason, Completeness, SourceStatus};
pub use id::{CheckpointId, ResourceId, SessionId, SourceId, WorkspaceId};
pub use resource::{Resource, ResourceKind, ResourcePayload};
pub use source::{SourceState, UnknownSourceState};
pub use time::{Clock, Deadline, MonotonicInstant, SystemClock, Timestamp};
