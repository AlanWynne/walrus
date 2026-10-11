//! The `cat` subcommand: reconstruct EXACTLY ONE tracked file and write its raw
//! bytes to stdout (CLI-DESIGN 5.2.2).
//!
//! `cat` never touches disk and takes no `-o`/`--force`. Selecting zero or more
//! than one tracked file is a usage error (RC 2); the beyond-last numeric index
//! fallback (original + warning + RC 4) matches extract.

use std::io::Write;
use std::path::PathBuf;

use anyhow::{Context, Result};
use walrus_fmt::{Archive, FileId};

use crate::extract::reconstruct_with_fallback;
use crate::mask;
use crate::selector::Selector;

/// Parsed inputs for one `cat` invocation.
pub struct CatArgs {
    /// Archive path to open.
    pub archive: PathBuf,
    /// The single file selector (an exact name/id or a mask).
    pub file: String,
    /// Version selector resolved against the single matched file.
    pub selector: Selector,
}

/// Run `cat`, returning the process return code (CLI-DESIGN section 6).
///
/// # Errors
/// Returns an error only for a hard failure opening or reconstructing the
/// archive; the exactly-one-match rule and the beyond-last clamp are reported
/// via the return code and stderr.
pub fn run(args: &CatArgs) -> Result<u8> {
    let archive = Archive::open(&args.archive)
        .with_context(|| format!("opening archive {}", args.archive.display()))?;

    let matched: Vec<String> = archive
        .files()
        .map(|f| f.name.clone())
        .filter(|name| mask::matches(&args.file, name))
        .collect();

    // cat requires EXACTLY ONE match; zero or more than one is RC 2 (AC3).
    if matched.len() != 1 {
        eprintln!(
            "walx: cat selects exactly one file; '{}' matched {}",
            args.file,
            matched.len()
        );
        return Ok(2);
    }

    let name = &matched[0];
    let tracked = archive
        .file(name)
        .with_context(|| format!("tracked file '{name}' vanished from the manifest"))?;
    let file_id = FileId::from(tracked.id.as_str());

    let (bytes, code) = reconstruct_with_fallback(&archive, &file_id, name, &args.selector)?;

    std::io::stdout()
        .write_all(&bytes)
        .context("writing reconstructed bytes to stdout")?;
    Ok(code)
}
