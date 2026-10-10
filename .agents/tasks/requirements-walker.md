# Walker stream requirements -- `walker` GUI frontend

Status: REQUIREMENTS (planning). Derived from `CLI-DESIGN.md` section 6b,
`FORMAT.md`, the gap report (`.agents/tasks/format-breaking-gap-report.md`), and
the multi-format design note (`.agents/tasks/history-source-automerge.md`). This
is the shared contract for the WALKER work stream (its own chat + the
`.worktrees/walker-gui` worktree, branch `feature/walker-gui`).

ASCII only (`->`, `<->`, `<=`, `>=`, `--`) per the documentation steering rule.

## 0. Scope and ownership

This stream owns `crates/walker` only. It CONSUMES the `walrus-fmt` public API
built by the WALRUS stream; it never modifies `walrus-fmt`, `walx`, or
`walrus-cli`.

Phasing (the user's directive):
- PHASE 1: make Walker a full GUI frontend for the NEW multi-file `.wal`/`.wbl`
  format. This is the whole first body of work.
- PHASE 2: add Automerge support (read Automerge documents and play their
  history), DEFERRED until Phase 1 is finished. Captured in section 5 so it is
  planned, not forgotten, but OUT OF SCOPE for the first delivery.

Hard invariants:
- INV-2 (boundary): all GUI/export dependencies live in `walker`; `walrus-fmt`
  stays GUI-free. Walker calls the library's read AND mutate API -- the same API
  the `walrus` CLI uses.
- Export (content transformation) is WALKER-EXCLUSIVE (CLI never transforms).
- Source rules: every `.rs` under 400 non-test lines (`view/richtext.rs` is the
  split seam); strict ASCII; walker is a binary (anyhow `.context`;
  `expect("why")` tolerable; tests use `expect`); TDD; headless `egui_kittest`
  for widget behaviour, MANUAL only for pixel-exact appearance and the `rfd`
  native dialog (each with a stated reason).

## 1. Dependency on the WALRUS stream (what is BLOCKED vs INDEPENDENT)

Walker's work splits by whether it needs the new `walrus-fmt` API. The contract
is section 1 of `requirements-walrus.md` (the `Archive`/`TrackedFile`/`Version`
surface). Walker MUST NOT guess that API: items marked BLOCKED wait until the
WALRUS stream reports the surface implemented.

INDEPENDENT (can proceed now, no new library API):
- The egui app shell, window, theming, chrome layout.
- The `HistorySource` trait seam (FR-K1) -- defined in `walker`, format-agnostic.
- The player state machine refactor's PURE parts (speed, loop/wrap -- already
  exist and are reusable; only index SEMANTICS change, see FR-K3).
