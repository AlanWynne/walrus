//! FR-W8 encoding entry points (feature `i18n`): declare a file's source
//! encoding at CREATE time and extract it back in a chosen encoding.
//!
//! These layer on the ungated mutate/reconstruct paths rather than widening
//! them (deviation 2): [`Archive::add_with_encoding`] transcodes the declared
//! source bytes to the single internal UTF-8 storage via
//! [`crate::encoding::decode_to_utf8`] and funnels into the SAME storage path as
//! [`Archive::add`], recording the resolved encoding label on the created file;
//! [`Archive::reconstruct_as_encoding`] reconstructs the UTF-8 bytes through the
//! un-gated [`Archive::reconstruct`] (the single apply path, INV-4) and encodes
//! them back via [`crate::encoding::encode_from_utf8`].
//!
//! Encoding is FIXED at create time and IMMUTABLE: a later save never changes a
//! file's recorded encoding, so every stored index remains valid UTF-8 relative
//! to the one declared source encoding.

use crate::archive::Archive;
use crate::encoding::{decode_to_utf8, encode_from_utf8};
use crate::wchg::Direction;
use crate::{FileId, Result, Version, WalrusError};

impl Archive {
    /// Add `name` from `bytes` declared to be in the source `encoding`, storing
    /// the content as UTF-8 and recording the resolved encoding label on the
    /// newly created file. If `name` is already tracked this records a save
    /// through the normal path and the file's recorded encoding is left
    /// UNCHANGED (encoding is immutable after create).
    ///
    /// # Errors
    /// Returns [`WalrusError::Encoding`] when `encoding` is unknown or `bytes`
    /// cannot be decoded under it, and otherwise as [`Archive::add`].
    pub fn add_with_encoding(
        &mut self,
        name: &str,
        bytes: &[u8],
        generations: Option<u32>,
        dir: Option<Direction>,
        encoding: &str,
    ) -> Result<()> {
        let (text, resolved) = decode_to_utf8(bytes, encoding)?;
        let was_tracked = self.is_tracked(name);
        self.add(name, text.as_bytes(), generations, dir)?;
        if !was_tracked {
            // Fresh create: stamp the resolved canonical encoding label on both
            // the manifest entry and the .wchg header. (A save on an existing
            // file leaves the recorded encoding untouched -- it is immutable.)
            let id = self
                .id_for_name_or_id(name)
                .ok_or_else(|| WalrusError::NotTracked {
                    name: name.to_string(),
                })?;
            self.set_encoding_label(&id, resolved.name())?;
        }
        Ok(())
    }

    /// Reconstruct the tracked file `file` at `version` in a chosen encoding.
    ///
    /// `encoding = Some(label)` encodes the reconstructed UTF-8 to `label`;
    /// `encoding = None` restores the file's RECORDED source encoding, which is
    /// a no-op returning the UTF-8 bytes unchanged for a utf-8-origin file.
    ///
    /// # Errors
    /// Returns [`WalrusError::NotTracked`] when `file` is absent,
    /// [`WalrusError::Encoding`] when the target encoding is unknown or the text
    /// cannot be represented in it, and otherwise as [`Archive::reconstruct`].
    pub fn reconstruct_as_encoding(
        &self,
        file: &str,
        version: Version,
        encoding: Option<&str>,
    ) -> Result<Vec<u8>> {
        let id = self
            .id_for_name_or_id(file)
            .ok_or_else(|| WalrusError::NotTracked {
                name: file.to_string(),
            })?;
        let utf8 = self.reconstruct(&FileId::from(id.as_str()), version)?;
        let label = match encoding {
            Some(label) => label,
            None => self.recorded_encoding(&id),
        };
        // utf-8-origin files whose target is utf-8 are a no-op: return as-is to
        // avoid a needless (and lossless) re-encode round-trip.
        if label.eq_ignore_ascii_case("utf-8") {
            return Ok(utf8);
        }
        let text = std::str::from_utf8(&utf8).map_err(|_| WalrusError::Encoding {
            label: label.to_string(),
            reason: "reconstructed bytes are not valid UTF-8".to_string(),
        })?;
        encode_from_utf8(text, label)
    }

    // === private helpers ================================================

    /// Resolve a selector (name first, then id) to a tracked-file id.
    fn id_for_name_or_id(&self, sel: &str) -> Option<String> {
        self.manifest()
            .files
            .iter()
            .find(|f| f.name == sel || f.id == sel)
            .map(|f| f.id.clone())
    }

    /// Whether a selector matches an already-tracked file.
    fn is_tracked(&self, sel: &str) -> bool {
        self.id_for_name_or_id(sel).is_some()
    }

    /// The recorded source encoding label of the tracked file `id` (defaults to
    /// "utf-8" if the entry is somehow absent).
    fn recorded_encoding(&self, id: &str) -> &str {
        self.manifest()
            .files
            .iter()
            .find(|f| f.id == id)
            .map(|f| f.encoding.as_str())
            .unwrap_or("utf-8")
    }

