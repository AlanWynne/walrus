//! The shared `search` engine: find a string (literal or regex) in selected
//! tracked files and render a report (CLI-DESIGN 5.8).
//!
//! Both `walx` and `walrus` reuse this engine, so the subcommand behaves
//! identically on both. Search is READ-ONLY: it reconstructs the selected
//! version or complete history, runs the pure `walrus_fmt` matcher over the
//! bytes, and renders the results.

use std::path::PathBuf;

use anyhow::{Context, Result};
use walrus_fmt::{Archive, Direction, FileId, MatchMode, SearchOptions, Version};

use crate::extract::reconstruct_with_fallback;
use crate::names;
use crate::search_render::{FileGroup, OutputFormat, SearchReport, SearchScope};
use crate::selector::Selector;

/// Parsed inputs for one `search` invocation.
pub struct SearchArgs {
    /// Archive path to open.
    pub archive: PathBuf,
    /// The string (literal or, with `regex`, a pattern) to search for.
    pub needle: String,
    /// Optional file mask; `None` searches every tracked file.
    pub mask: Option<String>,
    /// Version selector resolved per file (default: current / index 0).
    pub selector: Selector,
    /// Search every version, instead of the selected version.
    pub all_versions: bool,
    /// Fold case when matching (`-i` / `--ignore-case`).
    pub case_insensitive: bool,
    /// Treat `needle` as a regular expression (`--regex`).
    pub regex: bool,
    /// Output format (Markdown by default; Plain for piping).
    pub format: OutputFormat,
}

/// Run `search`, returning the process return code (CLI-DESIGN section 6):
/// 0 matches found, 1 no matches, 4 a numeric index was clamped beyond the last
/// version (the clamp warning dominates the found/none result).
///
/// stdout carries the report; diagnostics and the clamp warning go to stderr.
///
/// # Errors
/// Returns an error only for a hard failure opening or reconstructing the
/// archive, or an invalid regex pattern; the no-match and clamp outcomes are
/// folded into the returned code, not raised as errors.
pub fn run(args: &SearchArgs) -> Result<u8> {
    let archive = Archive::open(&args.archive)
        .with_context(|| format!("opening archive {}", args.archive.display()))?;

    let selected: Vec<String> = match &args.mask {
        Some(mask) => names::matching(&archive, mask),
        None => archive.files().map(|f| f.name.clone()).collect(),
    };

    // SearchOptions is #[non_exhaustive]; build it from Default then set fields.
    let mut opts = SearchOptions::default();
    opts.mode = match_mode(args.regex);
    opts.case_insensitive = args.case_insensitive;

    #[cfg(feature = "search-regex")]
    if args.regex {
        walrus_fmt::search_version("", 0, None, &[], &args.needle, &opts)
            .context("validating search pattern")?;
    }

    let mut groups = Vec::with_capacity(selected.len());
    let mut worst: u8 = 0;
    // Every selected file shares the selector, so the first file supplies the
    // single-version header metadata.
    let mut header_index = 0usize;
    let mut header_ts: Option<String> = None;
    let mut header_set = false;

    for name in &selected {
        if args.all_versions {
            groups.push(search_all_versions(&archive, name, args, &opts)?);
        } else {
            let (group, index, ts, code) = search_one(&archive, name, args, &opts)?;
            worst = worst.max(code);
            if !header_set {
                header_index = index;
                header_ts = ts;
                header_set = true;
            }
            groups.push(group);
        }
    }

    let report = SearchReport {
        archive_display: args.archive.display().to_string(),
        scope: if args.all_versions {
            SearchScope::AllVersions
        } else {
            SearchScope::Version {
                index: header_index,
                timestamp: header_ts,
            }
        },
        groups,
    };

    print!("{}", report.render(args.format));

    // RC: a beyond-last clamp (4) dominates; otherwise 0 if any match, else 1.
    if worst == 4 {
        return Ok(4);
    }
    if report.match_count() > 0 {
        Ok(0)
    } else {
        Ok(1)
    }
}

/// Search all versions of one tracked file, from current index 0 to original.
fn search_all_versions(
    archive: &Archive,
    name: &str,
    args: &SearchArgs,
    opts: &SearchOptions,
) -> Result<FileGroup> {
    let tracked = archive
        .file(name)
        .with_context(|| format!("tracked file '{name}' vanished from the manifest"))?;
    let file_id = FileId::from(tracked.id.as_str());
    let count = splice_count(archive, name);

    let matches = walrus_fmt::search_versions(
        name,
        (0..=count).map(|index| {
            let bytes = archive.reconstruct(&file_id, Version::Index(index))?;
            Ok((index, version_ts(archive, name, index, count), bytes))
        }),
        &args.needle,
        opts,
    )
    .with_context(|| format!("searching all versions of '{name}'"))?;

    Ok(FileGroup {
        file_name: name.to_string(),
        matches,
    })
}

