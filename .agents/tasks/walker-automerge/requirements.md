# Walker Phase 2 requirements -- Automerge support (FR-K11)

Status: REQUIREMENTS (Phase 2, Phase A step 1). Scoped to the `walker` egui GUI
crate only. Derived from `requirements-walker.md` section 5 (FR-K11 / FR-K12),
the design note `.agents/tasks/history-source-automerge.md`, and the CURRENT
`HistorySource` trait in `crates/walker/src/source/mod.rs`.

ASCII only (`--`, `->`, `<->`, `<=`) per the documentation steering rule.

## Summary

Phase 1 shipped Walker as a full GUI frontend for the multi-file `.wal`/`.wbl`
format, coded entirely to the format-agnostic `HistorySource` trait (FR-K1) with
`WalrusSource` as the only implementation. Phase 2 adds a SECOND implementation,
`AutomergeSource`, so Walker can open a real Automerge document, list its
history, reconstruct and render a past version, and play it through the exact
same transport -- without changing any Phase-1 `WalrusSource` behaviour.

This is FR-K11 ONLY. FR-K12 (Yrs as a third source) is explicitly out of scope.

The whole point of the Phase-1 trait seam was to make this cheap: the GUI,
transport, player state machine, renderers, and export already consume the trait
and nothing else, so Phase 2 is one new adapter plus the magic-byte sniffing and
the version-addressing variant the adapter needs -- not a GUI rework.

## Scope note: the trait has drifted from the design note

The design note (`history-source-automerge.md`) sketches a SINGLE-FILE,
single-document trait (`versions()`, `reconstruct(v)`, `metadata(v)`,
`content_hint()`). The trait that actually shipped in Phase 1 is MULTI-FILE and
per-`FileRef`:

```
fn files(&self) -> Vec<FileRef>;
fn versions(&self, f: &FileRef) -> Vec<VersionId>;
fn reconstruct(&self, f: &FileRef, v: &VersionId) -> Result<Vec<u8>>;
fn metadata(&self, f: &FileRef, v: &VersionId) -> VersionMeta;
fn content_hint(&self, f: &FileRef) -> (String, String);
fn file_info(&self, f: &FileRef) -> FileInfo;
fn history(&self, f: &FileRef) -> Vec<HistoryEntry>;
```

`VersionId` is today a single-variant enum `Index(usize)` (0 = CURRENT, higher =
OLDER), documented as deliberately open so a `Heads(..)` variant is churn-free.
`VersionMeta`, `FileInfo`, `FileRef { id, name }`, `HistoryEntry` and
`SpliceSummary` are plain walker-side structs carrying NO `walrus_fmt` types.

Requirements below target the SHIPPED trait, not the design-note sketch. The
single-document Automerge model is adapted to the multi-file trait by presenting
the Automerge document as a source whose `files()` returns the document's
addressable content object(s) (see FR-K11.5).

## Functional requirements

### FR-K11.1 Automerge source selection by magic bytes
Walker picks the source implementation by sniffing leading magic bytes, not by
extension (consistent with the existing codec-sniff invariant). Automerge magic
is `0x85 0x6F 0x4A 0x83`. A file whose bytes begin with that signature is opened
with `AutomergeSource`; `.wal`/`.wbl` (zstd `0x28 0xB5 0x2F 0xFD`, gzip
`0x1F 0x8B`) continue to open with `WalrusSource` exactly as in Phase 1.

### FR-K11.2 AutomergeSource implements the shipped HistorySource trait
`AutomergeSource` wraps the `automerge` crate and implements every method of the
MULTI-FILE `HistorySource` trait so the GUI consumes it unchanged:
- `files()` -- the document's addressable content object(s) as `FileRef`s.
- `versions(f)` -- the document's history as `VersionId`s in index order
  (0 = current, higher = older), walked from the change list.
- `reconstruct(f, v)` -- the content bytes AS OF version `v`, from an
  Automerge snapshot at the corresponding heads.
- `metadata(f, v)` -- label/timestamp/author from the change (hash/message/
  time/actor).
- `content_hint(f)` -- a `(mime, name)` pair driving renderer selection.
- `file_info(f)` -- index size, generations (`None` for Automerge), current
  size, log-byte figure, and mime, so the navigator and FR-K7 bloat compute from
  the trait alone.
- `history(f)` -- the change-log entries for the history viewer.

### FR-K11.3 VersionId gains a Heads variant
Add `Heads(Vec<ChangeHash>)` to `VersionId` for the Automerge DAG. The enum is
already documented as open for exactly this; the addition must be purely
additive -- `Index(usize)` and all its Phase-1 semantics (0 = current, higher =
older) stay unchanged, and every existing `WalrusSource` and in-memory-source
path continues to match and behave identically.

