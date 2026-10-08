//! Change-log records (`log.jsonl`).

use serde::{Deserialize, Serialize};

/// The operation a change record performs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Op {
    /// A splice: at `at`, delete `delete_len` bytes and insert `insert`.
    Splice {
        /// Byte offset into the version the record applies to.
        at: usize,
        /// Number of bytes removed at `at`.
        delete_len: usize,
        /// UTF-8 bytes inserted at `at` after deletion.
        insert: String,
    },
    /// An integrity checkpoint carrying a snapshot hash (not a mutation).
    Checkpoint {
        /// The version index this checkpoint corresponds to.
        index: usize,
        /// Hex-encoded SHA-256 of the reconstructed document at `index`.
        sha256: String,
    },
}

/// One line of the change log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangeRecord {
    /// The operation.
    #[serde(flatten)]
    pub op: Op,
    /// Optional ISO-8601 timestamp for this step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ts: Option<String>,
    /// Optional human note for this single step.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl ChangeRecord {
    /// Build a splice record with no timestamp or note.
    pub fn splice(at: usize, delete_len: usize, insert: impl Into<String>) -> Self {
        Self {
            op: Op::Splice {
                at,
                delete_len,
                insert: insert.into(),
            },
            ts: None,
            note: None,
        }
    }

    /// True when this record is a mutating splice (vs a checkpoint marker).
    pub fn is_splice(&self) -> bool {
        matches!(self.op, Op::Splice { .. })
    }
}
