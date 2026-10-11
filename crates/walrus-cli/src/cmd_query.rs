//! `query` (the default operation): list tracked files and their index size
//! (CLI-DESIGN 5.1, FR-W11 AC1).
//!
//! Terse (default): one `name<TAB>index_size` per line to stdout. Long (`-l` /
//! `--long`): adds the recorded attributes (size / mtime / mode), the inner
//! change-log byte size (`names::log_bytes`), and the generation cap. An
//! optional mask filters the listing via the segment-aware glob
//! (`names::matching`). Query is read-only.

use std::path::PathBuf;

use anyhow::{Context, Result};
use walrus_fmt::Archive;

use wal_cli_common::error::CliError;
use wal_cli_common::names;

use crate::common::split_option;

/// Parsed `query` inputs.
struct QueryArgs {
    archive: PathBuf,
    mask: Option<String>,
    long: bool,
}

/// Run `query`, returning the process return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let args = parse(tokens)?;
    let archive = Archive::open(&args.archive)
        .with_context(|| format!("opening archive {}", args.archive.display()))?;

    let names: Vec<String> = match &args.mask {
        Some(mask) => names::matching(&archive, mask),
        None => archive.files().map(|f| f.name.clone()).collect(),
    };

    let mut out = String::new();
    for name in &names {
        if args.long {
            out.push_str(&long_line(&archive, name));
        } else {
            out.push_str(&terse_line(&archive, name));
        }
        out.push('\n');
    }
    print!("{out}");
    Ok(0)
}

/// `name<TAB>index_size` for the terse listing.
fn terse_line(archive: &Archive, name: &str) -> String {
    let count = index_size(archive, name);
    format!("{name}\t{count}")
}

/// The long form: name, index size, log bytes, generation cap, and attributes.
fn long_line(archive: &Archive, name: &str) -> String {
    let tracked = archive.file(name);
    let count = index_size(archive, name);
    let log_bytes = names::log_bytes(archive, name);
    let generations = tracked.map(|f| f.generations).unwrap_or(0);
    let size = tracked
        .and_then(|f| f.attributes.size)
        .map(|s| s.to_string())
        .unwrap_or_else(|| "-".to_string());
    let mtime = tracked
        .and_then(|f| f.attributes.mtime.clone())
        .unwrap_or_else(|| "-".to_string());
    let mode = tracked
        .and_then(|f| f.attributes.mode.clone())
        .unwrap_or_else(|| "-".to_string());
    // Columns: name index_size log_bytes generations size mode mtime.
    format!("{name}\t{count}\t{log_bytes}\t{generations}\t{size}\t{mode}\t{mtime}")
}

/// The index size (count of recorded versions) for a tracked file.
fn index_size(archive: &Archive, name: &str) -> usize {
    archive.file(name).map(|f| f.versions.count).unwrap_or(0)
}

/// Parse `query` tokens: `<archive> [file-mask] [-l|--long]`.
fn parse(tokens: &[String]) -> Result<QueryArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut long = false;
    let mut options_done = false;

    for token in tokens {
        if options_done || !token.starts_with('-') || token == "-" {
            positionals.push(token.clone());
            continue;
        }
        if token == "--" {
            options_done = true;
            continue;
        }
        let (name, _inline) = split_option(token);
        match name {
            "-l" | "--long" => long = true,
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let mut positionals = positionals.into_iter();
    let archive = positionals
        .next()
        .ok_or_else(|| CliError::usage("usage: walrus query <archive> [file-mask] [-l]"))?;
    let mask = positionals.next();
    if positionals.next().is_some() {
        return Err(CliError::usage(
            "query takes at most <archive> and one file-mask",
        ));
    }
    Ok(QueryArgs {
        archive: PathBuf::from(archive),
        mask,
        long,
    })
}
