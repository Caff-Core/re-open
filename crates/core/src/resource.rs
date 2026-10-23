//! Captured resources and their versioned payloads.
//!
//! Payloads are typed and versioned per kind: the serde tag is
//! `<kind>.v<version>`, so a new payload version is a new variant and old
//! history stays readable. Payloads are immutable once committed and never
//! contain secrets or raw process command lines.

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::capability::AdapterRef;
use crate::id::{ResourceId, SourceId};

/// Maximum length of a filesystem path, in bytes.
pub const MAX_PATH_BYTES: usize = 4096;
/// Maximum length of a browser address, in bytes.
pub const MAX_URL_BYTES: usize = 8192;
/// Maximum length of an app identifier or display name, in bytes.
pub const MAX_NAME_BYTES: usize = 512;

/// The kind of thing a resource reopens.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceKind {
    Application,
    Project,
    Folder,
    BrowserTab,
}

/// A visible application, identified by its adapter (for example a desktop entry ID).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationV1 {
    pub app_id: String,
    pub display_name: String,
}

/// A project or document opened in an app, as reported by that app's adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectV1 {
    pub app_id: String,
    pub path: String,
}

/// A proven filesystem folder open in a file manager.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FolderV1 {
    pub path: String,
}

/// A browser tab. `window_key` groups tabs captured in the same window; it is
/// not a current browser API window ID.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserTabV1 {
    pub source_id: SourceId,
    pub window_key: u32,
    pub index: u32,
    pub url: String,
    pub pinned: bool,
    pub active: bool,
}

/// A versioned resource payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ResourcePayload {
    #[serde(rename = "application.v1")]
    ApplicationV1(ApplicationV1),
    #[serde(rename = "project.v1")]
    ProjectV1(ProjectV1),
    #[serde(rename = "folder.v1")]
    FolderV1(FolderV1),
    #[serde(rename = "browser_tab.v1")]
    BrowserTabV1(BrowserTabV1),
}

/// Why a payload was rejected before storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InvalidPayload {
    Empty { field: &'static str },
    TooLong { field: &'static str, max: usize },
    RelativePath { field: &'static str },
    ControlCharacter { field: &'static str },
}

impl fmt::Display for InvalidPayload {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Field names only; never the value.
        match self {
            Self::Empty { field } => write!(f, "{field} is empty"),
            Self::TooLong { field, max } => write!(f, "{field} exceeds {max} bytes"),
            Self::RelativePath { field } => write!(f, "{field} is not an absolute path"),
            Self::ControlCharacter { field } => write!(f, "{field} contains a control character"),
        }
    }
}

impl std::error::Error for InvalidPayload {}

fn text(field: &'static str, value: &str, max: usize) -> Result<(), InvalidPayload> {
    if value.is_empty() {
        return Err(InvalidPayload::Empty { field });
    }
    if value.len() > max {
        return Err(InvalidPayload::TooLong { field, max });
    }
    if value.chars().any(char::is_control) {
        return Err(InvalidPayload::ControlCharacter { field });
    }
    Ok(())
}

fn absolute_path(field: &'static str, value: &str) -> Result<(), InvalidPayload> {
    text(field, value, MAX_PATH_BYTES)?;
    if value.starts_with('/') {
        Ok(())
    } else {
        Err(InvalidPayload::RelativePath { field })
    }
}

impl ResourcePayload {
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        match self {
            Self::ApplicationV1(_) => ResourceKind::Application,
            Self::ProjectV1(_) => ResourceKind::Project,
            Self::FolderV1(_) => ResourceKind::Folder,
            Self::BrowserTabV1(_) => ResourceKind::BrowserTab,
        }
    }

    #[must_use]
    pub const fn version(&self) -> u16 {
        match self {
            Self::ApplicationV1(_)
            | Self::ProjectV1(_)
            | Self::FolderV1(_)
            | Self::BrowserTabV1(_) => 1,
        }
    }

    /// Identity used for deduplication and duplicate detection within a
    /// checkpoint. Browser addresses are compared exactly; no URL data is stripped.
    #[must_use]
    pub fn identity_key(&self) -> String {
        match self {
            Self::ApplicationV1(app) => format!("application:{}", app.app_id),
            Self::ProjectV1(project) => format!("project:{}:{}", project.app_id, project.path),
            Self::FolderV1(folder) => format!("folder:{}", folder.path),
            Self::BrowserTabV1(tab) => format!("browser_tab:{}:{}", tab.source_id, tab.url),
        }
    }

    /// Validates a payload before storage. Restore-time policy (allowed URL
    /// schemes, path existence, executable identity) is checked separately.
    ///
    /// # Errors
    ///
    /// Returns [`InvalidPayload`] naming the offending field.
    pub fn validate(&self) -> Result<(), InvalidPayload> {
        match self {
            Self::ApplicationV1(app) => {
                text("app_id", &app.app_id, MAX_NAME_BYTES)?;
                text("display_name", &app.display_name, MAX_NAME_BYTES)
            }
            Self::ProjectV1(project) => {
                text("app_id", &project.app_id, MAX_NAME_BYTES)?;
                absolute_path("path", &project.path)
            }
            Self::FolderV1(folder) => absolute_path("path", &folder.path),
            Self::BrowserTabV1(tab) => text("url", &tab.url, MAX_URL_BYTES),
        }
    }
}

/// One resource within a checkpoint.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resource {
    pub id: ResourceId,
    pub adapter: AdapterRef,
    /// Stable display and restore order within the checkpoint.
    pub ordinal: u32,
    pub payload: ResourcePayload,
}

