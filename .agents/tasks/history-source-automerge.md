# Walker as a multi-format history player: the HistorySource abstraction

Status: DESIGN NOTE for the upcoming planning session. Not yet a committed
decision. Prior-art and API claims below were verified against upstream sources
on 2026-10-10 (links inline). This note exists so the planning session starts
from facts, not from memory.

## The question this answers

Can Walker be adapted to read Automerge documents and deliver the same
history-playback functionality it gives `.wal`/`.wbl`, serving BOTH formats from
one GUI? Short answer: yes, cleanly, behind a trait. This note gives the
verified basis and a concrete design.

## Prior art (why this matters)

The core Walrus idea -- a single self-describing file that carries a document
plus its full edit history, reconstructable to any version -- is well-trodden:

- RCS (reverse deltas, single companion file) and SCCS (interleaved "weave")
  are the 1970s-80s ancestors. Open source but GPL command-line tools, NOT
  embeddable libraries; useful as format references only.
  (https://en.wikipedia.org/wiki/Interleaved_deltas ,
   http://web.mit.edu/ghudson/thoughts/file-versioning )
- The modern, active, embeddable equivalents are CRDT libraries:
  - Automerge -- MIT, Rust core (`automerge` on crates.io), self-contained
    binary doc carrying full history. (https://lib.rs/Automerge)
  - Yjs/Yrs (y-crdt) -- MIT, Rust port `yrs`. (https://docs.yjs.dev/license ,
    https://github.com/y-crdt/y-crdt/ )
  - SQLite (public domain, `rusqlite`) is the pragmatic "store versions in an
    embedded DB" alternative.

Both Automerge and Yrs are MIT + Rust + embeddable, so they are the real
"why not just use this" answers for Walrus's single-writer format. The strongest
positioning for Walrus is NOT to out-CRDT them but to be the lean, transparent,
single-writer, spec-first history CONTAINER, and to make Walker a UNIVERSAL
history player that also reads their formats. That turns "Automerge is a
competitor" into "Automerge is a supported input".

## Feasibility: the Automerge APIs Walker needs (all verified)

Automerge's history model maps directly onto Walker's player needs:

- Load from bytes: `Automerge::load(&bytes)` reconstructs a doc with full
  history -- the embeddable entry point. (https://lib.rs/Automerge)
- Version markers = "heads": hash identifiers for a point in history, like git
  commit hashes.
- Snapshot at a version: `view(heads)` returns a cheap immutable snapshot of the
  document at those heads -- this is the "reconstruct to version" primitive.
- Version list / history: `get_changes()` / `getHistory()`; each change exposes
  hash, message, timestamp, actor, seq, deps, size.
- Diff between two points: `diff(from_heads, to_heads)` -- efficient for UI.
  (https://automerge-automerge.mintlify.app/advanced/viewing-history )

Upstream performance guidance to respect in the player: `view()` is cheap (share
memory with the original; create liberally); `getHistory()` is expensive (only
for full traversal); store heads, not snapshots; use `diff()` for UI updates
rather than reconstructing full states each frame.

## File sniffing (how Walker tells the formats apart)

Walker already sniffs the container codec by magic bytes. Add Automerge:

| Format            | Leading magic bytes                 |
|-------------------|-------------------------------------|
| zstd (.wal/.wbl)  | 0x28 0xB5 0x2F 0xFD                 |
| gzip (.wal/.wbl)  | 0x1F 0x8B                           |
| Automerge         | 0x85 0x6F 0x4A 0x83                 |

(Automerge magic verified at
https://automerge.org/automerge-binary-format-spec .) Extension is a hint only;
magic bytes decide, consistent with the existing FORMAT.md invariant.

## Proposed architecture: the HistorySource trait

Walker's transport, player state machine, content renderers, and export backends
all operate on "reconstructed bytes at a chosen version". Introduce one trait so
they never learn which format they are playing:

```rust
/// An opaque version marker. walrus: a numeric index; automerge: a set of heads.
pub enum VersionId {
    Index(usize),          // walrus/wbl
    Heads(Vec<ChangeHash>),// automerge
}

pub struct VersionMeta {
    pub label: String,            // "version 12" or a change message
    pub timestamp: Option<i64>,   // ms since epoch, when known
    pub author: Option<String>,   // actor id / author, when known
}

pub trait HistorySource {
    /// Ordered oldest..current (or current..oldest per variant); the player
    /// owns direction, this just enumerates.
    fn versions(&self) -> Vec<VersionId>;
    /// The document bytes AS OF the given version (the single primitive the
    /// renderers consume).
    fn reconstruct(&self, v: &VersionId) -> anyhow::Result<Vec<u8>>;
    /// Per-version metadata for the status bar / history view.
    fn metadata(&self, v: &VersionId) -> VersionMeta;
    /// MIME/name hint so view::select_renderer picks the right renderer.
    fn content_hint(&self) -> (String /*mime*/, String /*name*/);
}
```

Implementations:

- `WalrusSource` wraps `walrus-fmt`: `versions()` = the index range,
  `reconstruct()` = splice replay to an index, `metadata()` = manifest + log
  record info, `content_hint()` = `manifest().document.{mime,name}`.
- `AutomergeSource` wraps the `automerge` crate: `versions()` = heads walked
  from `get_changes()`, `reconstruct()` = `view(heads)` then serialise the value
  to bytes, `metadata()` = change hash/message/time/actor, `content_hint()` =
  "text/plain" (or detect a single `text` root for richer typing).

Walker sniffs magic bytes, builds the right `HistorySource`, and the rest of the
GUI is unchanged.

## The one real caveat (be honest in planning)

Automerge is a STRUCTURED CRDT (nested map/list/text), not a byte blob:

- If the Automerge doc is essentially a text document (a single `text` object),
  `reconstruct() -> bytes` is clean and maps straight onto Walker's
  text/markdown/code renderers. This is the common case and the sweet spot.
- If the doc is a rich nested structure, "the bytes at this version" means
  SERIALISING a tree (e.g. to JSON), so Walker renders it as structured/JSON
  content with history, not as an "original file". Still useful for a viewer,
  but a different mental model from Walrus's "this IS a .html/.csv with history".

So `AutomergeSource` should (a) detect the single-text-object case and expose it
as text, and (b) fall back to a JSON/tree rendering for structured docs, clearly
labelled in the status bar.

## Dependency-boundary implication (invariant 2 stays intact)

`automerge` is a Walker-side (or feature-gated) dependency ONLY. It MUST NOT
enter `walrus-fmt`: the lean, GUI-free, embeddable core stays walrus-only, same
discipline as the export crates. The `HistorySource` trait itself lives in
`walker` (it is an application concern), not in `walrus-fmt`.

## Decision for the planning session

HEADLINE ARCHITECTURE DECISION, decide BEFORE the GUI rework (it shapes
Walker's core, not a bolt-on):

- Option A: Walker stays walrus-only (`.wal`/`.wbl`). Simpler; no CRDT dep.
- Option B: Walker becomes a multi-format history player (walrus + automerge)
  via `HistorySource`. Broader product story; adds an MIT Rust dependency behind
  a feature flag; needs the structured-vs-text handling above.
  (Yrs could be a later third source behind the same trait.)

Recommendation: adopt the `HistorySource` trait seam NOW even if only
`WalrusSource` ships first -- it is cheap to introduce up front and expensive to
retrofit, and it keeps Option B open without committing to the automerge
dependency until wanted.

## Open questions to resolve in planning

1. Is multi-format in scope for the first Walker rework, or is the trait seam
   introduced now with `AutomergeSource` deferred?
2. For `AutomergeSource`, how do we pick which Automerge object is "the document"
   (root text vs a named field vs whole-tree JSON)?
3. Does the Walker history view (the Markdown change-report export) need to
   represent Automerge's branching/merge history (DAG), which is richer than
   walrus's linear index? walrus history is linear; automerge is a commit DAG.
4. Export-from-automerge: do the existing export backends operate on the
   reconstructed bytes (yes, if text), or do structured docs need their own
   export path?
