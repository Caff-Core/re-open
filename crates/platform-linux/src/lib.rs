//! Linux platform adapters for Re-Open (first target: Fedora, GNOME on Wayland).
//!
//! Application identity comes from freedesktop desktop entries. Window
//! discovery follows ADR-016. Nothing here launches a shell or guesses an
//! executable by name.

#![cfg(target_os = "linux")]

pub mod catalog;
pub mod desktop;

pub use catalog::{AppIdentity, AppKind, Catalog, ExclusionPolicy, ExecError, data_dirs};
