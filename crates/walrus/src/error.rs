//! Error type for the walrus-fmt crate.

use thiserror::Error;

/// Result alias for walrus-fmt operations.
pub type Result<T> = std::result::Result<T, WalrusError>;

/// Errors produced when reading, writing, or reconstructing a Walrus file.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum WalrusError {
    /// Underlying I/O failure.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),

    /// The outer compression codec could not be recognised from magic bytes.
    #[error("unrecognised container codec (expected zstd or gzip magic)")]
    UnknownCodec,

    /// A required tar member was missing.
    #[error("missing required container member: {0}")]
    MissingMember(&'static str),

    /// The manifest failed to parse or was structurally invalid.
    #[error("invalid manifest: {0}")]
    Manifest(String),

    /// A change-log line failed to parse.
    #[error("invalid change record on line {line}: {reason}")]
    Record {
        /// 1-based line number within `log.jsonl`.
        line: usize,
        /// Human-readable reason.
        reason: String,
    },

    /// A requested version could not be resolved or was out of range.
    #[error("version out of range: requested index {requested}, valid 0..={max}")]
    VersionRange {
        /// The index that was requested.
        requested: usize,
        /// The maximum valid index (the number of log records).
        max: usize,
    },

    /// A named checkpoint label was not found.
    #[error("unknown checkpoint label: {0}")]
    UnknownCheckpoint(String),

    /// A splice referenced a byte offset outside the current document.
    #[error("splice out of bounds at offset {at} (document length {len})")]
    SpliceOutOfBounds {
        /// The offset the splice tried to use.
        at: usize,
        /// The document length at the time.
        len: usize,
    },

    /// JSON (de)serialisation failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// An export backend failed to produce output.
    #[error("export failed ({backend}): {reason}")]
    Export {
        /// The backend that failed (e.g. "csv", "pdf", "docx").
        backend: &'static str,
        /// Human-readable reason.
        reason: String,
    },
}