- Navigator two-pane LAYOUT and widget scaffolding (populated once the API lands).
- The content renderers (`view/`) and export backends already shipped -- they
  operate on reconstructed BYTES and survive the format change; they only
  re-point to the new per-file reconstruct call (BLOCKED on that call's shape).

BLOCKED on `walrus-fmt` (defer until the WALRUS stream signals ready):
- Anything calling `Archive::files/file/matching/reconstruct/log_bytes`.
- Import = add/update (`Archive::add/update`).
- Remove / condense / set-generations (`Archive::remove/condense/set_generations`).
- The bloat indicator (needs `log_bytes` + current-version byte length).
- Per-file player binding (needs per-file `reconstruct` + index size).

STRATEGY: build Phase 1 against the `HistorySource` trait (FR-K1) so the GUI is
coded to the trait, and the `WalrusSource` implementation is the single place
that touches the new `walrus-fmt` API. That isolates the blocked surface to one
adapter and lets the rest of the GUI progress.

## 2. Functional requirements -- PHASE 1 (new `.wal` multi-file frontend)

### FR-K1 HistorySource abstraction (format-agnostic seam) -- INDEPENDENT
Introduce a `HistorySource` trait in `walker` that the transport, player,
renderers, and export consume, so they never learn the concrete format. Ship
`WalrusSource` as the only Phase-1 implementation. (This is the seam that keeps
Phase 2 / Automerge cheap; see section 5.)
```
enum VersionId { Index(usize), /* Heads(..) added in Phase 2 */ }
struct VersionMeta { label: String, timestamp: Option<i64>, author: Option<String> }
struct FileInfo { index_size: usize, generations: Option<u32>, size_bytes: u64, log_bytes: u64, mime: String }
trait HistorySource {
    fn files(&self) -> Vec<FileRef>;                 // multi-file navigator
    fn versions(&self, f: &FileRef) -> Vec<VersionId>;
    fn reconstruct(&self, f: &FileRef, v: &VersionId) -> Result<Vec<u8>>;
    fn metadata(&self, f: &FileRef, v: &VersionId) -> VersionMeta;
    fn content_hint(&self, f: &FileRef) -> (String /*mime*/, String /*name*/);
    fn file_info(&self, f: &FileRef) -> FileInfo;    // navigator listing + FR-K7 bloat
}
```
- AC1 The trait and `VersionId`/`VersionMeta` compile and are unit-tested with a
  fake in-memory source (no `walrus-fmt` needed for the test).
- AC2 Transport/player/renderers reference ONLY the trait, not `walrus_fmt` types
  directly (verified by grep: no `walrus_fmt::Archive` outside `WalrusSource`).
- NOTE (added by FR-K4, Option A): `file_info`/`FileInfo` were added to this
  contract deliberately. `FileInfo` carries index size, generation cap, current
  size, and a change-log byte figure so BOTH the FR-K4 navigator listing AND the
  FR-K7 bloat ratio `R = log_bytes / size_bytes` compute from the trait alone.
  It holds NO `walrus_fmt` types, so AC2 still holds. This was purely additive --
  no existing method signature, `FileRef`, `VersionId`, or `VersionMeta` changed.

### FR-K2 WalrusSource adapter -- BLOCKED on walrus-fmt section 1
Implement `HistorySource` over the new `Archive` API: `files()` from
`archive.files()`, `reconstruct()` from `archive.reconstruct(file, Version)`,
`metadata()` from the per-file log/manifest, `content_hint()` from the file's
mime/name.
- AC1 Opens a multi-file `.wal` and a `.wbl` fixture and lists all tracked files.
- AC2 `reconstruct(file, Index(0))` is the current version for both variants.

### FR-K3 Index-flip transport semantics -- partially BLOCKED
Index 0 is now CURRENT; higher index is OLDER. The transport labels and
step/seek direction must reflect this: "step back in time" INCREASES the index.
The pure loop/wrap/speed state machine is reusable; only the index-direction
mapping and the labels change.
- AC1 Headless `egui_kittest`: a "step older" action increases the model index;
  "step newer" decreases it; seeking to 0 shows current.
- AC2 Speed control (0.25x..4x presets + fine slider over 400 ms base) drives the
  model interval (existing pure `interval_for_multiplier` reused).
- AC3 Transport button/labels read correctly under index-0-is-current (no
  "forward = toward head = newer" leftover from the single-file model).

### FR-K4 Two-pane navigator -- BLOCKED on walrus-fmt section 1
External pane (filesystem browser for import) + internal pane (the archive's
tracked files, the GUI form of `walrus query -l`): per file show name,
attributes, index size, generation cap, and the bloat indicator (FR-K7).
Multi-select for batch export/remove/condense.
- AC1 Internal pane lists every tracked file with its index size + generation cap.
- AC2 Selecting a file binds the player/viewer to that file's version timeline.
- AC3 Multi-select enables batch actions (export/remove/condense).

### FR-K5 Import = add/update (resolved automatically) -- BLOCKED
One "Import" action. New name -> add (no prompt); existing name -> default to
update with a surfaced prompt ("<name> exists -- import as a new version?") and
an "add under a different name" alternative. Goes through the library's
transactional mutate path.
- AC1 Importing a new name adds a tracked file; importing an existing name
  prompts and, on confirm, appends a new version.
- AC2 Verified headlessly (model state), with the native file dialog left MANUAL
  (stated reason: `rfd` OS dialog not headless-drivable).

### FR-K6 Per-file + batch controls -- BLOCKED
Context menu / details panel: set/change generation cap, condense now (with
`--keep N` field), remove, export selected version. Batch (multi-select):
export, remove (same multi-match confirmation the CLI `--force` provides),
condense. All via the library transactional path.
- AC1 Each control calls the matching `Archive` mutate op and reflects the result.
- AC2 Batch remove of multiple files requires an explicit confirmation.

### FR-K7 Bloat indicator -- BLOCKED on `log_bytes`
Color-code each internal row by bloat ratio `R = log_bytes / current_bytes` with
an index-count floor:
- GREEN `R < 2.0`; AMBER `2.0 <= R < 5.0`; RED `R >= 5.0`.
- Index-count floor: stay GREEN while index count `< 50` regardless of ratio.
- Advisory only (never auto-condenses); tooltip shows index count, log bytes,
  ratio. Thresholds user-configurable (these are starting defaults).
- AC1 Rows map to the correct band for fixtures spanning each threshold incl. the
  <50-version floor case.
- AC2 A `generations=N` (bounded) file reflects within-window churn and does not
  contradict the indicator.

### FR-K8 Content viewers re-pointed -- mostly INDEPENDENT (bytes), BLOCKED on call shape
The existing `view/` renderers (text/code/markdown/html/csv/richtext,
untrusted-safe: scripts dropped, images placeholders, no remote fetch) render the
bytes from `HistorySource::reconstruct`. `select_renderer(mime, name)` drives
choice from `content_hint`.
- AC1 Each renderer renders the reconstructed bytes of the selected file+version.
- AC2 Untrusted-content guarantees preserved (existing tests still pass).

### FR-K9 `.wchg` history viewer + Markdown report export -- BLOCKED
A per-file history view listing the change records (index, timestamp, note,
splice summary), exportable as a Markdown change report.
- AC1 Shows the selected file's records in index order with metadata.
- AC2 Exports a Markdown report of the history (deterministic content asserted).

### FR-K10 Export is Walker-exclusive -- INDEPENDENT (already shipped)
The `export/` backends (csv/docx/html/json/markdown/pdf) stay Walker-side (or
behind `walrus-fmt` optional features used ONLY by walker). Per-document export
menu from `offered_formats`, `rfd` save dialog, writes bytes.
- AC1 Export menu lists the offered formats for the selected file's type and
  writes each via the matching backend (existing tests carry forward).

## 3. Non-functional requirements
- NFR-1 Files under 400 non-test lines; new modules per concern
  (`navigator.rs`, `import.rs`, `history_view.rs`, `source/mod.rs`,
  `source/walrus.rs`).
- NFR-2 `cargo fmt -- --check` + `cargo clippy -p walker -- -D warnings` clean.
- NFR-3 Headless `egui_kittest` harness tests for navigator listing, import
  resolution, transport index direction, and bloat bands; MANUAL only for
  pixel-exact appearance and the `rfd` dialog (reason stated in a comment).
- NFR-4 Full gate is the owner's manual step; scoped `-p walker` is the inner
  loop.

## 4. Ordered delivery (Phase 1)
1. FR-K1 HistorySource seam (INDEPENDENT) + fake-source tests.
2. FR-K3 index-flip transport semantics (pure parts INDEPENDENT).
3. FR-K8/FR-K10 re-point renderers/export to the trait (byte path).
   -- then, once the WALRUS stream signals the API ready --
4. FR-K2 WalrusSource adapter.
5. FR-K4 navigator -> FR-K5 import -> FR-K6 controls -> FR-K7 bloat -> FR-K9
   history viewer.
Start the INDEPENDENT items immediately; gate the BLOCKED items on the WALRUS
stream's "API ready" signal (section 6 of `requirements-walrus.md`).

## 5. PHASE 2 (DEFERRED) -- Automerge support

OUT OF SCOPE until Phase 1 is finished. Captured so the Phase-1 seam (FR-K1) is
built to accommodate it. Full rationale and verified API basis are in
`.agents/tasks/history-source-automerge.md`.

### FR-K11 (Phase 2) AutomergeSource
Add `Heads(Vec<ChangeHash>)` to `VersionId` and implement `HistorySource` over
the `automerge` crate (MIT, Rust): `files()`/`versions()` from `get_changes()`
heads, `reconstruct()` from `view(heads)` serialised to bytes, `metadata()` from
change hash/message/time/actor, `content_hint()` detecting a single-`text` root
vs structured (JSON) docs.
- Sniff Automerge magic bytes `0x85 0x6F 0x4A 0x83` to pick the source.
- `automerge` is a Walker-side (feature-gated) dependency ONLY; never in
  `walrus-fmt` (INV-2).
- Caveat to handle: Automerge is a structured CRDT with a BRANCHING commit DAG;
  text docs map to the renderers, structured docs render as JSON/tree; the
  history view must represent a DAG, not just a linear index.
- AC (Phase 2): opens an Automerge text document, lists its history, reconstructs
  and renders a past version, and plays it through the same transport -- with
  Phase-1 `WalrusSource` behaviour unchanged.

### FR-K12 (Phase 2, optional) Yrs as a third source
`yrs` (MIT) behind the same `HistorySource` trait, if wanted later. No commitment.

## 6. Signal back to the WALRUS stream
If a Walker requirement needs an API shape not in `requirements-walrus.md`
section 1, raise it to the WALRUS stream rather than inventing it in `walker`;
the library is the single source of the mutate/read surface both frontends share.
