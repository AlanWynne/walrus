//! walrus-fmt -- the `.wal` / `.wbl` document-with-history format.
//!
//! A Walrus file is a compressed tar bundle of a base document plus an ordered
//! change log. See `FORMAT.md` at the repository root for the authoritative,
//! language-agnostic specification.
//!
//! ```text
//! .wal :  original (base)  + forward_log  = current
//! .wbl :  current  (base)  - reverse_log  = original
//! ```
//!
//! This crate is GUI-free and dependency-light so it can be embedded. Export
//! formats are behind opt-in cargo features (`export-*`).

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod archive;
mod atomic;
mod container;
#[cfg(feature = "diff")]
mod diff;
#[cfg(feature = "i18n")]
mod encoding;
mod error;
mod invert;
mod manifest;
mod mutate;
mod mutate_add;
mod mutate_convert;
#[cfg(feature = "i18n")]
mod mutate_i18n;
mod mutate_remove;
mod mutate_stamp;
mod rebase;
mod reconstruct;
mod record;
mod search;
mod standalone;
mod timestamp;
mod version;
mod wchg;
mod write;

pub mod export;

pub use archive::{Archive, FileId};
#[cfg(feature = "diff")]
pub use diff::{diff, DiffResult, Granularity};
pub use error::{Result, WalrusError};
pub use manifest::{
    Checkpoint, FileAttributes, Manifest, RebaseInfo, TrackedFile, Variant, Versions,
};
pub use record::{ChangeRecord, Op};
pub use search::{search_version, search_versions, Match, MatchMode, SearchOptions};
pub use version::Version;
pub use wchg::{Direction, WchgHeader};
