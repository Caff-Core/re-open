//! Adapter capability model.
//!
//! An adapter declares what it can do. A missing capability produces an
//! unsupported/skipped result, never guessed behaviour.

use std::fmt;

use serde::{Deserialize, Serialize};

/// One thing an adapter may be able to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    DiscoverApp,
    DiscoverProject,
    RestoreApp,
    RestoreProject,
    DetectExisting,
    RestoreGeometry,
}

impl Capability {
    pub const ALL: [Self; 6] = [
        Self::DiscoverApp,
        Self::DiscoverProject,
        Self::RestoreApp,
        Self::RestoreProject,
        Self::DetectExisting,
        Self::RestoreGeometry,
    ];

    const fn bit(self) -> u8 {
        1 << (self as u8)
    }

    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::DiscoverApp => "discover_app",
            Self::DiscoverProject => "discover_project",
            Self::RestoreApp => "restore_app",
            Self::RestoreProject => "restore_project",
            Self::DetectExisting => "detect_existing",
            Self::RestoreGeometry => "restore_geometry",
        }
    }
}

/// The capabilities an adapter supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash, Serialize, Deserialize)]
#[serde(from = "Vec<Capability>", into = "Vec<Capability>")]
pub struct CapabilitySet(u8);

impl CapabilitySet {
    #[must_use]
    pub const fn empty() -> Self {
        Self(0)
    }

    #[must_use]
    pub const fn with(self, capability: Capability) -> Self {
        Self(self.0 | capability.bit())
    }

    #[must_use]
    pub const fn contains(self, capability: Capability) -> bool {
        self.0 & capability.bit() != 0
    }

    pub fn iter(self) -> impl Iterator<Item = Capability> {
        Capability::ALL
            .into_iter()
            .filter(move |capability| self.contains(*capability))
    }

    /// # Errors
    ///
    /// Returns [`Unsupported`] when the capability is not in the set.
    pub const fn require(self, capability: Capability) -> Result<(), Unsupported> {
        if self.contains(capability) {
            Ok(())
        } else {
            Err(Unsupported { capability })
        }
    }
}

impl From<Vec<Capability>> for CapabilitySet {
    fn from(capabilities: Vec<Capability>) -> Self {
        capabilities.into_iter().fold(Self::empty(), Self::with)
    }
}

impl From<CapabilitySet> for Vec<Capability> {
    fn from(set: CapabilitySet) -> Self {
        set.iter().collect()
    }
}

/// An adapter lacks a capability; report it, do not guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unsupported {
    pub capability: Capability,
}

impl fmt::Display for Unsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "adapter does not support {}", self.capability.as_str())
    }
}

impl std::error::Error for Unsupported {}

/// Which adapter, at which payload version, produced a resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AdapterRef {
    /// Stable adapter identifier, for example `linux.desktop-entry`.
    pub id: String,
    pub version: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_contains_only_added_capabilities() {
        let set = CapabilitySet::empty()
            .with(Capability::DiscoverApp)
            .with(Capability::RestoreApp);
        assert!(set.contains(Capability::RestoreApp));
        assert!(!set.contains(Capability::RestoreProject));
        assert_eq!(
            set.require(Capability::DetectExisting),
            Err(Unsupported {
                capability: Capability::DetectExisting
            })
        );
        assert_eq!(set.require(Capability::DiscoverApp), Ok(()));
    }

    #[test]
    fn serde_is_a_sorted_name_list() {
        let set = CapabilitySet::empty()
            .with(Capability::RestoreApp)
            .with(Capability::DiscoverApp);
        let json = serde_json::to_string(&set).unwrap();
        assert_eq!(json, r#"["discover_app","restore_app"]"#);
        assert_eq!(serde_json::from_str::<CapabilitySet>(&json).unwrap(), set);
    }

    #[test]
    fn names_match_serde() {
        for capability in Capability::ALL {
            assert_eq!(
                serde_json::to_string(&capability).unwrap(),
                format!("\"{}\"", capability.as_str())
            );
        }
    }
}
