//! `remove`: delete tracked file(s) matched by a mask (CLI-DESIGN 5.5).
//!
//! The library matches by EXACT id-or-name, so the CLI expands the segment-aware
//! glob to a set of exact names (`names::matching`) and drives `Archive::remove`
//! once per matched name. Zero matches -> RC 8; more than one without `--force`
//! -> RC 2 listing the matches (the AmbiguousRemove sense). Transactional: the
//! archive is written once after every removal succeeds.

use std::path::PathBuf;

use anyhow::{Context, Result};
use walrus_fmt::Archive;

use wal_cli_common::error::CliError;
use wal_cli_common::names;

use crate::common::split_option;

/// Parsed `remove` inputs.
struct RemoveArgs {
    archive: PathBuf,
    mask: String,
    force: bool,
}

/// Run `remove`, returning the process return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let args = parse(tokens)?;
    let mut archive = Archive::open(&args.archive)
        .with_context(|| format!("opening archive {}", args.archive.display()))?;

    let matched = names::matching(&archive, &args.mask);
    if matched.is_empty() {
        return Err(CliError {
            message: format!("no tracked file matches '{}'", args.mask),
            code: 8,
        }
        .into());
    }
    if matched.len() > 1 && !args.force {
        return Err(CliError::usage(format!(
            "mask '{}' matches {} files; pass --force to remove all: {}",
            args.mask,
            matched.len(),
            matched.join(", ")
        ))
        .into());
    }

    for name in &matched {
        archive
            .remove(name, args.force)
            .with_context(|| format!("removing '{name}'"))?;
    }
    archive
        .write(&args.archive)
        .with_context(|| format!("writing archive {}", args.archive.display()))?;
    Ok(0)
}

/// Parse `remove` tokens: `<archive> <file-mask> [--force]`.
fn parse(tokens: &[String]) -> Result<RemoveArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut force = false;
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
            "--force" => force = true,
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let mut iter = positionals.into_iter();
    let archive = iter
        .next()
        .ok_or_else(|| CliError::usage("usage: walrus remove <archive> <file-mask> [--force]"))?;
    let mask = iter
        .next()
        .ok_or_else(|| CliError::usage("usage: walrus remove <archive> <file-mask> [--force]"))?;
    if iter.next().is_some() {
        return Err(CliError::usage("remove takes <archive> and one file-mask"));
    }
    Ok(RemoveArgs {
        archive: PathBuf::from(archive),
        mask,
        force,
    })
}
