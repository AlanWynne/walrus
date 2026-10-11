//! The `extract` subcommand: restore selected tracked files to disk under
//! their stored names at a chosen version (CLI-DESIGN 5.2.1).
//!
//! Extract is PER FILE and NOT transactional: a refused or unrepresentable file
//! does not abort the others. The whole-command return code is the most severe
//! per-file outcome: any safety refusal (RC 2) outranks a beyond-last clamp
//! (RC 4), which outranks success (RC 0). An empty match set is decided up
//! front as RC 8.

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use walrus_fmt::{Archive, FileId, Version, WalrusError};

use crate::args::Selector;
use crate::mask;
use crate::safety::{self, NameRefusal};

/// Parsed inputs for one `extract` invocation.
pub struct ExtractArgs {
    /// Archive path to open.
    pub archive: PathBuf,
    /// Optional file mask; `None` selects every tracked file.
    pub mask: Option<String>,
    /// Version selector resolved per file.
    pub selector: Selector,
    /// Output directory (stored relative paths are recreated beneath it).
    pub output: PathBuf,
    /// Overwrite an existing target instead of refusing it.
    pub force: bool,
    /// Deterministically rewrite unrepresentable names instead of refusing.
    pub sanitize: bool,
}

/// Run `extract`, returning the process return code (CLI-DESIGN section 6).
///
/// # Errors
/// Returns an error only for a hard failure opening or reconstructing the
/// archive; per-file safety and clamp outcomes are reported on stderr and
/// folded into the returned code, not raised as errors.
pub fn run(args: &ExtractArgs) -> Result<u8> {
    let archive = Archive::open(&args.archive)
        .with_context(|| format!("opening archive {}", args.archive.display()))?;

    let selected: Vec<String> = archive
        .files()
        .map(|f| f.name.clone())
        .filter(|name| match &args.mask {
            Some(m) => mask::matches(m, name),
            None => true,
        })
        .collect();

    // extract with a mask (or over an empty archive) that matches nothing -> RC 8.
    if selected.is_empty() {
        eprintln!("walx: no tracked file matches");
        return Ok(8);
    }

    let mut worst: u8 = 0;
    for name in &selected {
        let code = extract_one(&archive, name, args)?;
        worst = worst.max(code);
    }
    Ok(worst)
}

/// Extract one tracked file by name, returning its per-file return code.
fn extract_one(archive: &Archive, name: &str, args: &ExtractArgs) -> Result<u8> {
    // Resolve the stored name to a writable target, applying zip-slip and
    // reserved-name policy before any reconstruction.
    let target = match resolve_target(name, args) {
        Ok(path) => path,
        Err(code) => return Ok(code),
    };

    if target.exists() && !args.force {
        eprintln!("walx: {name}: target already exists (pass --force to overwrite)");
        return Ok(2);
    }

    let tracked = archive
        .file(name)
        .with_context(|| format!("tracked file '{name}' vanished from the manifest"))?;
    let file_id = FileId::from(tracked.id.as_str());

    let (bytes, code) = reconstruct_with_fallback(archive, &file_id, name, &args.selector)?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating directory {}", parent.display()))?;
    }
    let mut file =
        std::fs::File::create(&target).with_context(|| format!("creating {}", target.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("writing {}", target.display()))?;
    Ok(code)
}

/// Resolve the stored name to a writable path beneath the output directory,
/// enforcing the 5.4d naming policy. On refusal, report to stderr and return
/// the per-file return code (always RC 2).
fn resolve_target(name: &str, args: &ExtractArgs) -> std::result::Result<PathBuf, u8> {
    match safety::classify(name) {
        Ok(()) => Ok(safety::localized_join(&args.output, name)),
        Err(NameRefusal::ZipSlip) => {
            // Always refused; no flag relaxes it, including --sanitize.
            eprintln!("walx: {name}: refused (path escapes output directory)");
            Err(2)
        }
        Err(NameRefusal::Unrepresentable(rewrite)) => {
            if args.sanitize {
                eprintln!("walx: {name}: remapped to {rewrite}");
                Ok(safety::localized_join(&args.output, &rewrite))
            } else {
                eprintln!(
                    "walx: {name}: refused (name unrepresentable on this filesystem; \
                     pass --sanitize to rewrite)"
                );
                Err(2)
            }
        }
    }
}

/// Reconstruct `name` at the selector, applying the AC2 beyond-last fallback for
/// a numeric `--index`: a `VersionRange` error writes the ORIGINAL with a
/// warning and signals RC 4. Returns the bytes and this file's return code.
pub fn reconstruct_with_fallback(
    archive: &Archive,
    file_id: &FileId,
    name: &str,
    selector: &Selector,
) -> Result<(Vec<u8>, u8)> {
    match archive.reconstruct(file_id, selector.to_version()) {
        Ok(bytes) => Ok((bytes, 0)),
        Err(WalrusError::VersionRange { .. }) if selector.is_numeric_index() => {
            let original = archive
                .reconstruct(file_id, Version::Original)
                .with_context(|| format!("reconstructing original of '{name}'"))?;
            eprintln!("walx: {name}: index beyond last; wrote the original version");
            Ok((original, 4))
        }
        Err(e) => Err(anyhow::Error::new(e).context(format!("reconstructing '{name}'"))),
    }
}

/// Resolve the output directory for an extract (default: current directory).
pub fn output_dir(explicit: Option<&str>) -> PathBuf {
    match explicit {
        Some(dir) => PathBuf::from(dir),
        None => Path::new(".").to_path_buf(),
    }
}
