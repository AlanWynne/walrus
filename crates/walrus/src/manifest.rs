//! The `manifest.json` container member.

use serde::{Deserialize, Serialize};

/// Which variant of the format a file is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Variant {
    /// `.wal`: base is the original, log runs forward to current.
    Wal,
    /// `.wbl`: base is the current, log runs in reverse to the original.
    Wbl,
}

/// Declared properties of the stored document.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DocumentInfo {
    /// Text encoding of the base bytes (e.g. `utf-8`).
    pub encoding: String,
    /// MIME type (e.g. `text/plain`, `text/csv`, `text/markdown`).
    pub mime: String,
    /// Suggested file name for an extracted document.
    pub name: String,
}

impl Default for DocumentInfo {
    fn default() -> Self {
        Self {
            encoding: "utf-8".to_string(),
            mime: "text/plain".to_string(),
            name: "document.txt".to_string(),
        }
    }
}

/// Version-count bookkeeping.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Versions {
    /// Number of change records in the log.
    pub count: usize,
    /// The index at which the stored base sits (always 0 in 0.1).
    pub base_index: usize,
    /// The far-end index (equals `count` in 0.1).
    pub head_index: usize,
}

/// A named, human-addressable version index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// The version index this checkpoint names (in `0..=count`).
    pub index: usize,
    /// A human label, e.g. "before refactor".
    pub label: String,
}

/// The parsed `manifest.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// Always `"walrus"`.
    pub format: String,
    /// Spec version string, e.g. `"0.1"`.
    pub spec_version: String,
    /// `wal` or `wbl`.
    pub variant: Variant,
    /// `"original"` for `.wal`, `"current"` for `.wbl`.
    pub base_role: String,
    /// `"forward"` for `.wal`, `"reverse"` for `.wbl`.
    pub log_direction: String,
    /// Document properties.
    pub document: DocumentInfo,
    /// Version counts.
    pub versions: Versions,
    /// Optional sorted checkpoints.
    #[serde(default)]
    pub checkpoints: Vec<Checkpoint>,
    /// ISO-8601 creation timestamp (optional).
    #[serde(default)]
    pub created: Option<String>,
    /// Generator identifier (optional).
    #[serde(default)]
    pub generator: Option<String>,
}

impl Manifest {
    /// Current spec version this crate reads and writes.
    pub const SPEC_VERSION: &'static str = "0.1";

    /// Build a minimal `.wal` manifest for a base of the given log length.
    pub fn new_wal(log_count: usize) -> Self {
        Self {
            format: "walrus".to_string(),
            spec_version: Self::SPEC_VERSION.to_string(),
            variant: Variant::Wal,
            base_role: "original".to_string(),
            log_direction: "forward".to_string(),
            document: DocumentInfo::default(),
            versions: Versions {
                count: log_count,
                base_index: 0,
                head_index: log_count,
            },
            checkpoints: Vec::new(),
            created: None,
            generator: None,
        }
    }

    /// Validate structural invariants (format tag, spec compatibility, ranges).
    pub fn validate(&self) -> crate::Result<()> {
        if self.format != "walrus" {
            return Err(crate::WalrusError::Manifest(format!(
                "unexpected format tag '{}'",
                self.format
            )));
        }
        if self.spec_version != Self::SPEC_VERSION {
            return Err(crate::WalrusError::Manifest(format!(
                "incompatible spec_version '{}' (this build reads {})",
                self.spec_version,
                Self::SPEC_VERSION
            )));
        }
        for cp in &self.checkpoints {
            if cp.index > self.versions.count {
                return Err(crate::WalrusError::Manifest(format!(
                    "checkpoint '{}' index {} exceeds count {}",
                    cp.label, cp.index, self.versions.count
                )));
            }
        }
        Ok(())
    }
}
