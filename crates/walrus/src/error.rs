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

    /// A `reconstruct` call named a file id with no backing tracked file.
    #[error("unknown file id: {id}")]
    UnknownFile {
        /// The file id that did not match any tracked file.
        id: String,
    },

    /// A splice referenced a byte offset outside the current document.
    #[error("splice out of bounds at offset {at} (document length {len})")]
    SpliceOutOfBounds {
        /// The offset the splice tried to use.
        at: usize,
        /// The document length at the time.
        len: usize,
    },

    /// A re-base / trim request could not be satisfied: either an impossible
    /// request (e.g. retaining zero versions, when the current version always
    /// exists) or, at the archive level, a `condense` mask matching no tracked
    /// file. The benign `keep >= count` case is a no-op success, not this error.
    #[error("re-base failed: {reason}")]
    Rebase {
        /// Human-readable reason the re-base could not proceed.
        reason: String,
    },

    /// JSON (de)serialisation failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),

    /// The diff engine was given binary input (FR-W6). The adapter takes UTF-8
    /// `&str`, so this triggers when the input is not valid UTF-8 or when
    /// otherwise-valid UTF-8 contains a NUL byte.
    #[error("binary input rejected: {reason}")]
    BinaryInput {
        /// Human-readable reason the input was treated as binary.
        reason: String,
    },

    /// A mutate op that requires an existing tracked file (`update`,
    /// `update_with_splices`, `set_generations`) -- or a `remove` whose mask
    /// matched nothing -- named a file that is not tracked. The RC-8
    /// "must add first" / target-not-found condition.
    #[error("file '{name}' is not tracked; add it first")]
    NotTracked {
        /// The selector (name or id) that matched no tracked file.
        name: String,
    },

    /// A caller supplied a change-record timestamp that is not the fixed UTC
    /// shape `YYYY-MM-DDThh:mm:ssZ` the writer and the `--at` resolver require
    /// (FORMAT.md 4.1 / section 5). The stamping seam rejects it rather than
    /// letting a malformed `ts` enter the log, where it would silently fail
    /// timestamp addressing.
    #[error("invalid record timestamp '{value}' (expected UTC YYYY-MM-DDThh:mm:ssZ)")]
    InvalidTimestamp {
        /// The offending timestamp string.
        value: String,
    },

    /// A `remove` mask matched more than one tracked file without `force`.
    #[error("mask '{mask}' matches {count} files; pass force to remove all: {matches}")]
    AmbiguousRemove {
        /// The mask that matched multiple files.
        mask: String,
        /// The number of files the mask matched.
        count: usize,
        /// A human-readable list of the matched files.
        matches: String,
    },

    /// A caller-supplied splice in `update_with_splices` has an in-range offset
    /// that falls in the MIDDLE of a multi-byte UTF-8 character. Distinct from
    /// [`WalrusError::SpliceOutOfBounds`], which is an offset past the end of
    /// the version.
    #[error("splice at {at} does not land on a UTF-8 character boundary")]
    SpliceNotCharBoundary {
        /// The offset that split a multi-byte character.
        at: usize,
    },

    /// A source encoding declared for an `add_with_encoding` call could not be
    /// resolved to a known codec, or the input failed to decode under it
    /// (FR-W8).
    #[cfg(feature = "i18n")]
    #[error("unsupported or undecodable encoding '{label}': {reason}")]
    Encoding {
        /// The declared encoding label (e.g. "windows-1252").
        label: String,
        /// Human-readable reason the encoding could not be used.
        reason: String,
    },

    /// An export backend failed to produce output.
    #[error("export failed ({backend}): {reason}")]
    Export {
        /// The backend that failed (e.g. "csv", "pdf", "docx").
        backend: &'static str,
        /// Human-readable reason.
        reason: String,
    },
}
