//! walrus -- the full management CLI over the walrus-fmt multi-file Archive API.
//!
//! Subcommands (canonical) and their flag aliases:
//!   query / -q, extract / -x, cat, add / -a, update / -u, remove / -r,
//!   condense, convert.
//! Operations are MUTUALLY EXCLUSIVE: naming two in one invocation is RC 2.
//!
//! The parser is hand-rolled to keep the dependency surface auditable (INV-2):
//! `walrus-cli` links `walrus-fmt` with only the `diff` feature plus the shared
//! `wal-cli-common` crate. The selector grammar, naming safety, mask globbing,
//! and the extract/cat engines live in `wal-cli-common`; this binary owns
//! dispatch, the mutate/query command modules, recursion, and stdin ingestion.

use std::process::ExitCode;

use walrus_fmt::WalrusError;

use wal_cli_common::error::CliError;

mod alias_shim;
mod clock;
mod cmd_add;
mod cmd_alias;
mod cmd_cat;
mod cmd_condense;
mod cmd_convert;
mod cmd_extract;
mod cmd_query;
mod cmd_remove;
mod cmd_search;
mod cmd_update;
mod common;
mod dispatch;
mod recurse;
mod stdin_src;

fn main() -> ExitCode {
    match dispatch::run() {
        Ok(code) => ExitCode::from(code),
        Err(e) => ExitCode::from(map_error(&e)),
    }
}

/// Map a dispatch error to its process return code (CLI-DESIGN section 6),
/// printing a `walrus:`-prefixed diagnostic to stderr.
///
/// A typed [`CliError`] carries its own code; a [`WalrusError`] maps per the
/// typed-error table (NotTracked -> 8, AmbiguousRemove -> 2, everything else a
/// general failure RC 1); anything else is RC 1.
fn map_error(e: &anyhow::Error) -> u8 {
    if let Some(cli) = e.downcast_ref::<CliError>() {
        eprintln!("walrus: {}", cli.message);
        return cli.code;
    }
    if let Some(w) = e.downcast_ref::<WalrusError>() {
        let code = rc_for_walrus_error(w);
        eprintln!("walrus: {e:#}");
        return code;
    }
    eprintln!("walrus: {e:#}");
    1
}

/// The return code for a library [`WalrusError`] (CLI-DESIGN section 6 table).
fn rc_for_walrus_error(e: &WalrusError) -> u8 {
    match e {
        WalrusError::NotTracked { .. } => 8,
        WalrusError::AmbiguousRemove { .. } => 2,
        // BinaryInput, SpliceNotCharBoundary, InvalidTimestamp, and every other
        // variant are general failures (non-zero RC 1); the batch is aborted
        // transactionally by not writing (CLI-DESIGN 5.4a).
        _ => 1,
    }
}
