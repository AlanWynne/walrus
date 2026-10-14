//! The FR-K6 per-file and batch mutate orchestration for [`WalrusSource`].
//!
//! This is a child module of `source/walrus.rs` (declared there with a
//! `#[path]` attribute), so it stays INSIDE the one `walrus_fmt`-aware adapter:
//! the FR-K1 AC2 boundary (no `walrus_fmt` type in a signature outside the
//! walrus adapter) still holds. It was split out purely to keep each `.rs` file
//! under the 400 non-test-line standard (NFR-1).
//!
//! The per-file (set-generations / condense / remove) and batch (condense /
//! remove) controls all go through the library TRANSACTIONAL path: mutate the
//! in-memory [`Archive`], then persist with `Archive::write`, which stages the
//! full container in a sibling temp file and renames last (FORMAT.md section 6),
//! so a failed write leaves any pre-existing archive at `path` byte-for-byte
//! unchanged. On success the `file_ids` cache is rebuilt so a later `files()`
//! reflects the mutation. Each method resolves a plain `FileRef`/`usize` id to
//! an exact archive id string and passes THAT as the mask, so a per-file op
//! never matches a same-named sibling. Export is NOT a mutate: it reuses the
//! existing read seam + `export_ui` and so lives in the app shell, not here.

use std::path::Path;

use anyhow::{Context, Result};

use super::FileRef;

impl super::WalrusSource {
    /// Set (or clear) the generation retention cap for `f`, then persist to
    /// `path` transactionally and refresh the file list. `n = None` maps to a
    /// cap of 0 (unlimited in the 0.1 format).
    ///
    /// # Errors
    /// Returns an error if `f` is a stale/foreign ref, if the archive rejects
    /// the mutation, or if the transactional write fails; in the write-failure
    /// case `path` is unchanged.
    pub fn set_generations(&mut self, f: &FileRef, n: Option<u32>, path: &Path) -> Result<()> {
        let id = self.resolve_id(f)?;
        self.archive
            .set_generations(&id, n.unwrap_or(0))
            .with_context(|| format!("setting generation cap on {}", f.name()))?;
        self.persist(path)
    }

    /// Condense `f` to `keep` versions beyond the current, discarding everything
    /// older, then persist to `path` transactionally and refresh the file list.
    ///
    /// # Errors
    /// Returns an error if `f` is a stale/foreign ref, if the archive rejects
    /// the condense, or if the transactional write fails; in the write-failure
    /// case `path` is unchanged.
    pub fn condense_file(&mut self, f: &FileRef, keep: usize, path: &Path) -> Result<()> {
        let id = self.resolve_id(f)?;
        self.archive
            .condense(&id, keep)
            .with_context(|| format!("condensing {} to keep {keep}", f.name()))?;
        self.persist(path)
    }

    /// Remove the single file `f`, then persist to `path` transactionally and
    /// refresh the file list. A single-id match removes regardless of `force`,
    /// so this never needs the batch confirmation gate.
    ///
    /// # Errors
    /// Returns an error if `f` is a stale/foreign ref, if the archive rejects
    /// the remove, or if the transactional write fails; in the write-failure
    /// case `path` is unchanged.
    pub fn remove_file(&mut self, f: &FileRef, path: &Path) -> Result<()> {
        let id = self.resolve_id(f)?;
        self.archive
            .remove(&id, false)
            .with_context(|| format!("removing {}", f.name()))?;
        self.persist(path)
    }

    /// Remove every file named by `ids` (a batch remove), then persist to `path`
    /// transactionally and refresh the file list. Each id resolves to an exact
    /// archive id, so each `remove` is a single-id match (`force` is irrelevant)
    /// and no name-mask ambiguity can arise.
    ///
    /// # Errors
    /// Returns an error if any id is stale/foreign, if the archive rejects a
    /// remove, or if the transactional write fails; in the write-failure case
    /// `path` is unchanged. The ids are resolved up front, so a bad id errors
    /// before any in-memory removal.
    pub fn remove_files(&mut self, ids: &[usize], path: &Path) -> Result<()> {
        let archive_ids = self.resolve_ids(ids)?;
        for id in &archive_ids {
            self.archive
                .remove(id, false)
                .with_context(|| format!("removing archive file {id}"))?;
        }
        self.persist(path)
    }

    /// Condense every file named by `ids` to the same `keep` count (a batch
    /// condense), then persist to `path` transactionally and refresh the file
    /// list.
    ///
    /// # Errors
    /// Returns an error if any id is stale/foreign, if the archive rejects a
    /// condense, or if the transactional write fails; in the write-failure case
    /// `path` is unchanged.
    pub fn condense_files(&mut self, ids: &[usize], keep: usize, path: &Path) -> Result<()> {
        let archive_ids = self.resolve_ids(ids)?;
        for id in &archive_ids {
            self.archive
                .condense(id, keep)
                .with_context(|| format!("condensing archive file {id} to keep {keep}"))?;
        }
        self.persist(path)
    }

    // === internal helpers ===============================================

    /// Resolve `f` to its exact archive id string, or error for a foreign ref.
    fn resolve_id(&self, f: &FileRef) -> Result<String> {
        self.file_id_for(f)
            .ok_or_else(|| anyhow::anyhow!("no tracked file for id {}", f.id()))
    }

    /// Resolve each 0-based manifest id to its archive id string, erroring on
    /// the first stale/foreign id so a batch never partly mutates in memory
    /// before discovering a bad id.
    fn resolve_ids(&self, ids: &[usize]) -> Result<Vec<String>> {
        ids.iter()
            .map(|&index| {
                self.file_id_by_index(index)
                    .ok_or_else(|| anyhow::anyhow!("no tracked file for id {index}"))
            })
            .collect()
    }

    /// Persist the in-memory archive to `path` transactionally and rebuild the
    /// `file_ids` cache so a later `files()` reflects the mutation.
    fn persist(&mut self, path: &Path) -> Result<()> {
        self.archive
            .write(path)
            .with_context(|| format!("writing mutated archive to {}", path.display()))?;
        self.file_ids = self
            .archive
            .files()
            .map(|tracked| walrus_fmt::FileId::from(tracked.id.as_str()))
            .collect();
        Ok(())
    }
}
