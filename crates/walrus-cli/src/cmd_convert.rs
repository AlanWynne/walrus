//! `convert`: flip replay direction between `.wal` and `.wbl` (CLI-DESIGN 5.7).
//!
//! No mask converts the WHOLE archive; a mask converts the matching file(s)
//! per-file. The library's `convert` matches by EXACT id-or-name, so a glob
//! mask is expanded via `names::matching` (empty -> RC 8 pre-check) and
//! `convert` is folded once per matched name. The result is written back to the
//! archive path, or to `-o <name>` when given. An extension-rename SUGGESTION is
//! printed to stderr; the on-disk file is never auto-renamed (AC4).

use std::path::PathBuf;

use anyhow::{Context, Result};
use walrus_fmt::{Archive, Direction};

use wal_cli_common::error::CliError;
use wal_cli_common::names;

use crate::common::{split_option, take_value};

/// Parsed `convert` inputs.
struct ConvertArgs {
    archive: PathBuf,
    mask: Option<String>,
    to: Direction,
    to_label: String,
    output: Option<PathBuf>,
}

/// Run `convert`, returning the process return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let args = parse(tokens)?;
    let archive = Archive::open(&args.archive)
        .with_context(|| format!("opening archive {}", args.archive.display()))?;

    let converted = match &args.mask {
        None => archive.convert(None, Some(args.to)).context("converting")?,
        Some(mask) => {
            let matched = names::matching(&archive, mask);
            if matched.is_empty() {
                return Err(CliError {
                    message: format!("no tracked file matches '{mask}'"),
                    code: 8,
                }
                .into());
            }
            let mut acc = archive;
            for name in &matched {
                acc = acc
                    .convert(Some(name), Some(args.to))
                    .with_context(|| format!("converting '{name}'"))?;
            }
            acc
        }
    };

    let dest = args.output.clone().unwrap_or_else(|| args.archive.clone());
    converted
        .write(&dest)
        .with_context(|| format!("writing archive {}", dest.display()))?;

    // Suggest an extension rename; never auto-rename on disk (AC4).
    if args.output.is_none() {
        eprintln!(
            "walrus: converted to {}; consider renaming to *.{}",
            args.to_label, args.to_label
        );
    }
    Ok(0)
}

/// Parse `convert` tokens: `<archive> [file-mask] --to <wal|wbl> [-o <name>]`.
fn parse(tokens: &[String]) -> Result<ConvertArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut to: Option<(Direction, String)> = None;
    let mut output: Option<String> = None;
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
            "--to" => {
                let value = take_value(name, inline, &mut iter)?;
                to = Some(parse_to(&value)?);
            }
            "-o" | "--output" => output = Some(take_value(name, inline, &mut iter)?),
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let (to, to_label) = to.ok_or_else(|| CliError::usage("convert requires --to <wal|wbl>"))?;

    let mut iter = positionals.into_iter();
    let archive = iter.next().ok_or_else(|| {
        CliError::usage("usage: walrus convert <archive> [file-mask] --to <wal|wbl>")
    })?;
    let mask = iter.next();
    if iter.next().is_some() {
        return Err(CliError::usage(
            "convert takes at most <archive> and one file-mask",
        ));
    }
    Ok(ConvertArgs {
        archive: PathBuf::from(archive),
        mask,
        to,
        to_label,
        output: output.map(PathBuf::from),
    })
}

/// Parse a `--to <wal|wbl>` value into a direction and its label.
fn parse_to(value: &str) -> Result<(Direction, String), CliError> {
    match value {
        "wal" => Ok((Direction::Forward, "wal".to_string())),
        "wbl" => Ok((Direction::Reverse, "wbl".to_string())),
        other => Err(CliError::usage(format!(
            "invalid --to '{other}' (expected wal or wbl)"
        ))),
    }
}