    /// Stamp the resolved encoding label on both the manifest entry and the
    /// `.wchg` header of a freshly created file.
    fn set_encoding_label(&mut self, id: &str, label: &str) -> Result<()> {
        {
            let tracked = self
                .manifest_mut()
                .files
                .iter_mut()
                .find(|f| f.id == id)
                .ok_or_else(|| WalrusError::UnknownFile { id: id.to_string() })?;
            tracked.encoding = label.to_string();
        }
        let file = self.archive_file_mut(id)?;
        file.header.encoding = label.to_string();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{Manifest, Variant};
    use crate::record::ChangeRecord;
    use encoding_rs::{BIG5, GBK, SHIFT_JIS};

    // ROUND-TRIP ASSUMPTION: each sample string below is chosen so every one of
    // its characters has a UNIQUE encoding-in / decoding-out mapping under the
    // named legacy encoding, so decode_to_utf8 then encode_from_utf8 restores
    // the ORIGINAL bytes exactly. The fixtures are produced from those strings
    // via encoding_rs in the test setup (the .rs source stays strictly ASCII --
    // no multibyte literals are pasted here).

    fn empty_archive(variant: Variant) -> Archive {
        Archive::from_parts_for_test(Manifest::new(variant), Vec::new())
    }

    /// Encode a known UTF-8 sample to a legacy encoding, assert the fixture is
    /// genuinely multi-byte (not accidental ASCII), and return the legacy bytes.
    fn legacy_fixture(sample: &str, encoding: &'static encoding_rs::Encoding) -> Vec<u8> {
        let (bytes, _, had_errors) = encoding.encode(sample);
        assert!(!had_errors, "sample must be representable in the encoding");
        assert!(
            bytes.len() > sample.chars().count(),
            "fixture must be genuinely multi-byte"
        );
        bytes.into_owned()
    }

    #[test]
    fn gbk_add_stores_utf8_and_keep_original_extract_restores_original_bytes() {
        // A GBK sample: the two CJK scalars U+4E2D U+6587 ("Chinese"). Built from
        // escapes so the source stays ASCII.
        let sample: String = ['\u{4E2D}', '\u{6587}'].iter().collect();
        let legacy = legacy_fixture(&sample, GBK);

        let mut a = empty_archive(Variant::Wal);
        a.add_with_encoding("doc.txt", &legacy, None, None, "gbk")
            .expect("add gbk");

        // The STORED current is the transcoded UTF-8, not the legacy bytes.
        let stored = a
            .reconstruct(&FileId::from("f0"), Version::Index(0))
            .expect("reconstruct");
        assert_eq!(stored, sample.as_bytes(), "stored as UTF-8");
        // The recorded encoding is the resolved canonical label.
        assert_eq!(a.file("doc.txt").expect("tracked").encoding, GBK.name());

        // Keep-original (None) extract restores the ORIGINAL legacy bytes.
        let restored = a
            .reconstruct_as_encoding("doc.txt", Version::Index(0), None)
            .expect("keep-original extract");
        assert_eq!(restored, legacy, "keep-original restores the legacy bytes");
    }

    #[test]
    fn shift_jis_add_roundtrips_keep_original() {
        // A Shift-JIS sample: Hiragana "a" U+3042 and "i" U+3044.
        let sample: String = ['\u{3042}', '\u{3044}'].iter().collect();
        let legacy = legacy_fixture(&sample, SHIFT_JIS);

        let mut a = empty_archive(Variant::Wal);
        a.add_with_encoding("doc.txt", &legacy, None, None, "shift_jis")
            .expect("add shift_jis");
        let restored = a
            .reconstruct_as_encoding("doc.txt", Version::Index(0), None)
            .expect("keep-original extract");
        assert_eq!(restored, legacy);
    }

    #[test]
    fn big5_add_roundtrips_keep_original() {
        // A Big5 sample: Traditional CJK U+4E2D U+6587.
        let sample: String = ['\u{4E2D}', '\u{6587}'].iter().collect();
        let legacy = legacy_fixture(&sample, BIG5);

        let mut a = empty_archive(Variant::Wal);
        a.add_with_encoding("doc.txt", &legacy, None, None, "big5")
            .expect("add big5");
        let restored = a
            .reconstruct_as_encoding("doc.txt", Version::Index(0), None)
            .expect("keep-original extract");
        assert_eq!(restored, legacy);
    }

    #[test]
    fn update_preserves_recorded_encoding() {
        // Encoding is immutable: a later save never changes the recorded label.
        let sample: String = ['\u{4E2D}'].iter().collect();
        let legacy = legacy_fixture(&sample, GBK);

        let mut a = empty_archive(Variant::Wal);
        a.add_with_encoding("doc.txt", &legacy, None, None, "gbk")
            .expect("add gbk");
        let recorded = a.file("doc.txt").expect("tracked").encoding.clone();

        // A plain (UTF-8) save does NOT touch the recorded encoding or header.
        let next: String = ['\u{4E2D}', '\u{6587}'].iter().collect();
        a.update_with_splices(
            "doc.txt",
            &[ChangeRecord::splice(sample.len(), 0, "\u{6587}")],
        )
        .expect("save");
        assert_eq!(
            a.file("doc.txt").expect("tracked").encoding,
            recorded,
            "recorded encoding is immutable across a save"
        );
        assert_eq!(
            a.header("doc.txt").expect("header").encoding,
            recorded,
            "header encoding is immutable across a save"
        );
        assert_eq!(
            a.reconstruct(&FileId::from("f0"), Version::Index(0))
                .expect("current"),
            next.as_bytes(),
            "the new current is the UTF-8 save"
        );
    }

    #[test]
    fn reconstruct_as_encoding_none_on_utf8_origin_is_noop() {
        // A plain add records encoding "utf-8"; keep-original extract returns the
        // UTF-8 bytes unchanged.
        let mut a = empty_archive(Variant::Wal);
        a.add("doc.txt", b"hello", None, None).expect("add");
        assert_eq!(a.file("doc.txt").expect("tracked").encoding, "utf-8");
        let out = a
            .reconstruct_as_encoding("doc.txt", Version::Index(0), None)
            .expect("none extract");
        assert_eq!(out, b"hello", "utf-8 keep-original is a no-op");
    }
}