### FR-K11.4 Dependency boundary (INV-2) -- automerge is walker-only and feature-gated
`automerge` is a WALKER-ONLY dependency, declared behind a cargo feature. It MUST
NOT appear in `walrus-fmt` (which stays GUI-free and embeddable) nor in `walx`.
With the feature OFF, `AutomergeSource` and its `automerge` dependency compile
out entirely and Walker builds and behaves as a `WalrusSource`-only Phase-1 GUI.
The `HistorySource` trait itself (an application concern) stays in `walker`.

### FR-K11.5 Structured-vs-text content caveat
Automerge is a STRUCTURED CRDT (nested map/list/text), not a byte blob, so
"the bytes at this version" depends on the document shape:
- Text-root case: when the document is essentially a single `text` object, the
  reconstructed bytes are that text and feed Walker's text/markdown/code
  renderers directly (the common, clean case).
- Structured case: when the document is a rich nested structure, reconstruction
  SERIALISES the tree to JSON (or an equivalent tree rendering) and Walker
  renders it as structured/JSON content with history.
`AutomergeSource` must detect which case applies and set `content_hint` and the
status-bar labelling accordingly, so the user is never misled into reading a
serialised tree as if it were an original file.

### FR-K11.6 Phase-1 behaviour unchanged
Opening, listing, reconstructing, rendering, transport playback, navigator,
import, bloat, history view, and export for `.wal`/`.wbl` via `WalrusSource` must
behave byte-for-byte as before. Phase 2 is additive; no Phase-1 acceptance
criterion may regress.

## Acceptance criteria

1. Opening a file whose leading bytes are `0x85 0x6F 0x4A 0x83` selects
   `AutomergeSource`; a `.wal`/`.wbl` file still selects `WalrusSource`.
   (AC maps to FR-K11.1.)
2. Given a real Automerge TEXT document, `files()` + `versions()` list its
   history in index order (0 = current, higher = older). (FR-K11.2)
3. `reconstruct(f, v)` on that document returns the text bytes AS OF version `v`,
   and `reconstruct(f, Index(0))` returns the current text. (FR-K11.2)
4. The reconstructed bytes of a past version render through the SAME content
   renderers, and the document plays through the SAME transport as a
   `WalrusSource` file, with no transport/player code aware of the format.
   (FR-K11.2, FR-K11.6)
5. `VersionId::Heads(Vec<ChangeHash>)` exists and is additive: all existing
   `Index(usize)` behaviour and tests pass unchanged. (FR-K11.3)
6. With the automerge feature OFF, the crate builds with no `automerge`
   dependency compiled in (verifiable by `cargo tree -p walker` showing no
   `automerge`) and all Phase-1 behaviour is intact; `walrus-fmt` and `walx`
   never gain the dependency in either feature state. (FR-K11.4)
7. A single-`text`-root Automerge document reports a text mime via
   `content_hint` and renders as text; a structured Automerge document reports a
   JSON/tree hint, renders as structured content, and is clearly labelled as
   such in the status bar. (FR-K11.5)
8. Every Phase-1 `WalrusSource` test continues to pass with the automerge
   feature both ON and OFF. (FR-K11.6)

## Non-functional requirements

- NFR-1 Source files stay under 400 non-test lines; the Automerge adapter lives
  in its own module (`source/automerge.rs`) analogous to `source/walrus.rs`.
- NFR-2 `cargo fmt -- --check` and `cargo clippy -p walker -- -D warnings` clean
  with the feature both ON and OFF.
- NFR-3 Automerge reconstruct/version logic is pure and unit-tested; prefer a
  headless `egui_kittest` harness for any rendered-widget behaviour, MANUAL only
  for pixel-exact appearance or the `rfd` native dialog (reason stated).
- NFR-4 Full gate is the owner's manual step; scoped `-p walker` is the inner
  loop.

## Out of scope

- FR-K12 (Yrs as a third source) -- captured in `requirements-walker.md`, not
  part of this delivery. FR-K11 only.
- Any change to `walrus-fmt`, `walx`, or the on-disk `.wal`/`.wbl` format.
- Writing/mutating Automerge documents -- Walker reads and plays history only.
- A full Automerge DAG-branch visualisation beyond listing history in index
  order; richer DAG rendering is a later question (design-note open question 3)
  and is NOT required to satisfy FR-K11.

## Assumptions

- The `automerge` crate's load/view/change-list API is available and stable as
  recorded in the design note (verified 2026-10-10). The design step will pin the
  exact version and confirm the current method names.
- The multi-file trait is the fixed contract; where the design note and the
  shipped trait disagree, the shipped trait wins (noted above).
- "A real Automerge TEXT document" fixture will be produced deterministically for
  tests (committed under `tests/fixtures/`); its construction is a design/
  implementation concern, not a requirement here.