impl Resource {
    #[must_use]
    pub const fn kind(&self) -> ResourceKind {
        self.payload.kind()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(url: &str) -> ResourcePayload {
        ResourcePayload::BrowserTabV1(BrowserTabV1 {
            source_id: SourceId::from_uuid(uuid::Uuid::nil()),
            window_key: 1,
            index: 0,
            url: url.to_owned(),
            pinned: false,
            active: true,
        })
    }

    #[test]
    fn payload_tag_encodes_kind_and_version() {
        let payload = ResourcePayload::FolderV1(FolderV1 {
            path: "/home/u/src".to_owned(),
        });
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "type": "folder.v1", "path": "/home/u/src" })
        );
        assert_eq!(
            serde_json::from_value::<ResourcePayload>(json).unwrap(),
            payload
        );
        assert_eq!(payload.kind(), ResourceKind::Folder);
        assert_eq!(payload.version(), 1);
    }

    #[test]
    fn unknown_type_and_fields_are_rejected() {
        let unknown_type = serde_json::json!({ "type": "folder.v9", "path": "/x" });
        assert!(serde_json::from_value::<ResourcePayload>(unknown_type).is_err());
        let extra_field = serde_json::json!({ "type": "folder.v1", "path": "/x", "cmdline": "rm" });
        assert!(serde_json::from_value::<ResourcePayload>(extra_field).is_err());
    }

    #[test]
    fn browser_identity_keeps_query_and_fragment() {
        let a = tab("https://example.com/a?x=1#top");
        let b = tab("https://example.com/a?x=1");
        assert_ne!(a.identity_key(), b.identity_key());
    }

    #[test]
    fn validation_rejects_bad_fields_without_echoing_values() {
        let relative = ResourcePayload::FolderV1(FolderV1 {
            path: "src/secret".to_owned(),
        });
        let error = relative.validate().unwrap_err();
        assert_eq!(error, InvalidPayload::RelativePath { field: "path" });
        assert!(!error.to_string().contains("secret"));

        assert_eq!(
            tab("").validate(),
            Err(InvalidPayload::Empty { field: "url" })
        );
        let long = tab(&"a".repeat(MAX_URL_BYTES + 1));
        assert_eq!(
            long.validate(),
            Err(InvalidPayload::TooLong {
                field: "url",
                max: MAX_URL_BYTES
            })
        );
        let control = ResourcePayload::FolderV1(FolderV1 {
            path: "/a\nb".to_owned(),
        });
        assert_eq!(
            control.validate(),
            Err(InvalidPayload::ControlCharacter { field: "path" })
        );
        assert_eq!(tab("https://example.com/").validate(), Ok(()));
    }
}
