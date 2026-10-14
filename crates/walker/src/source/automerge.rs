//! The `AutomergeSource` adapter: a feature-gated, read-only [`HistorySource`]
//! over an `automerge` 0.12.0 document (FR-K11).
//!
//! This is the ONE module in `walker` allowed to name `automerge` types.
//! Everything else in the GUI depends only on the format-agnostic
//! [`HistorySource`] trait (`source/mod.rs`), so the `automerge` dependency and
//! every `automerge` symbol are confined here (INV-2). The whole module is
//! `#[cfg(feature = "automerge")]`-gated, so with the feature off it, its
//! imports, and the dependency compile out entirely.
//!
//! # Model
//!
//! One loaded document is exposed as EXACTLY one [`FileRef`] (id 0). The change
//! DAG is LINEARISED into the Phase-1 index space (index 0 = CURRENT/newest,
//! higher = OLDER), matching the `[0, N]` inclusive convention the walrus
//! source uses, so the player and transport arithmetic work unchanged.
//!
//! Reconstruction reads IN PLACE on the loaded document via the `ReadDoc`
//! `*_at(.., heads)` methods (there is no snapshot/fork object in 0.12.0): a
//! single-text-root document yields the text bytes, a structured document is
//! serialised to deterministic pretty JSON (sorted keys) and fed through the
//! existing `application/json` renderer path.
//!
//! # Labelled limitation (not silently dropped)
//!
//! The change DAG's branch/merge structure is NOT represented in this first
//! cut. [`AutomergeSource::versions`] and [`AutomergeSource::history`] present
//! automerge's own topological (oldest-first) linearisation as a flat list; a
//! genuinely branched-and-merged document is shown as that flat sequence. The
//! [`VersionId::Heads`] variant is head-SET-valued so a later DAG-aware
//! iteration can address merge frontiers without another `VersionId` change.

use std::str::FromStr;

use anyhow::{Context, Result};
use automerge::{Automerge, ChangeHash};

use super::{
    FileInfo, FileRef, HeadId, HistoryEntry, HistorySource, SpliceSummary, VersionId, VersionMeta,
};

// The shape detection and the deterministic `*_at` JSON walk live in a `#[path]`
// child module to keep this file under the 400 non-test-line standard (NFR-1).
// It is a CHILD of the automerge adapter, so it stays inside the one
// `automerge`-aware module and may name `automerge` types freely.
#[path = "automerge_shape.rs"]
mod automerge_shape;

use automerge_shape::Shape;

// === AutomergeSource ====================================================

/// A read-only [`HistorySource`] backed by a loaded `automerge` document.
///
/// Open one with [`AutomergeSource::open`]. The document is exposed as a single
/// [`FileRef`] (id 0); the linearised per-index head table and the cached
/// [`Shape`] are computed once at open.
pub struct AutomergeSource {
    doc: Automerge,
    name: String,
    shape: Shape,
    /// Per-index head table: index 0 holds the current heads, index `k` holds
    /// the heads after the first `N - k` changes (so the last index is the
    /// empty/original state). Length is `N + 1` for `N` changes.
    linear_heads: Vec<Vec<HeadId>>,
    /// The per-index producing change metadata (label/timestamp/author/op
    /// count), aligned with `linear_heads`. Index 0 is produced by the newest
    /// change; the last index (original) has no producing change.
    changes: Vec<ChangeInfo>,
}

/// The metadata of one producing change, in index order.
struct ChangeInfo {
    label: String,
    short_hash: String,
    timestamp: i64,
    author: String,
    op_count: usize,
}

impl AutomergeSource {
    /// Open an Automerge document from `path`.
    ///
    /// # Errors
    /// Returns an error (with context naming the path) if the file cannot be
    /// read or the bytes are not a loadable Automerge document.
    pub fn open(path: impl AsRef<std::path::Path>) -> Result<AutomergeSource> {
        let path = path.as_ref();
        let bytes = std::fs::read(path)
            .with_context(|| format!("reading Automerge document {}", path.display()))?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".to_string());
        Self::from_bytes(&bytes, name)
            .with_context(|| format!("loading Automerge document {}", path.display()))
    }

