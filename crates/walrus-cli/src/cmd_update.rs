//! `update`: record a new version of an ALREADY-tracked file (CLI-DESIGN 5.4).
//!
//! Like `add` for sources (file args, `-R` directories, `-` stdin), but a file
//! absent from the archive is `NotTracked` -> RC 8 (AC3/AC7e/AC9e). Uses the
//! diff path (`Archive::update_at`) and stamps each recorded change with the
//! save-instant UTC timestamp (AC8). Transactional: the archive is persisted
//! once at the end, so a mid-batch RC-8 leaves it byte-for-byte unchanged.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use walrus_fmt::Archive;

use wal_cli_common::error::CliError;

use crate::clock;
use crate::common::{split_option, take_value};
use crate::recurse;
use crate::stdin_src;

/// One source to update: its stored name and bytes.
struct Source {
    name: String,
    bytes: Vec<u8>,
}

/// Parsed `update` inputs.
struct UpdateArgs {
    archive: PathBuf,
    files: Vec<String>,
    dash_count: usize,
    name: Option<String>,
    generations: Option<u32>,
    recurse: bool,
}

/// Run `update`, returning the process return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let args = parse(tokens)?;
    let sources = collect_sources(&args)?;

    let mut archive = Archive::open(&args.archive)
        .with_context(|| format!("opening archive {}", args.archive.display()))?;
    let ts = clock::now_utc();

    for source in &sources {
        let before = archive
            .file(&source.name)
            .map(|f| f.versions.count)
            .unwrap_or(0);
        // update_at surfaces NotTracked for an absent file -> mapped to RC 8 by
        // main. The archive is not written on error, so the batch is atomic.
        archive
            .update_at(&source.name, &source.bytes, args.generations, &ts)
            .with_context(|| format!("updating '{}'", source.name))?;
        if archive
            .file(&source.name)
            .map(|f| f.versions.count)
            .unwrap_or(0)
            == before
        {
            eprintln!("walrus: {}: no change, not versioned", source.name);
        }
    }

    archive
        .write(&args.archive)
        .with_context(|| format!("writing archive {}", args.archive.display()))?;
    Ok(0)
}

/// Collect the ordered sources for this update from files / `-R` / stdin.
fn collect_sources(args: &UpdateArgs) -> Result<Vec<Source>> {
    if let Some(stdin) = stdin_src::resolve(
        args.dash_count,
        args.name.as_deref(),
        args.files.len(),
        args.recurse,
    )? {
        return Ok(vec![Source {
            name: stdin.name,
            bytes: stdin.bytes,
        }]);
    }

    let mut out = Vec::new();
    for arg in &args.files {
        let path = Path::new(arg);
        if path.is_dir() {
            if !args.recurse {
                return Err(CliError::usage(format!(
                    "'{arg}' is a directory; pass -R to update its contents"
                ))
                .into());
            }
            for c in recurse::collect_dir(path)? {
                out.push(Source {
                    name: c.name,
                    bytes: c.bytes,
                });
            }
        } else {
            let bytes =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            out.push(Source {
                name: stored_name(arg),
                bytes,
            });
        }
    }
    Ok(out)
}

/// The stored name for a PLAIN file argument (not under `-R`): the file's own
/// name (basename), matching how `add` names a plain file argument so an
/// `update <archive> <path>` resolves to the file `add <archive> <path>`
/// created. A bare file argument carries no tracked-tree context, so the full
/// (possibly absolute) path is never used as the name; the `-R` recursion path
/// handles relative-tree naming separately (CLI-DESIGN 5.4b/5.4c).
fn stored_name(arg: &str) -> String {
    std::path::Path::new(arg)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| arg.to_string())
}

/// Parse `update` tokens.
fn parse(tokens: &[String]) -> Result<UpdateArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut dash_count = 0usize;
    let mut name: Option<String> = None;
    let mut generations: Option<u32> = None;
    let mut recurse = false;
    let mut options_done = false;
    let mut iter = tokens.iter().peekable();

    while let Some(token) = iter.next() {
        if token == "-" {
            dash_count += 1;
            continue;
        }
        if options_done || !token.starts_with('-') {
            positionals.push(token.clone());
            continue;
        }
        if token == "--" {
            options_done = true;
            continue;
        }
        let (opt, inline) = split_option(token);
        match opt {
            "--name" => name = Some(take_value(opt, inline, &mut iter)?),
            "--generations" => {
                let value = take_value(opt, inline, &mut iter)?;
                generations =
                    Some(value.parse::<u32>().map_err(|_| {
                        CliError::usage(format!("invalid --generations '{value}'"))
                    })?);
            }
            "-R" | "--recurse" => recurse = true,
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let mut iter = positionals.into_iter();
    let archive = iter
        .next()
        .ok_or_else(|| CliError::usage("usage: walrus update <archive> <files...>"))?;
    let files: Vec<String> = iter.collect();
    if files.is_empty() && dash_count == 0 {
        return Err(CliError::usage(
            "update needs at least one file or '-' (stdin)",
        ));
    }
    Ok(UpdateArgs {
        archive: PathBuf::from(archive),
        files,
        dash_count,
        name,
        generations,
        recurse,
    })
}
