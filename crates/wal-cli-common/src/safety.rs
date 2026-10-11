//! Extract naming safety: zip-slip refusal, reserved-name policy, `--sanitize`
//! rewriting, and localize-on-read path joining (CLI-DESIGN 5.4d).
//!
//! A stored name is platform-neutral (`/`-separated). The target filesystem may
//! be unable to represent it, or the name may try to escape the output
//! directory. This module encodes the fidelity-first, refuse-by-default policy:
//! path escaping is ALWAYS refused (no flag relaxes it); an unrepresentable
//! name is refused by default and deterministically rewritten under
//! `--sanitize`.

use std::path::{Path, PathBuf};

/// Why a stored name cannot be written to disk as-is.
#[derive(Debug, PartialEq, Eq)]
pub enum NameRefusal {
    /// The name would escape the output directory (absolute, drive-qualified,
    /// leading `/`, or a `..` component). ALWAYS refused; no flag relaxes it.
    ZipSlip,
    /// The local filesystem cannot represent the name (reserved device name,
    /// reserved character, or trailing dot/space). Refused by default; the
    /// carried string is the sanitized rewrite `--sanitize` would use.
    Unrepresentable(String),
}

/// Windows reserved device names (case-insensitive, with or without extension).
const RESERVED_DEVICE_NAMES: &[&str] = &[
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

/// Characters the local filesystem (Windows, the strict target) cannot
/// represent in a path segment.
const RESERVED_CHARS: &[char] = &['<', '>', ':', '"', '\\', '|', '?', '*'];

/// Classify a stored `/`-normalized `name` for extraction safety.
///
/// Returns `Ok(())` when the name is writable as-is, `Err(NameRefusal::ZipSlip)`
/// when it would escape the output directory (always refused), or
/// `Err(NameRefusal::Unrepresentable(rewrite))` when the local FS cannot
/// represent it (refused by default; `rewrite` is the `--sanitize` form).
pub fn classify(name: &str) -> Result<(), NameRefusal> {
    if is_zip_slip(name) {
        return Err(NameRefusal::ZipSlip);
    }
    let rewrite = sanitize(name);
    if rewrite != name {
        return Err(NameRefusal::Unrepresentable(rewrite));
    }
    Ok(())
}

/// True when `name` would resolve outside the output directory: an absolute or
/// drive-qualified path, a leading `/`, or any `..` segment. ALWAYS refused.
pub fn is_zip_slip(name: &str) -> bool {
    if name.starts_with('/') || name.starts_with('\\') {
        return true;
    }
    // Drive-qualified (`C:...`) or any Windows/absolute path shape.
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return true;
    }
    // A `..` as any `/`-split component escapes upward.
    name.split('/').any(|seg| seg == "..")
}

/// Deterministically rewrite a stored name so the local FS can represent it:
/// reserved characters -> `_`, a reserved device-name segment gets a `_`
/// suffix, and trailing dots/spaces are trimmed per segment. Operates per
/// `/`-segment; the `/` separators are preserved (localization happens at the
/// join step, not here).
pub fn sanitize(name: &str) -> String {
    let segments: Vec<String> = name.split('/').map(sanitize_segment).collect();
    segments.join("/")
}

/// Sanitize one path segment: map reserved chars to `_`, trim trailing dots and
/// spaces, then suffix `_` if the stem is a reserved device name.
fn sanitize_segment(segment: &str) -> String {
    let mapped: String = segment
        .chars()
        .map(|c| if RESERVED_CHARS.contains(&c) { '_' } else { c })
        .collect();
    let trimmed = mapped.trim_end_matches(['.', ' ']).to_string();
    if is_reserved_device(&trimmed) {
        format!("{trimmed}_")
    } else {
        trimmed
    }
}

/// True when a segment's stem (text before the first `.`) is a Windows reserved
/// device name, case-insensitively.
fn is_reserved_device(segment: &str) -> bool {
    let stem = segment.split('.').next().unwrap_or(segment);
    let lower = stem.to_ascii_lowercase();
    RESERVED_DEVICE_NAMES.contains(&lower.as_str())
}

/// Join a stored `/`-normalized `name` beneath `base`, splitting on `/` and
/// rebuilding with the local separator (localize-on-read, CLI-DESIGN 5.4d). The
/// caller has already classified the name as safe; this performs no escaping
/// check of its own beyond rejecting empty input.
pub fn localized_join(base: &Path, name: &str) -> PathBuf {
    let mut out = base.to_path_buf();
    for segment in name.split('/') {
        if segment.is_empty() {
            continue;
        }
        out.push(segment);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn absolute_or_drive_or_dotdot_name_is_refused_zip_slip() {
        assert!(matches!(classify("/etc/passwd"), Err(NameRefusal::ZipSlip)));
        assert!(matches!(
            classify("C:/Windows/system32"),
            Err(NameRefusal::ZipSlip)
        ));
        assert!(matches!(
            classify("../escape.txt"),
            Err(NameRefusal::ZipSlip)
        ));
        assert!(matches!(
            classify("src/../../escape.txt"),
            Err(NameRefusal::ZipSlip)
        ));
    }

    #[test]
    fn leading_slash_name_is_refused() {
        assert!(matches!(classify("/leading"), Err(NameRefusal::ZipSlip)));
        assert!(matches!(classify("\\leading"), Err(NameRefusal::ZipSlip)));
    }

    #[test]
    fn reserved_device_name_is_refused_by_default() {
        let refusal = classify("aux").expect_err("reserved device refused");
        match refusal {
            NameRefusal::Unrepresentable(rewrite) => assert_eq!(rewrite, "aux_"),
            other => panic!("expected unrepresentable, got {other:?}"),
        }
        // Reserved even with an extension.
        assert!(matches!(
            classify("nul.txt"),
            Err(NameRefusal::Unrepresentable(_))
        ));
    }

    #[test]
    fn sanitize_rewrites_reserved_chars_and_device_names_and_trims() {
        assert_eq!(sanitize("weird:name.txt"), "weird_name.txt");
        assert_eq!(sanitize("aux"), "aux_");
        assert_eq!(sanitize("trailing. "), "trailing");
        assert_eq!(sanitize("a<b>c"), "a_b_c");
        // Nested path segments are each sanitized; `/` is preserved here.
        assert_eq!(sanitize("src/com1"), "src/com1_");
    }

    #[test]
    fn sanitize_never_relaxes_zip_slip_refusal() {
        // A zip-slip name is refused BEFORE sanitize is consulted; even though
        // its segments contain nothing reserved, classify refuses it.
        assert!(matches!(
            classify("../../etc/passwd"),
            Err(NameRefusal::ZipSlip)
        ));
    }

    #[test]
    fn localize_on_read_splits_on_slash_and_rejoins_with_local_separator() {
        let base = Path::new("out");
        let joined = localized_join(base, "src/net/http.rs");
        let expected: PathBuf = ["out", "src", "net", "http.rs"].iter().collect();
        assert_eq!(joined, expected);
    }
}
