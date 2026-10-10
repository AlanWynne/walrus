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

    /// A tracked file's `files/<id>/base` member was absent from the container.
    #[error("missing base member for tracked file '{id}'")]
    BaseMissing {
        /// The tracked-file id whose base member was missing.
        id: String,
    },

    /// A tracked file's `files/<id>/<name>.wchg` member was absent.
    #[error("missing .wchg member for tracked file '{id}'")]
    WchgMissing {
        /// The tracked-file id whose `.wchg` member was missing.
        id: String,
    },

    /// A record line in a specific file's `.wchg` failed to parse (or the line
    /// framing was malformed, e.g. a CRLF terminator or non-UTF-8 bytes).
    #[error("invalid .wchg record for file '{id}' on line {line}: {reason}")]
    Wchg {
        /// The owning tracked-file id.
        id: String,
        /// Header-adjusted line number (the first record line is line 2); a
        /// value of 0 denotes a stream-level fault (e.g. non-UTF-8 bytes).
        line: usize,
        /// Human-readable reason.
        reason: String,
    },

    /// The manifest failed to parse or was structurally invalid.
    #[error("invalid manifest: {0}")]
    Manifest(String),

    /// A `.wchg` header line was missing, not a `wchg` object, or malformed.
    #[error("invalid .wchg header: {0}")]
    WchgHeader(String),

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
