//! `query` (the default operation): list tracked files (CLI-DESIGN 5.1,
//! FR-W11 AC1).
//!
//! Terse (default): ls-style, one bare relative name per line to stdout, sorted.
//! Long (`-l` / `--long`): an aligned table with a header row and an ASCII `-`
//! separator row, columns Versions | LastWriteTime | Bytes | Name. An optional
//! mask filters the listing via the segment-aware glob (`names::matching`).
//! Query is read-only.

use std::path::PathBuf;

use anyhow::{Context, Result};
use walrus_fmt::{Archive, Direction, FileId, Version};

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

    let mut names: Vec<String> = match &args.mask {
        Some(mask) => names::matching(&archive, mask),
        None => archive.files().map(|f| f.name.clone()).collect(),
    };
    // Byte-wise lexicographic sort of the FINAL vector so terse and long emit
    // the same deterministic order for both the mask and no-mask branches.
    names.sort();

    let mut out = String::new();
    if args.long {
        out.push_str(&long_header());
        out.push('\n');
    }
    for name in &names {
        if args.long {
            out.push_str(&long_line(&archive, name));
        } else {
            out.push_str(&terse_line(name));
        }
        out.push('\n');
    }
    print!("{out}");
    Ok(0)
}

/// ls-style terse listing: just the name, one per line.
fn terse_line(name: &str) -> String {
    name.to_string()
}

/// The long table header plus a dash-run separator row.
fn long_header() -> String {
    format!(
        "{:>8} {:>19} {:>12} {}\n{} {} {} {}",
        "Versions",
        "LastWriteTime",
        "Bytes",
        "Name",
        "-".repeat(8),
        "-".repeat(19),
        "-".repeat(12),
        "-".repeat(4),
    )
}

/// One aligned long-table row: Versions | LastWriteTime | Bytes | Name.
fn long_line(archive: &Archive, name: &str) -> String {
    let versions = index_size(archive, name);
    let last = last_write_time(archive, name);
    let bytes = current_bytes(archive, name);
    format!("{versions:>8} {last:>19} {bytes:>12} {name}")
}

/// The recorded-version count for a tracked file.
fn index_size(archive: &Archive, name: &str) -> usize {
    archive.file(name).map(|f| f.versions.count).unwrap_or(0)
}

/// The byte length of the CURRENT reconstructed version (index 0), rendered as
/// a decimal string. Degrades to `-` on a reconstruct error (best-effort per
/// column: a listing is read-only and never aborts on one bad cell).
fn current_bytes(archive: &Archive, name: &str) -> String {
    let Some(tracked) = archive.file(name) else {
        return "-".to_string();
    };
    let id = FileId::from(tracked.id.as_str());
    match archive.reconstruct(&id, Version::Index(0)) {
        Ok(bytes) => bytes.len().to_string(),
        Err(_) => "-".to_string(),
    }
}

/// The `ts` of the CURRENT version's splice record, selected by direction.
///
/// Record order is OLDEST-APPLIED-FIRST along the log, and index 0 is always
/// the current version. A FORWARD (`.wal`) file APPENDS its net splice, so the
/// current version's record is the LAST splice; a REVERSE (`.wbl`) file inserts
/// at index 0, so the current version's record is the FIRST splice. The
/// `is_splice` filter also skips a leading/trailing checkpoint record.
fn current_record_ts(archive: &Archive, name: &str) -> Option<String> {
    let direction = archive.header(name)?.direction;
    let records = archive.records(name)?;
    let rec = match direction {
        Direction::Forward => records.iter().rev().find(|r| r.is_splice()),
        Direction::Reverse => records.iter().find(|r| r.is_splice()),
    }?;
    rec.ts.clone()
}

/// The LastWriteTime for the display: the current version's record `ts`, else
/// the create-time `attributes.mtime`, else `-`. Both candidate stamps route
/// through [`render_last_write`], which is total and never panics.
fn last_write_time(archive: &Archive, name: &str) -> String {
    let stamp = current_record_ts(archive, name)
        .or_else(|| archive.file(name).and_then(|f| f.attributes.mtime.clone()));
    match stamp.as_deref() {
        Some(s) => render_last_write(s),
        None => "-".to_string(),
    }
}

