//! `add`: create a tracked file or record a new version (CLI-DESIGN 5.3).
//!
//! Sources are file arguments, directories walked under `-R`, or a single `-`
//! stdin stream named by `--name`. A directory argument without `-R` is RC 2
//! (AC7a). The create-time direction follows the seed order (explicit flag ->
//! brand-new-archive extension -> default forward). A binary input aborts the
//! whole batch transactionally (AC7d/AC9d): the archive is persisted once at the
//! end via `Archive::write`, so a mid-batch failure leaves it unchanged. Each
//! recorded change is stamped with the save-instant UTC timestamp (AC8).

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walrus_fmt::{Archive, Direction, Manifest, Variant};

use wal_cli_common::error::CliError;
use wal_cli_common::input_glob;

use crate::clock;
use crate::common::{split_option, take_value};
use crate::recurse;
use crate::stdin_src;

/// One source to add: its stored name, bytes, and source-file mtime UTC stamp
/// (or the wall-clock fallback for stdin).
struct Source {
    name: String,
    bytes: Vec<u8>,
    ts: String,
}

/// Parsed `add` inputs.
struct AddArgs {
    archive: PathBuf,
    files: Vec<String>,
    dash_count: usize,
    name: Option<String>,
    generations: Option<u32>,
    direction: Option<Direction>,
    recurse: bool,
}

/// Run `add`, returning the process return code.
pub fn run(tokens: &[String]) -> Result<u8> {
    let args = parse(tokens)?;
    let sources = collect_sources(&args)?;

    let mut archive = open_or_create(&args)?;

    for source in &sources {
        // A file ALREADY tracked whose content is byte-identical records no new
        // version (CLI-DESIGN 5.4): detect by comparing its count across the
        // save. A brand-new file (not previously tracked) is a genuine create.
        let was_tracked = archive.file(&source.name).is_some();
        let before = count_of(&archive, &source.name);
        archive
            .add_at(
                &source.name,
                &source.bytes,
                args.generations,
                args.direction,
                &source.ts,
            )
            .with_context(|| format!("adding '{}'", source.name))?;
        // A fresh create (count 0) records no splice, so attributes.mtime is the
        // only LastWriteTime a single-version file can surface. Reuse the SAME
        // source.ts (no second mtime read); set it ONLY on first create.
        if !was_tracked {
            archive
                .set_attributes_mtime(&source.name, &source.ts)
                .with_context(|| format!("stamping mtime for '{}'", source.name))?;
        }
        if was_tracked && count_of(&archive, &source.name) == before {
            eprintln!("walrus: {}: no change, not versioned", source.name);
        }
    }

    archive
        .write(&args.archive)
        .with_context(|| format!("writing archive {}", args.archive.display()))?;
    Ok(0)
}

/// The recorded-version count of a tracked file, or 0 when absent.
fn count_of(archive: &Archive, name: &str) -> usize {
    archive.file(name).map(|f| f.versions.count).unwrap_or(0)
}

/// Collect the ordered sources for this add from files / `-R` / stdin.
fn collect_sources(args: &AddArgs) -> Result<Vec<Source>> {
    // stdin path (AC9): validated and buffered; mutually exclusive with files.
    if let Some(stdin) = stdin_src::resolve(
        args.dash_count,
        args.name.as_deref(),
        args.files.len(),
        args.recurse,
    )? {
        return Ok(vec![Source {
            name: stdin.name,
            bytes: stdin.bytes,
            ts: clock::now_utc(),
        }]);
    }

    // Expand any wildcard file-input operand to concrete paths BEFORE any byte
    // is read (CLI-DESIGN 5.4f). literal-path-wins, else portable glob (a
    // no-match is RC 2), else unchanged passthrough. `-R` still governs how a
    // resolved directory is handled below, unchanged.
    let resolved = input_glob::resolve_file_operands(&args.files)
        .map_err(|e| CliError::usage(e.to_string()))?;

    let mut out = Vec::new();
    for path in &resolved {
        if path.is_dir() {
            if !args.recurse {
                return Err(CliError::usage(format!(
                    "'{}' is a directory; pass -R to add its contents",
                    path.display()
                ))
                .into());
            }
            for c in recurse::collect_dir(path)? {
                out.push(Source {
                    name: c.name,
                    bytes: c.bytes,
                    ts: c.ts,
                });
            }
        } else {
            let bytes =
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
            out.push(Source {
                name: stored_name(&path.to_string_lossy()),
                bytes,
                ts: clock::file_mtime_utc(path),
            });
        }
    }
    Ok(out)
}

