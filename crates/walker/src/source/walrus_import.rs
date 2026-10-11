//! The FR-K5 import (WRITE) orchestration for [`WalrusSource`].
//!
//! This is a child module of `source/walrus.rs` (declared there with a
//! `#[path]` attribute), so it stays INSIDE the one `walrus_fmt`-aware adapter:
//! the FR-K1 AC2 boundary ("no `walrus_fmt` type in a signature outside the
//! walrus adapter") still holds. It was split out purely to keep each `.rs`
//! file under the 400 non-test-line standard (NFR-1); it has no behaviour of
//! its own beyond what `walrus.rs` previously inlined.
//!
//! Import is the first WRITE path in the GUI. The in-memory mutate goes through
//! a crate-private [`MutateArchive`] seam so the ORCHESTRATION (resolve ->
//! `add`/`update` -> refresh) is unit-tested against a `FakeArchive` without the
//! real `walrus_fmt` mutate op (`Archive::add`/`update`, FR-W7), which is a
//! cross-stream dependency not yet merged on this branch. The real impl + the
//! `WalrusSource::import` method live behind the `import-mutate` cargo feature.

use anyhow::{Context, Result};

use crate::import::ImportResolution;

// === MutateArchive seam =================================================

/// The subset of the archive mutate surface the import path needs.
///
/// A seam so the import ORCHESTRATION (resolve -> `add` vs `update` -> refresh)
/// is unit-tested against a `FakeArchive` without the real `walrus_fmt` mutate
/// op, which is a cross-stream dependency (FR-W7) that is not yet merged on this
/// branch. The real impl for `walrus_fmt::Archive` lives behind the
/// `import-mutate` cargo feature (see [`real_mutate`]).
///
/// This trait is CRATE-private and defined inside the one `walrus_fmt`-aware
/// adapter module tree, so the FR-K1 AC2 boundary holds (no archive type leaks
/// out).
// Used by the `#[cfg(test)]` orchestration tests (a `FakeArchive` impl) and, in
// a non-test build, only by the `import-mutate`-gated real impl + `import`
// method. Without that feature the sole non-test user is compiled out, so allow
// dead_code there rather than widen the API surface.
#[cfg_attr(not(feature = "import-mutate"), allow(dead_code))]
pub(crate) trait MutateArchive {
    /// Add `name` with `bytes`: create when `name` is untracked, else append a
    /// new save, mirroring `walrus_fmt::Archive::add`.
    ///
    /// # Errors
    /// Returns an error when the backing archive rejects the mutation (e.g.
    /// non-UTF-8 bytes for a text-only format).
    fn add(&mut self, name: &str, bytes: &[u8]) -> Result<()>;

    /// Append a new version to the existing `name`.
    ///
    /// # Errors
    /// Returns an error when `name` is not tracked or the archive rejects the
    /// mutation.
    fn update(&mut self, name: &str, bytes: &[u8]) -> Result<()>;
}

/// Apply a resolved import to a [`MutateArchive`]: dispatch the resolution to
/// the right mutate call. Pure orchestration, generic over the seam so it is
/// tested with a `FakeArchive` now and wired to the real archive once FR-W7
/// lands.
///
/// `Add`/`AddAs` route to [`MutateArchive::add`] (create / add under a new
/// name); a confirmed `UpdatePrompt` routes to [`MutateArchive::update`]
/// (append a version to the existing name).
///
/// # Errors
/// Propagates any error from the underlying mutate call, with context naming
/// the resolution.
// See `MutateArchive`: exercised by the `#[cfg(test)]` tests and the
// `import-mutate`-gated `import` method; allow dead_code when the feature is off.
#[cfg_attr(not(feature = "import-mutate"), allow(dead_code))]
pub(crate) fn apply_resolution(
    archive: &mut dyn MutateArchive,
    resolution: &ImportResolution,
    bytes: &[u8],
) -> Result<()> {
    match resolution {
        ImportResolution::Add { name } => archive
            .add(name, bytes)
            .with_context(|| format!("adding new tracked file {name}")),
        ImportResolution::AddAs { new_name, .. } => archive
            .add(new_name, bytes)
            .with_context(|| format!("adding import under new name {new_name}")),
        ImportResolution::UpdatePrompt { name } => archive
            .update(name, bytes)
            .with_context(|| format!("appending a new version to {name}")),
    }
}

// === WalrusSource::import ===============================================

#[cfg(feature = "import-mutate")]
impl super::WalrusSource {
    /// Import `bytes` into the archive per `resolution`, then persist the result
    /// to `path` TRANSACTIONALLY and refresh the file list.
    ///
    /// The in-memory mutate goes through [`apply_resolution`]; the on-disk write
    /// uses `Archive::write`, which stages the full container in a sibling temp
    /// file and renames last (FORMAT.md section 6), so a failed or interrupted
    /// import leaves any pre-existing archive at `path` byte-for-byte unchanged.
    /// On success the `file_ids` cache is rebuilt so a later `files()` lists the
    /// added/updated file.
    ///
    /// Requires the `import-mutate` cargo feature: the `walrus_fmt` mutate API
    /// (`Archive::add`/`update`, FR-W7) is a cross-stream dependency not yet
    /// merged on this branch. Without the feature this method is absent and the
    /// import flow stops at the resolved [`ImportResolution`], which is still
    /// fully headless-tested via [`apply_resolution`] + a `FakeArchive`.
    ///
    /// # Errors
    /// Returns an error if the mutate is rejected (e.g. non-UTF-8 bytes, or an
    /// update of an untracked name) or if the transactional write fails; in the
    /// write-failure case `path` is unchanged.
    pub fn import(
        &mut self,
        resolution: &ImportResolution,
        bytes: &[u8],
        path: &std::path::Path,
    ) -> Result<()> {
        // `walrus_import` is a child module of `walrus` (declared there with
        // `#[path]`), so it may touch `WalrusSource`'s private `archive` /
        // `file_ids` directly.
        apply_resolution(&mut self.archive, resolution, bytes)?;
        self.archive
            .write(path)
            .with_context(|| format!("writing imported archive to {}", path.display()))?;
        self.file_ids = self
            .archive
            .files()
            .map(|tracked| walrus_fmt::FileId::from(tracked.id.as_str()))
            .collect();
        Ok(())
    }
}