    /// Build a source from in-memory document bytes and a display name.
    ///
    /// # Errors
    /// Returns an error if the bytes are not a loadable Automerge document.
    fn from_bytes(bytes: &[u8], name: String) -> Result<AutomergeSource> {
        let doc = Automerge::load(bytes).context("decoding Automerge document bytes")?;
        let shape = Shape::detect(&doc, &name);
        let linear_heads = linearise(&doc);
        let changes = change_infos(&doc);
        Ok(AutomergeSource {
            doc,
            name,
            shape,
            linear_heads,
            changes,
        })
    }

    /// The single [`FileRef`] this source exposes, or `None` for a foreign ref.
    fn only_file<'a>(&self, f: &'a FileRef) -> Option<&'a FileRef> {
        (f.id() == 0).then_some(f)
    }

    /// Resolve the `automerge` heads a [`VersionId`] addresses.
    ///
    /// A [`VersionId::Heads`] is converted hex -> [`ChangeHash`]; a bare
    /// [`VersionId::Index`] is mapped through the linear head table as
    /// defence-in-depth (the normal paths always use [`version_at`], which emits
    /// `Heads`).
    ///
    /// # Errors
    /// Returns an error if a head's hex is unparseable, or a bare index is out
    /// of range.
    fn heads_for(&self, v: &VersionId) -> Result<Vec<ChangeHash>> {
        match v {
            VersionId::Heads(heads) => heads
                .iter()
                .map(|h| {
                    ChangeHash::from_str(h.as_str())
                        .map_err(|_| anyhow::anyhow!("unknown or invalid heads"))
                })
                .collect(),
            VersionId::Index(index) => {
                let heads = self
                    .linear_heads
                    .get(*index)
                    .ok_or_else(|| anyhow::anyhow!("version index {index} out of range"))?;
                heads
                    .iter()
                    .map(|h| {
                        ChangeHash::from_str(h.as_str())
                            .map_err(|_| anyhow::anyhow!("unknown or invalid heads"))
                    })
                    .collect()
            }
        }
    }
}

// === HistorySource ======================================================

impl HistorySource for AutomergeSource {
    fn files(&self) -> Vec<FileRef> {
        vec![FileRef::new(0, self.name.as_str())]
    }

    fn versions(&self, f: &FileRef) -> Vec<VersionId> {
        if self.only_file(f).is_none() {
            return Vec::new();
        }
        (0..self.linear_heads.len())
            .map(|index| VersionId::Heads(self.linear_heads[index].clone()))
            .collect()
    }

    fn version_at(&self, f: &FileRef, index: usize) -> VersionId {
        if self.only_file(f).is_none() || self.linear_heads.is_empty() {
            return VersionId::Heads(Vec::new());
        }
        // Clamp an out-of-range index to the far (original) end.
        let clamped = index.min(self.linear_heads.len() - 1);
        VersionId::Heads(self.linear_heads[clamped].clone())
    }

    fn status_note(&self, f: &FileRef) -> Option<String> {
        self.only_file(f)?;
        self.shape.status_note()
    }

    fn reconstruct(&self, f: &FileRef, v: &VersionId) -> Result<Vec<u8>> {
        if self.only_file(f).is_none() {
            return Err(anyhow::anyhow!("no such file: {}", f.name()));
        }
        let heads = self.heads_for(v)?;
        self.shape.reconstruct(&self.doc, &heads)
    }

    fn metadata(&self, f: &FileRef, v: &VersionId) -> VersionMeta {
        if self.only_file(f).is_none() {
            return empty_meta();
        }
        let index = self.index_of(v);
        match index.and_then(|i| self.changes.get(i)) {
            Some(info) => VersionMeta {
                label: if info.label.is_empty() {
                    info.short_hash.clone()
                } else {
                    info.label.clone()
                },
                timestamp: Some(info.timestamp),
                author: Some(info.author.clone()),
            },
            None => empty_meta(),
        }
    }

    fn content_hint(&self, f: &FileRef) -> (String, String) {
        match self.only_file(f) {
            Some(_) => self.shape.content_hint(),
            None => (String::new(), String::new()),
        }
    }

    fn file_info(&self, f: &FileRef) -> FileInfo {
        let Some(file) = self.only_file(f) else {
            return FileInfo {
                index_size: 0,
                generations: None,
                size_bytes: 0,
                log_bytes: 0,
                mime: String::new(),
            };
        };
        let size_bytes = self
            .reconstruct(file, &self.version_at(file, 0))
            .map(|b| b.len() as u64)
            .unwrap_or(0);
        FileInfo {
            index_size: self.linear_heads.len(),
            generations: None,
            size_bytes,
            // No raw log-byte figure exists; the saved document size is the
            // honest change-log byte proxy for the FR-K7 bloat ratio.
            log_bytes: self.doc.save().len() as u64,
            mime: self.shape.content_hint().0,
        }
    }

