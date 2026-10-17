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

#[cfg(feature = "office")]
use crate::extract::reconstruct_group_with_fallback;
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

    let matching_groups: Vec<String> = archive
        .groups()
        .filter(|group| mask::matches(&args.file, &group.name))
        .map(|group| group.name.clone())
        .collect();

    #[cfg(not(feature = "office"))]
    if !matching_groups.is_empty() {
        anyhow::bail!("office support not built; rebuild this CLI with --features office");
    }

    #[cfg(feature = "office")]
    let grouped_file_ids: std::collections::HashSet<&str> = matching_groups
        .iter()
        .filter_map(|name| archive.group(name))
        .flat_map(|group| {
            std::iter::once(group.envelope_id.as_str())
                .chain(group.member_ids.iter().map(String::as_str))
        })
        .collect();
    let matched: Vec<String> = archive
        .files()
        .filter(|file| {
            #[cfg(feature = "office")]
            if grouped_file_ids.contains(file.id.as_str()) {
                return false;
            }
            mask::matches(&args.file, &file.name)
        })
        .map(|file| file.name.clone())
        .collect();

    // cat requires EXACTLY ONE match; zero or more than one is RC 2 (AC3).
    if matching_groups.len() + matched.len() != 1 {
        eprintln!(
            "walx: cat selects exactly one file; '{}' matched {}",
            args.file,
            matching_groups.len() + matched.len()
        );
        return Ok(2);
    }

    #[cfg(feature = "office")]
    if let Some(group) = matching_groups.first() {
        let (bytes, code) = reconstruct_group_with_fallback(&archive, group, &args.selector)?;
        std::io::stdout()
            .write_all(&bytes)
            .context("writing reconstructed office bytes to stdout")?;
        return Ok(code);
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