/// Search ONE tracked file at the resolved version, returning its match group,
/// the resolved index, that version's timestamp, and the per-file return code
/// (0 normally, 4 when a numeric index was clamped beyond the last version).
fn search_one(
    archive: &Archive,
    name: &str,
    args: &SearchArgs,
    opts: &SearchOptions,
) -> Result<(FileGroup, usize, Option<String>, u8)> {
    let tracked = archive
        .file(name)
        .with_context(|| format!("tracked file '{name}' vanished from the manifest"))?;
    let file_id = FileId::from(tracked.id.as_str());

    let (bytes, code) = reconstruct_with_fallback(archive, &file_id, name, &args.selector)?;

    // The concrete index actually reconstructed: the clamp fallback writes the
    // original, so a clamped search reports (and stamps) the original index.
    let count = splice_count(archive, name);
    let index = if code == 4 {
        count
    } else {
        resolve_index(archive, name, &args.selector, count)
    };
    let ts = version_ts(archive, name, index, count);

    let matches =
        walrus_fmt::search_version(name, index, ts.as_deref(), &bytes, &args.needle, opts)
            .with_context(|| format!("searching '{name}'"))?;

    Ok((
        FileGroup {
            file_name: name.to_string(),
            matches,
        },
        index,
        ts,
        code,
    ))
}

/// Map the `--regex` flag to a [`MatchMode`]. The binaries enable the
/// `walrus-fmt/search-regex` feature, so the regex arm is always available when
/// this crate is built as a dependency of a CLI binary; without the feature the
/// enum variant is unnameable and `regex` falls through to a literal search.
fn match_mode(regex: bool) -> MatchMode {
    #[cfg(feature = "search-regex")]
    {
        if regex {
            return MatchMode::Regex;
        }
    }
    #[cfg(not(feature = "search-regex"))]
    {
        let _ = regex;
    }
    MatchMode::Literal
}

/// The number of SPLICE records (a file's far/original index) for `name`.
fn splice_count(archive: &Archive, name: &str) -> usize {
    archive
        .records(name)
        .map(|recs| recs.iter().filter(|r| r.is_splice()).count())
        .unwrap_or(0)
}

/// Resolve a [`Selector`] to the concrete index it addresses in this file's
/// `[0, count]` space, mirroring the library's per-file addressing (the
/// library resolver is `pub(crate)`, so the CLI reproduces the same rule here,
/// the same reason the UTC parser is duplicated in `selector.rs`). The
/// beyond-last numeric clamp is handled separately by the caller via the
/// reconstruct fallback, so a numeric index past `count` is clamped to `count`
/// here too for a consistent header.
fn resolve_index(archive: &Archive, name: &str, selector: &Selector, count: usize) -> usize {
    match selector {
        Selector::Current => 0,
        Selector::Original => count,
        Selector::Index(n) => (*n).min(count),
        Selector::Relative(n) => clamp_relative(*n, count),
        Selector::At(epoch) => resolve_timestamp(archive, name, count, *epoch),
    }
}

/// Clamp a signed offset-from-current into `[0, count]` (mirrors the library).
fn clamp_relative(n: i64, count: usize) -> usize {
    if n <= 0 {
        0
    } else if (n as u128) >= count as u128 {
        count
    } else {
        n as usize
    }
}

/// Resolve an `--at` epoch to a concrete index, mirroring the library's
/// timestamp rule: the index whose record timestamp is the newest at-or-before
/// `epoch`; a timestamp before the earliest record resolves to index 0.
fn resolve_timestamp(archive: &Archive, name: &str, count: usize, epoch: i64) -> usize {
    let Some(records) = archive.records(name) else {
        return 0;
    };
    let mut best_ordinal: Option<usize> = None;
    let mut ordinal = 0usize;
    for record in records {
        if !record.is_splice() {
            continue;
        }
        ordinal += 1;
        if let Some(e) = record.ts.as_deref().and_then(parse_epoch) {
            if e <= epoch {
                best_ordinal = Some(ordinal);
            }
        }
    }
    match best_ordinal {
        Some(o) => count.saturating_sub(o).min(count),
        None => 0,
    }
}