/// The stored name for a PLAIN file argument (not under `-R`): the file's own
/// name (basename), never the directory path it was given by. A bare file
/// argument has no tracked-tree context, so storing the full path -- especially
/// an absolute or drive-qualified one -- would both leak the caller's directory
/// layout into the archive and produce an invalid `files/<id>/` member. The
/// recursion path (`-R`, `recurse::collect_dir`) handles relative-tree naming
/// separately (CLI-DESIGN 5.4b/5.4c). The result always uses `/` and carries no
/// separator for a plain file (it is a single component).
fn stored_name(arg: &str) -> String {
    Path::new(arg)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| arg.to_string())
}

/// Open the archive, or create an empty one of the seed-order variant when the
/// path does not exist (CLI-DESIGN 5.3 brand-new seed order).
fn open_or_create(args: &AddArgs) -> Result<Archive> {
    if args.archive.exists() {
        return Archive::open(&args.archive)
            .with_context(|| format!("opening archive {}", args.archive.display()));
    }
    let variant = seed_variant(&args.archive, args.direction);
    Ok(Archive::new(Manifest::new(variant)))
}

/// The seed variant for a brand-new archive: explicit direction flag first,
/// else the archive extension (`.wal` -> forward, `.wbl` -> reverse), else the
/// default forward / wal.
fn seed_variant(archive: &Path, direction: Option<Direction>) -> Variant {
    if let Some(d) = direction {
        return match d {
            Direction::Forward => Variant::Wal,
            Direction::Reverse => Variant::Wbl,
        };
    }
    match archive.extension().and_then(|e| e.to_str()) {
        Some("wbl") => Variant::Wbl,
        _ => Variant::Wal,
    }
}

/// Parse `add` tokens.
fn parse(tokens: &[String]) -> Result<AddArgs, CliError> {
    let mut positionals: Vec<String> = Vec::new();
    let mut dash_count = 0usize;
    let mut name: Option<String> = None;
    let mut generations: Option<u32> = None;
    let mut direction: Option<Direction> = None;
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
                generations = Some(parse_generations(&value)?);
            }
            "--wal" => set_direction(&mut direction, Direction::Forward)?,
            "--wbl" => set_direction(&mut direction, Direction::Reverse)?,
            "--direction" => {
                let value = take_value(opt, inline, &mut iter)?;
                set_direction(&mut direction, parse_direction(&value)?)?;
            }
            "-R" | "--recurse" => recurse = true,
            other => return Err(CliError::usage(format!("unknown option '{other}'"))),
        }
    }

    let mut iter = positionals.into_iter();
    let archive = iter
        .next()
        .ok_or_else(|| CliError::usage("usage: walrus add <archive> <files...>"))?;
    let files: Vec<String> = iter.collect();
    if files.is_empty() && dash_count == 0 {
        return Err(CliError::usage(
            "add needs at least one file or '-' (stdin)",
        ));
    }
    Ok(AddArgs {
        archive: PathBuf::from(archive),
        files,
        dash_count,
        name,
        generations,
        direction,
        recurse,
    })
}

/// Set the create-time direction, raising RC 2 if two conflicting flags appear.
fn set_direction(slot: &mut Option<Direction>, value: Direction) -> Result<(), CliError> {
    if slot.is_some_and(|d| d != value) {
        return Err(CliError::usage(
            "conflicting direction flags (--wal/--wbl/--direction)",
        ));
    }
    *slot = Some(value);
    Ok(())
}

/// Parse a `--direction <forward|reverse>` value.
fn parse_direction(value: &str) -> Result<Direction, CliError> {
    match value {
        "forward" => Ok(Direction::Forward),
        "reverse" => Ok(Direction::Reverse),
        other => Err(CliError::usage(format!(
            "invalid --direction '{other}' (expected forward or reverse)"
        ))),
    }
}

/// Parse a `--generations <n>` value (a non-negative integer).
fn parse_generations(value: &str) -> Result<u32, CliError> {
    value
        .parse::<u32>()
        .map_err(|_| CliError::usage(format!("invalid --generations value '{value}'")))
}