/// Render a stored UTC stamp to the display shape `YYYY-MM-DD HH:MM:SS`.
/// TOTAL: returns `"-"` for any value that is not the fixed 20-char
/// `YYYY-MM-DDThh:mm:ssZ` shape, so a malformed stamp never panics the listing.
fn render_last_write(stamp: &str) -> String {
    let b = stamp.as_bytes();
    // Explicit char-boundary checks on every slice edge so a 20-BYTE string
    // carrying a multi-byte char at an interior position (which could still
    // satisfy the ASCII-byte checks below) can NEVER slice mid-char.
    if b.len() == 20
        && b[10] == b'T'
        && b[19] == b'Z'
        && stamp.is_char_boundary(10)
        && stamp.is_char_boundary(11)
        && stamp.is_char_boundary(19)
    {
        format!("{} {}", &stamp[0..10], &stamp[11..19])
    } else {
        "-".to_string()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use walrus_fmt::{Archive, Manifest, Variant};

    #[test]
    fn render_last_write_renders_a_conforming_stamp() {
        assert_eq!(
            render_last_write("2026-10-11T16:18:44Z"),
            "2026-10-11 16:18:44"
        );
    }

    #[test]
    fn render_last_write_returns_dash_for_a_short_string() {
        assert_eq!(render_last_write("2026-10-11"), "-");
        assert_eq!(render_last_write(""), "-");
    }

    #[test]
    fn render_last_write_is_total_on_a_20_byte_multibyte_string() {
        // A 20-BYTE string with a multi-byte char at an interior position must
        // return "-" WITHOUT panicking (the is_char_boundary guard).
        let s = "2026-\u{00E9}0-11T16:18:4Z"; // 20 bytes, interior 2-byte char
        assert_eq!(s.len(), 20, "fixture must be exactly 20 bytes");
        assert_eq!(render_last_write(s), "-");
    }

    /// Build a two-version forward (.wal) file whose splices carry known stamps.
    fn forward_two_version() -> Archive {
        let mut a = Archive::new(Manifest::new(Variant::Wal));
        a.add("doc.txt", b"hello", None, None).expect("create");
        a.add_at(
            "doc.txt",
            b"hello world",
            None,
            None,
            "2020-01-01T00:00:00Z",
        )
        .expect("save 1 (oldest)");
        a.add_at(
            "doc.txt",
            b"hello world!",
            None,
            None,
            "2026-10-11T16:18:44Z",
        )
        .expect("save 2 (current)");
        a
    }

    #[test]
    fn current_record_ts_forward_returns_the_last_splice_stamp() {
        let a = forward_two_version();
        assert_eq!(
            current_record_ts(&a, "doc.txt").as_deref(),
            Some("2026-10-11T16:18:44Z"),
            "forward current version is the LAST splice record"
        );
    }

    #[test]
    fn current_record_ts_reverse_returns_the_first_splice_stamp() {
        let mut a = Archive::new(Manifest::new(Variant::Wbl));
        a.add("doc.txt", b"hello", None, Some(Direction::Reverse))
            .expect("create reverse");
        a.add_at(
            "doc.txt",
            b"hello world",
            None,
            None,
            "2020-01-01T00:00:00Z",
        )
        .expect("save 1 (oldest)");
        a.add_at(
            "doc.txt",
            b"hello world!",
            None,
            None,
            "2026-10-11T16:18:44Z",
        )
        .expect("save 2 (current)");
        // A reverse file inserts at index 0, so the current version's record is
        // the FIRST splice -- the later stamp.
        assert_eq!(
            current_record_ts(&a, "doc.txt").as_deref(),
            Some("2026-10-11T16:18:44Z"),
            "reverse current version is the FIRST splice record"
        );
    }
}