/// The change timestamp of the version at `index`, derived direction-aware from
/// the file's records (mirrors `walrus-cli`'s `current_record_ts`, generalised
/// to any index). The ORIGINAL (index == count) has no producing record and
/// returns `None`.
///
/// Record order along the log is oldest-applied-first. For a FORWARD file the
/// splice at natural ordinal `count - index` (1-based among splices) produces
/// the version at `index` (index 0 = current = the last splice). For a REVERSE
/// file records are stored current-first, so the splice at ordinal `index + 1`
/// produces the version at `index` (index 0 = current = the first splice).
fn version_ts(archive: &Archive, name: &str, index: usize, count: usize) -> Option<String> {
    if index >= count {
        return None; // the original base has no producing change record
    }
    let direction = archive.header(name)?.direction;
    let records = archive.records(name)?;
    let target_ordinal = match direction {
        Direction::Forward => count - index,
        Direction::Reverse => index + 1,
    };
    records
        .iter()
        .filter(|r| r.is_splice())
        .nth(target_ordinal.checked_sub(1)?)
        .and_then(|r| r.ts.clone())
}

/// Parse the fixed-shape UTC `YYYY-MM-DDThh:mm:ssZ` stamp a record carries into
/// epoch seconds, reusing the selector module's validated parser. A malformed
/// stamp yields `None` so a bad record never breaks timestamp addressing.
fn parse_epoch(stamp: &str) -> Option<i64> {
    crate::selector::parse_at_utc(stamp).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use walrus_fmt::{Archive, ChangeRecord, Manifest, Variant};

    /// Build the canonical multi-file fixture on disk and return its path and
    /// the TempDir keeping it alive. `notes.txt`: original "cat" -> "cats" ->
    /// current "the cats"; `src/main.rs`: single-version "fn main() {}".
    fn fixture() -> (TempDir, PathBuf) {
        let dir = TempDir::new().expect("temp dir");
        let archive = dir.path().join("fixture.wal");
        let mut a = Archive::new(Manifest::new(Variant::Wal));
        a.add("notes.txt", b"cat", None, None).expect("add notes");
        a.update_with_splices("notes.txt", &[ChangeRecord::splice(3, 0, "s")])
            .expect("notes save 1");
        a.update_with_splices("notes.txt", &[ChangeRecord::splice(0, 0, "the ")])
            .expect("notes save 2");
        a.add("src/main.rs", b"fn main() {}", None, None)
            .expect("add nested");
        a.write(&archive).expect("write fixture");
        (dir, archive)
    }

    fn base_args(archive: PathBuf, needle: &str) -> SearchArgs {
        SearchArgs {
            archive,
            needle: needle.to_string(),
            mask: None,
            selector: Selector::Current,
            all_versions: false,
            case_insensitive: false,
            regex: false,
            format: OutputFormat::Markdown,
        }
    }

    #[test]
    fn search_finds_literal_match_and_returns_rc_zero() {
        let (_dir, archive) = fixture();
        let code = run(&base_args(archive, "cats")).expect("search runs");
        assert_eq!(code, 0, "a found match exits 0");
    }

    #[test]
    fn search_with_no_match_returns_rc_one() {
        let (_dir, archive) = fixture();
        let code = run(&base_args(archive, "penguin")).expect("search runs");
        assert_eq!(code, 1, "a valid search with zero hits exits 1");
    }

    #[test]
    fn search_mask_restricts_to_matching_files() {
        let (_dir, archive) = fixture();
        let mut args = base_args(archive, "main");
        args.mask = Some("src/main.rs".to_string());
        let code = run(&args).expect("search runs");
        assert_eq!(code, 0, "'main' is only in src/main.rs");
    }

    #[test]
    fn search_mask_matching_no_file_is_a_zero_hit_search_rc_one() {
        let (_dir, archive) = fixture();
        let mut args = base_args(archive, "anything");
        args.mask = Some("missing.txt".to_string());
        let code = run(&args).expect("search runs");
        assert_eq!(
            code, 1,
            "a mask matching no file is still a zero-hit search"
        );
    }

    #[test]
    fn resolve_index_maps_the_selector_to_a_concrete_index() {
        let (_dir, archive) = fixture();
        let a = Archive::open(&archive).expect("open");
        let count = splice_count(&a, "notes.txt");
        assert_eq!(count, 2, "notes.txt has two splices");
        assert_eq!(resolve_index(&a, "notes.txt", &Selector::Current, count), 0);
        assert_eq!(
            resolve_index(&a, "notes.txt", &Selector::Original, count),
            2
        );
        assert_eq!(
            resolve_index(&a, "notes.txt", &Selector::Index(1), count),
            1
        );
        // A numeric index past the last version clamps to the original index.
        assert_eq!(
            resolve_index(&a, "notes.txt", &Selector::Index(9), count),
            2
        );
    }

    #[test]
    fn version_ts_of_the_original_index_is_none() {
        let (_dir, archive) = fixture();
        let a = Archive::open(&archive).expect("open");
        let count = splice_count(&a, "notes.txt");
        // The original (index == count) has no producing change record.
        assert_eq!(version_ts(&a, "notes.txt", count, count), None);
    }
}