    fn history(&self, f: &FileRef) -> Vec<HistoryEntry> {
        if self.only_file(f).is_none() {
            return Vec::new();
        }
        self.changes
            .iter()
            .enumerate()
            .filter(|(index, _)| *index < self.linear_heads.len().saturating_sub(1))
            .map(|(index, info)| HistoryEntry {
                index,
                timestamp: Some(info.timestamp),
                note: (!info.label.is_empty()).then(|| info.label.clone()),
                // Automerge changes are not byte splices; the summary is a
                // documented best-effort: no offset/deletion, inserted_len is
                // the change op count.
                splice: SpliceSummary {
                    at: 0,
                    deleted_len: 0,
                    inserted_len: info.op_count,
                },
            })
            .collect()
    }
}

impl AutomergeSource {
    /// The linear index a [`VersionId`] addresses, if resolvable.
    fn index_of(&self, v: &VersionId) -> Option<usize> {
        match v {
            VersionId::Index(index) => (*index < self.linear_heads.len()).then_some(*index),
            VersionId::Heads(heads) => self.linear_heads.iter().position(|h| h == heads),
        }
    }
}

/// A default, index-less [`VersionMeta`].
fn empty_meta() -> VersionMeta {
    VersionMeta {
        label: String::new(),
        timestamp: None,
        author: None,
    }
}

// === linearisation ======================================================

/// Build the per-index head table for `doc`.
///
/// Index 0 is the current heads (`get_heads`); index `k` is the frontier after
/// the first `N - k` changes; the last index (`N`) is the empty/original state
/// (no changes applied). Returns a `N + 1` entry table matching the walrus
/// `[0, N]` inclusive convention.
fn linearise(doc: &Automerge) -> Vec<Vec<HeadId>> {
    let changes = doc.get_changes(&[]);
    let count = changes.len();
    let mut table: Vec<Vec<HeadId>> = Vec::with_capacity(count + 1);
    // index 0 = current heads.
    table.push(heads_to_ids(&doc.get_heads()));
    // index k (1..=count) = heads after the first (count - k) changes. For
    // k = count that is zero changes -> the empty original frontier (no heads).
    for k in 1..=count {
        let applied = count - k;
        // The frontier after `applied` changes is the hash of the last applied
        // change (changes are oldest-first, linear in this first cut); for a
        // genuinely branched doc this flat ordering is the labelled limitation.
        let frontier = if applied == 0 {
            Vec::new()
        } else {
            vec![HeadId::new(changes[applied - 1].hash().to_string())]
        };
        table.push(frontier);
    }
    table
}

/// Build the per-index [`ChangeInfo`] list for `doc`, newest-first (index 0 is
/// the newest change). Length equals the change count.
fn change_infos(doc: &Automerge) -> Vec<ChangeInfo> {
    let changes = doc.get_changes(&[]);
    // changes are oldest-first; index 0 must be the NEWEST change, so reverse.
    changes
        .iter()
        .rev()
        .map(|c| {
            let hash = c.hash().to_string();
            let short_hash = hash.chars().take(8).collect::<String>();
            ChangeInfo {
                label: c.message().map(str::to_owned).unwrap_or_default(),
                short_hash,
                timestamp: c.timestamp(),
                author: c.actor_id().to_hex_string(),
                op_count: c.len(),
            }
        })
        .collect()
}

/// Convert an `automerge` heads slice into walker-side [`HeadId`]s.
fn heads_to_ids(heads: &[ChangeHash]) -> Vec<HeadId> {
    heads.iter().map(|h| HeadId::new(h.to_string())).collect()
}

// === Tests ==============================================================
#[cfg(test)]
mod tests {
    use super::*;
    use automerge::transaction::Transactable;
    use automerge::{AutoCommit, ObjType, ROOT};

    // === fixtures =======================================================

    /// Build a single-text-root document: the root map has one key `doc.txt`
    /// whose value is a Text object. `edits` are applied one per change (each
    /// `splice_text` at the current end), so there are `edits.len()` changes.
    fn text_doc(key: &str, edits: &[&str]) -> Vec<u8> {
        let mut doc = AutoCommit::new();
        let text = doc
            .put_object(ROOT, key, ObjType::Text)
            .expect("put text object");
        doc.commit();
        let mut at = 0usize;
        for edit in edits {
            doc.splice_text(&text, at, 0, edit).expect("splice text");
            at += edit.chars().count();
            doc.commit();
        }
        doc.save()
    }

