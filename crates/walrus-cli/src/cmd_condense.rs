//! `condense`: re-base selected files, discarding older history (CLI-DESIGN
//! 5.6).
//!
//! The library matches by EXACT id-or-name and returns `Rebase` (not
//! `NotTracked`) for a no-match, so the CLI pre-checks `names::matching`
//! emptiness to raise RC 8, then drives `Archive::condense` once per matched
//! name with `keep` (default 0). Transactional: written once at the end.

use std::path::PathBuf;

use anyhow::{Context, Result};
use walrus_fmt::Archive;

use wal_cli_common::error::CliError;
use wal_cli_common::names;

use crate::common::{split_option, take_value};

/// Parsed `condense` inputs.
struct CondenseArgs {
    archive: PathBuf,
    mask: String,
    keep: usize,
}

/// Run `condense`, returning the process return code.
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

    for name in &matched {
        archive
            .condense(name, args.keep)
            .with_context(|| format!("condensing '{name}'"))?;
    }
    archive
        .write(&args.archive)
        .with_context(|| format!("writing archive {}", args.archive.display()))?;
    Ok(0)
}

/// Parse `condense` tokens: `<archive> <file-mask> [--keep <n>]`.
fn parse(tokens: &[String]) -> Result<CondenseArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut keep = 0usize;
    let mut options_done = false;
    let mut iter = tokens.iter().peekable();

    while let Some(token) = iter.next() {
        if options_done || !token.starts_with('-') || token == "-" {
            positionals.push(token.clone());
            continue;
        }
        if token == "--" {
            options_done = true;
            continue;
        }
        let (name, inline) = split_option(token);
        match name {
            "--keep" => {
                let value = take_value(name, inline, &mut iter)?;
                keep = value
                    .parse::<usize>()
                    .map_err(|_| CliError::usage(format!("invalid --keep value '{value}'")))?;
            }
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let mut iter = positionals.into_iter();
    let archive = iter.next().ok_or_else(|| {
        CliError::usage("usage: walrus condense <archive> <file-mask> [--keep n]")
    })?;
    let mask = iter.next().ok_or_else(|| {
        CliError::usage("usage: walrus condense <archive> <file-mask> [--keep n]")
    })?;
    if iter.next().is_some() {
        return Err(CliError::usage(
            "condense takes <archive> and one file-mask",
        ));
    }
    Ok(CondenseArgs {
        archive: PathBuf::from(archive),
        mask,
        keep,
    })
}