// === Real MutateArchive impl (gated on FR-W7) ===========================

/// The real [`MutateArchive`] for `walrus_fmt::Archive`, gated on the
/// `import-mutate` feature because the underlying API (FR-W7) is not on this
/// branch yet.
///
/// Both `add` and `update` route through `Archive::add`: per the FR-W7 contract
/// `add` on a TRACKED name records a new save exactly as the splice-based update
/// path would, so a single call covers BOTH the create and the append cases
/// without enabling the `diff`-gated `Archive::update(bytes)`. When FR-W7 merges
/// and this feature is flipped on, this is the only code that needs to compile
/// against the new API.
#[cfg(feature = "import-mutate")]
mod real_mutate {
    use anyhow::Result;
    use walrus_fmt::Archive;

    use super::MutateArchive;

    impl MutateArchive for Archive {
        fn add(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
            // `dir = None` -> the archive variant's default direction on create;
            // `generations = None` -> inherit the archive default.
            Archive::add(self, name, bytes, None, None).map_err(anyhow::Error::from)
        }

        fn update(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
            // `add` on a tracked name appends one net save (FR-W7 contract), so
            // the update path reuses it rather than the diff-gated update.
            Archive::add(self, name, bytes, None, None).map_err(anyhow::Error::from)
        }
    }
}

// === Tests ==============================================================
#[cfg(test)]
mod tests {
    use super::{apply_resolution, MutateArchive};
    use crate::import::ImportResolution;
    use anyhow::Result;

    // === Import orchestration (FR-K5) ===================================
    //
    // These exercise the resolve -> add/update dispatch through the
    // `MutateArchive` seam with a recording `FakeArchive`, so the import
    // ORCHESTRATION runs GREEN now even while the real `walrus_fmt` mutate API
    // (FR-W7) is unmerged. The real-archive, on-disk tests (AC1 + the
    // transactional guarantee) live in `tests/import.rs` behind the
    // `import-mutate` feature.

    /// A `MutateArchive` that records the calls made to it so a test can assert
    /// the import routed to the right op and never mutates a real archive.
    #[derive(Default)]
    struct FakeArchive {
        adds: Vec<(String, Vec<u8>)>,
        updates: Vec<(String, Vec<u8>)>,
        fail: bool,
    }

    impl MutateArchive for FakeArchive {
        fn add(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
            if self.fail {
                return Err(anyhow::anyhow!("forced add failure"));
            }
            self.adds.push((name.to_string(), bytes.to_vec()));
            Ok(())
        }

        fn update(&mut self, name: &str, bytes: &[u8]) -> Result<()> {
            if self.fail {
                return Err(anyhow::anyhow!("forced update failure"));
            }
            self.updates.push((name.to_string(), bytes.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn walrus_source_import_routes_a_new_name_to_add() {
        let mut archive = FakeArchive::default();
        let resolution = ImportResolution::Add {
            name: "extra.txt".to_string(),
        };
        apply_resolution(&mut archive, &resolution, b"hello").expect("add dispatch");
        assert_eq!(
            archive.adds,
            vec![("extra.txt".to_string(), b"hello".to_vec())]
        );
        assert!(archive.updates.is_empty(), "a new name never calls update");
    }

    #[test]
    fn walrus_source_import_routes_a_confirmed_update_to_update() {
        let mut archive = FakeArchive::default();
        let resolution = ImportResolution::UpdatePrompt {
            name: "a.txt".to_string(),
        };
        apply_resolution(&mut archive, &resolution, b"v2").expect("update dispatch");
        assert_eq!(archive.updates, vec![("a.txt".to_string(), b"v2".to_vec())]);
        assert!(
            archive.adds.is_empty(),
            "a confirmed update never calls add"
        );
    }

    #[test]
    fn walrus_source_import_routes_add_as_to_add_under_the_new_name() {
        let mut archive = FakeArchive::default();
        let resolution = ImportResolution::AddAs {
            original: "a.txt".to_string(),
            new_name: "a-copy.txt".to_string(),
        };
        apply_resolution(&mut archive, &resolution, b"v2").expect("add-as dispatch");
        assert_eq!(
            archive.adds,
            vec![("a-copy.txt".to_string(), b"v2".to_vec())],
            "add-as adds under the NEW name, not the original"
        );
        assert!(archive.updates.is_empty());
    }

    #[test]
    fn walrus_source_import_propagates_a_mutate_failure() {
        let mut archive = FakeArchive {
            fail: true,
            ..FakeArchive::default()
        };
        let resolution = ImportResolution::Add {
            name: "extra.txt".to_string(),
        };
        let result = apply_resolution(&mut archive, &resolution, b"hello");
        assert!(result.is_err(), "a mutate failure propagates to the caller");
    }
}