    /// Build a structured document: a root map with several keys (scalars, a
    /// nested map, a list) in a single change.
    fn structured_doc() -> Vec<u8> {
        let mut doc = AutoCommit::new();
        doc.put(ROOT, "title", "walrus").expect("put title");
        doc.put(ROOT, "count", 3_i64).expect("put count");
        doc.put(ROOT, "ready", true).expect("put ready");
        let nested = doc
            .put_object(ROOT, "meta", ObjType::Map)
            .expect("put meta");
        doc.put(&nested, "author", "alice").expect("put author");
        let list = doc
            .put_object(ROOT, "tags", ObjType::List)
            .expect("put tags");
        doc.insert(&list, 0, "a").expect("insert a");
        doc.insert(&list, 1, "b").expect("insert b");
        doc.commit();
        doc.save()
    }

    fn open(bytes: &[u8], name: &str) -> AutomergeSource {
        AutomergeSource::from_bytes(bytes, name.to_string()).expect("load document")
    }

    // === files + shape ==================================================

    #[test]
    fn files_lists_exactly_one_file_with_id_zero() {
        let source = open(&text_doc("doc.txt", &["hello"]), "note.amrg");
        let files = source.files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].id(), 0);
        assert_eq!(files[0].name(), "note.amrg");
    }

    #[test]
    fn a_single_text_root_doc_detects_the_text_shape() {
        let source = open(&text_doc("doc.txt", &["hello"]), "note.amrg");
        let file = &source.files()[0];
        assert_eq!(source.status_note(file), None);
        let (mime, _name) = source.content_hint(file);
        assert_eq!(mime, "text/plain");
    }

    #[test]
    fn a_multi_key_doc_detects_the_json_shape() {
        let source = open(&structured_doc(), "data.amrg");
        let file = &source.files()[0];
        assert_eq!(
            source.status_note(file),
            Some("Automerge (structured JSON view)".to_string())
        );
        let (mime, _name) = source.content_hint(file);
        assert_eq!(mime, "application/json");
    }

    // === linearisation ==================================================

    #[test]
    fn a_k_change_text_doc_lists_k_plus_one_versions_zero_current() {
        // 1 object-create change + 3 text edits = 4 changes -> 5 versions.
        let source = open(&text_doc("doc.txt", &["a", "b", "c"]), "n.amrg");
        let file = &source.files()[0];
        assert_eq!(source.versions(file).len(), 5);
    }

    #[test]
    fn version_at_zero_equals_the_current_heads() {
        let source = open(&text_doc("doc.txt", &["a", "b"]), "n.amrg");
        let file = &source.files()[0];
        let current = source.version_at(file, 0);
        let expected = VersionId::Heads(heads_to_ids(&source.doc.get_heads()));
        assert_eq!(current, expected);
    }

    #[test]
    fn version_at_an_out_of_range_index_clamps_to_the_far_end() {
        let source = open(&text_doc("doc.txt", &["a"]), "n.amrg");
        let file = &source.files()[0];
        let far = source.version_at(file, 999);
        let last = source.linear_heads.len() - 1;
        assert_eq!(far, source.version_at(file, last));
    }

    // === reconstruct ====================================================

    #[test]
    fn reconstruct_at_index_zero_returns_the_current_text() {
        let source = open(&text_doc("doc.txt", &["hello ", "world"]), "n.amrg");
        let file = &source.files()[0];
        let bytes = source
            .reconstruct(file, &source.version_at(file, 0))
            .expect("reconstruct current");
        assert_eq!(bytes, b"hello world");
    }

    #[test]
    fn reconstruct_at_a_higher_index_returns_the_older_text() {
        let source = open(&text_doc("doc.txt", &["hello ", "world"]), "n.amrg");
        let file = &source.files()[0];
        // index 1 is one change older than current: before "world" was spliced.
        let bytes = source
            .reconstruct(file, &source.version_at(file, 1))
            .expect("reconstruct older");
        assert_eq!(bytes, b"hello ");
    }

    #[test]
    fn reconstruct_at_the_original_index_returns_empty_bytes() {
        let source = open(&text_doc("doc.txt", &["hello"]), "n.amrg");
        let file = &source.files()[0];
        let last = source.linear_heads.len() - 1;
        let bytes = source
            .reconstruct(file, &source.version_at(file, last))
            .expect("reconstruct original");
        assert!(bytes.is_empty(), "original state has no text content yet");
    }

    #[test]
    fn reconstruct_with_a_bare_index_zero_matches_version_at_zero() {
        // Defence-in-depth export fallback: a bare Index(0) reconstructs the
        // same bytes as version_at(0).
        let source = open(&text_doc("doc.txt", &["hello"]), "n.amrg");
        let file = &source.files()[0];
        let via_index = source
            .reconstruct(file, &VersionId::Index(0))
            .expect("reconstruct via index");
        let via_heads = source
            .reconstruct(file, &source.version_at(file, 0))
            .expect("reconstruct via heads");
        assert_eq!(via_index, via_heads);
    }

    #[test]
    fn a_structured_doc_reconstructs_to_sorted_key_pretty_json() {
        let bytes = structured_doc();
        let source = open(&bytes, "data.amrg");
        let file = &source.files()[0];
        let json = source
            .reconstruct(file, &source.version_at(file, 0))
            .expect("reconstruct json");
        let text = String::from_utf8(json).expect("utf8 json");
        // Sorted keys: count < meta < ready < tags < title.
        let value: serde_json::Value = serde_json::from_str(&text).expect("parse json");
        assert_eq!(value["title"], serde_json::json!("walrus"));
        assert_eq!(value["count"], serde_json::json!(3));
        assert_eq!(value["ready"], serde_json::json!(true));
        assert_eq!(value["meta"]["author"], serde_json::json!("alice"));
        assert_eq!(value["tags"], serde_json::json!(["a", "b"]));
        // Determinism: byte-identical across two independent loads.
        let source2 = open(&bytes, "data.amrg");
        let json2 = source2
            .reconstruct(
                &source2.files()[0],
                &source2.version_at(&source2.files()[0], 0),
            )
            .expect("reconstruct json 2");
        assert_eq!(text.into_bytes(), json2);
    }

    // === metadata / content_hint / file_info ============================

    #[test]
    fn metadata_maps_timestamp_as_seconds_and_actor_as_author() {
        let source = open(&text_doc("doc.txt", &["x"]), "n.amrg");
        let file = &source.files()[0];
        let meta = source.metadata(file, &source.version_at(file, 0));
        assert!(meta.timestamp.is_some());
        assert!(meta.author.is_some());
        assert!(!meta.author.unwrap().is_empty());
    }

    #[test]
    fn file_info_reports_index_size_equal_to_version_count() {
        let source = open(&text_doc("doc.txt", &["a", "b"]), "n.amrg");
        let file = &source.files()[0];
        let info = source.file_info(file);
        assert_eq!(info.index_size, source.versions(file).len());
        assert_eq!(info.generations, None);
        assert_eq!(info.mime, "text/plain");
        assert!(info.log_bytes > 0);
    }

    // === history ========================================================

    #[test]
    fn history_lists_one_entry_per_change_in_index_order() {
        let source = open(&text_doc("doc.txt", &["a", "b", "c"]), "n.amrg");
        let file = &source.files()[0];
        let history = source.history(file);
        // 4 changes -> 5 versions -> entries for indices 0..=3 (not the
        // original end).
        let indices: Vec<usize> = history.iter().map(|e| e.index).collect();
        assert_eq!(indices, vec![0, 1, 2, 3]);
        // Best-effort splice summary: at 0, no deletion, inserted_len = op count.
        assert_eq!(history[0].splice.at, 0);
        assert_eq!(history[0].splice.deleted_len, 0);
        assert!(history[0].splice.inserted_len >= 1);
    }

    // === foreign ref ====================================================

    #[test]
    fn a_foreign_file_ref_yields_benign_defaults() {
        let source = open(&text_doc("doc.txt", &["a"]), "n.amrg");
        let foreign = FileRef::new(99, "ghost");
        assert!(source.versions(&foreign).is_empty());
        assert!(source.history(&foreign).is_empty());
        assert_eq!(source.status_note(&foreign), None);
        assert_eq!(source.file_info(&foreign).index_size, 0);
        assert_eq!(
            source.content_hint(&foreign),
            (String::new(), String::new())
        );
        assert!(source.reconstruct(&foreign, &VersionId::Index(0)).is_err());
    }
}
