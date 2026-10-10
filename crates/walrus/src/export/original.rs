//! Verbatim "Original" export: write the stored bytes back unchanged.
//!
//! This is always compiled and has no dependency: it simply returns the
//! reconstructed document bytes. The caller names the output file from
//! `manifest().document.name` so a stored `report.html` is written back as
//! `report.html`, byte for byte.

/// Return the reconstructed document bytes verbatim.
///
/// Owns the "Original export is byte-identical to the stored document"
/// invariant (design section 7.4): only the library knows the stored bytes, so
/// no re-encoding happens here.
pub fn to_original(doc: &[u8]) -> Vec<u8> {
    doc.to_vec()
}
