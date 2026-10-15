# Walrus + Walker read-only audit

Branch `main` at commit `58117df` ("refactor(walrus-fmt): remove legacy
WalrusFile shim"). Read-only audit; no source files were changed. The only file
written is this report.

Scope: the committed main tree under `crates/` (walrus / walrus-fmt, walx,
walrus-cli, wal-cli-common, walker) plus the root docs. The `.worktrees/`
checkouts (`walrus-core`, `walker-gui`) are SEPARATE working trees and are OUT
OF SCOPE -- several greps below hit them; those hits are explicitly excluded
from every finding.

---

## Executive summary

The 58117df refactor is clean at the code level. The main `crates/` tree has
ZERO references to the removed legacy symbols (`WalrusFile`, `new_wal`,
`read_container`, `write_container`, `reconstruct_base`, `reconstruct_head`) --
every remaining hit is in `.worktrees/` (out of scope). The library builds and
clippies clean (default and `--all-features`), and the GUI-free dependency
boundary HOLDS. Library code contains no `unwrap`/`expect`/`panic!` outside
`#[cfg(test)]`.

Counts per section:

1. Stale patterns / legacy residue: 3 findings, all DOCUMENTATION (code clean).
2. Dead code / unused surface: 2 findings (both already-justified `allow`s), 0 defects.
3. GUI paths lacking test coverage (walker): 1 systemic gap + a per-method table. HIGHEST PRIORITY.
4. Efficiency: 2 findings (1 real per-frame re-reconstruct; 1 minor).
5. Correctness: 0 defects found; several edge cases confirmed SOUND. 1 spec note (gzip error type).
6. NFR-1 (400-line cap): 3 files over the cap.

Top 5 highest-priority items across all categories:

1. [High] No test ever drives the real `WalkerApp` shell (`update`/`ui`,
   `open_walrus`, `refresh_bytes`, `bind_player`, export/import/mutate wiring).
   Every "harness" test rebuilds a logic slice in a standalone closure that
   MIRRORS app.rs instead of exercising it -- the exact shape of the bug that
   shipped green last session. (Section 3)
2. [High] Stale ROADMAP.md: it still states walker is NOT migrated and step 9 is
   "OUTSTANDING", but 5726a00 migrated walker and 58117df removed the shim. A
   future session reading ROADMAP will act on false state. (Section 1)
3. [Medium] `WalkerApp::ui` re-reconstructs the viewed document from the archive
   EVERY FRAME via `view::render` -> `present::render` -> `source.reconstruct`,
   while the cached `current_bytes` is used only on the fallback arm. Wasteful
   during playback/repaint. (Section 4)
4. [Medium] README.md library example still imports and calls the removed
   `WalrusFile` type (`use walrus_fmt::{WalrusFile, Version}; WalrusFile::open`).
   The public type is now `Archive`. (Section 1)
5. [Medium] NFR-1 breaches: `crates/walrus/src/archive.rs` (~440 non-test
   lines), `crates/walker/src/source/mod.rs` (~472, inflated by a non-`cfg(test)`
   `testsupport` module), `crates/walker/src/app.rs` (~535). (Section 6)

Checks run (all via `.gate/*.log` with EXIT sentinels, read back):
- `cargo check -p walrus-fmt` -> clean, EXIT=0, no warnings.
- `cargo build -p walrus-fmt --all-features` -> clean, EXIT=0, no warnings.
- `cargo clippy -p walrus-fmt --all-targets --all-features` -> clean, EXIT=0.
- `cargo tree -p walrus-fmt` -> dependency boundary confirmed (see Section 1).
Checks NOT run (to respect "no build-heavy full-workspace gate"): a full
`cargo build`/`cargo test` across walker + both CLIs, and `cargo clippy`
on walker/walx/walrus-cli. Walker findings below are from static reading, not a
walker compile. The user's own transcript shows a full `cargo gate --build` ran
clean (530/530) at this commit, so the workspace does compile and pass.

---

## 1. Stale patterns / legacy residue

### 1.1 Code: CLEAN -- no removed-symbol residue in the main tree

`grep` for the removed symbols across `crates/**/*.rs`:

```
WalrusFile|new_wal|read_container|write_container|reconstruct_base|reconstruct_head
-> No matches found.
```

(The same grep across the whole workspace matches only `.worktrees/walrus-core`
and `.worktrees/walker-gui`, which are out of scope.)

`crates/walrus/src/lib.rs` exports only the new surface -- `Archive`, `FileId`,
`Version`, `Direction`, `WchgHeader`, the manifest types, `ChangeRecord`/`Op`,
and the feature-gated `diff` -- with no `WalrusFile` re-export (lib.rs lines
45-54). `reconstruct.rs` exposes only `reconstruct_directional` (the per-file,
direction-aware engine) and the shared `apply_record`/`apply_splice`
primitives; the old forward-only `reconstruct(base, log, index)` wrapper is
gone (`crates/walrus/src/reconstruct.rs:18-31`).

### 1.2 INTENTIONAL seam confirmed: `standalone.rs` (FR-W9)

`crates/walrus/src/standalone.rs:31-40` carries the deliberate future-embedder
seam, and its doc still reads correctly:

```rust
// FR-W9 exposes the standalone replay path as crate-internal API, exercised by
// the tests below; no non-test caller consumes it yet (a future standalone
// `.wchg` reader / an FFWB embedder will), so the non-test build would flag it
// as dead. The allow keeps the deliberate seam without widening the public API.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn reconstruct_standalone( ... )
```

This is INTENTIONAL, not a defect; the doc still describes it as a future-
embedder seam. No action.

### 1.3 "legacy" / "single-file" / "shim" grep -- all benign

Every remaining hit for `legacy|single-document|single-file|shim|old model` in
the main tree is legitimate:
- `crates/walrus/src/encoding.rs:70`, `mutate_i18n.rs:148-160` -- "legacy
  encoding" is `encoding_rs` domain terminology (GBK etc.), not Walrus legacy.
- `crates/walrus-cli/src/cmd_alias.rs`, `alias_shim.rs`, `tests/alias.rs` --
  "shim" is the PATH alias-install feature (a `walrus`->binary shim), a distinct,
  live feature; not format legacy.
- `crates/walker/tests/harness.rs:155-193` -- a test that ASSERTS the stale
  single-file transport labels are GONE; keeping the word is correct.
- `crates/walrus/src/archive.rs:449`, `mutate.rs:193`, `manifest.rs:252` --
  "single-file archive" means an archive tracking ONE file; correct usage.
- `crates/walker/src/app.rs:216-217`, `source/walrus.rs:539-541` -- comments
  explaining that the status-bar variant is read from the open source, NOT by
  "re-opening with the legacy single-file reader"; accurate historical context.
- `crates/walrus/src/reconstruct.rs`, `examples/generate_walk.rs:20-23`,
  `tests/deep_history.rs:3` -- doc context explaining the index flip vs the OLD
  convention; accurate and useful.

No TODO/FIXME/HACK/XXX markers were found in `crates/**/*.rs`.

### 1.4 [Medium] ROADMAP.md is STALE about walker

`ROADMAP.md` top-of-file status and step 9 both still claim walker is NOT
migrated:

> WALKER (the GUI frontend) is the one dependent piece NOT yet migrated: it
> still opens archives via the legacy single-file `WalrusFile::open` and so
> fails on the new multi-file fixtures (`MissingMember("base")`).

and

> 9. OUTSTANDING (separate walker stream) -- WALKER full frontend: migrate off
>    the legacy `WalrusFile::open` to the `Archive` API (clears the known-red
>    `MissingMember("base")` integration test) ...

But the git log shows this is DONE on `main`:
- `5726a00 fix(walker): open multi-file archives via WalrusSource, not legacy WalrusFile`
- `58117df refactor(walrus-fmt): remove legacy WalrusFile shim`

and `crates/walker/src/app.rs:219` opens via `WalrusSource::open(&path)` (the
`Archive` API), with `crates/walker/tests/integration.rs:32` driving
`Archive::open`. The whole "HISTORICAL ... the one place still on the legacy
`WalrusFile::open`" paragraph (ROADMAP bottom) is also now false. Rationale:
ROADMAP is the hand-off state for the next session; leaving it claiming the
central task is still open invites wasted/duplicated work. Fix: move step 9 to
Done and update the status banner. Effort: ~15 min docs.

### 1.5 [Medium] README.md library example names the removed `WalrusFile`

`README.md` "Using the library" shows:

```rust
use walrus_fmt::{WalrusFile, Version};
let archive = WalrusFile::open("report.wbl")?;
...
let current = archive.reconstruct("report.md", Version::Index(0))?;
```

`WalrusFile` no longer exists; the type is `Archive` and `reconstruct` takes a
`&FileId` + `Version`, not a `&str`. The example is captioned "API names are
illustrative", which softens but does not excuse naming a deleted type in the
first code a prospective embedder reads. Fix: rewrite against `Archive` /
`FileId` (match `crates/walker/tests/integration.rs:32-66`, which is the real,
compiling usage). Effort: ~15 min docs.

---

## 2. Dead code / unused surface

`cargo clippy -p walrus-fmt --all-targets --all-features` is CLEAN (EXIT=0, no
warnings), so the compiler sees no unexpected dead code. The two explicit
`#[allow(dead_code)]`s in library code are both already justified:

### 2.1 `rebase.rs::trim_to_versions` -- justified, mild redundancy

`crates/walrus/src/rebase.rs:140-155`:

```rust
// ... This typed entry is retained and unit-tested as the version-count-facing
// form; its only caller today is the pure-core test, hence the scoped allow.
#[allow(dead_code)]
pub(crate) fn trim_to_versions( ... ) {
    if versions == 0 { return Err(... Rebase ...); }
    trim_to(base, records, direction, versions - 1)
}
```

It is a thin wrapper over `trim_to` adding a zero-versions guard that the
Archive layer handles elsewhere. Genuinely only test-called. Not a defect (the
allow + comment are honest), but it is dead production surface: either promote
the Archive layer to call it (so the zero-check lives in one place) or drop it
and keep the guard test against `trim_to` directly. [Low]

### 2.2 `player.rs::is_empty` -- deliberate API-pair surface

`crates/walker/src/player.rs:76-79`:

```rust
// Companion to `len()`; part of the public Player API, not yet called by the
// GUI transport. Kept so embedders have the standard len/is_empty pair.
#[allow(dead_code)]
pub fn is_empty(&self) -> bool { self.len == 0 }
```

Intentional public-API completeness. Not a defect. No action.

### 2.3 walrus-fmt public surface with no in-repo caller -- INTENTIONAL

`walrus-fmt` is meant to be embeddable (FFWB CR-NR-107). Public items such as
`Archive::new`, `Archive::to_bytes`, `Archive::write`, and the standalone seam
exist for embedders and example generators even where no in-repo binary calls
them. These are deliberate library surface, NOT dead code. Called out so a
future pass does not "prune" them.

---

## 3. GUI paths lacking test coverage (walker) -- PRIORITY

### 3.1 [HIGH] Systemic gap: the real `WalkerApp` shell is never driven

This is the same failure mode that let a GUI bug ship green last session. No
test constructs a `WalkerApp` and drives it through an `egui_kittest` `Harness`.
Evidence:

- `grep` for `WalkerApp` / `Harness::new` across `crates/walker/tests/**`: every
  harness in `tests/` is `Harness::new_ui(...)` / `Harness::new_ui_state(...)`
  over a HAND-WRITTEN closure that reconstructs a slice of app logic
  (`transport::controls`, `navigator::panels`, `history_view::history_panel`,
  `view::render`). None instantiates `WalkerApp` or calls its `update`/`ui`.
- `crates/walker/tests/automerge_harness.rs:74-77` states this outright: the
  test "mirrors the app-shell open path (`app.rs::open_file`) exactly" -- i.e.
  it re-implements the shell's sizing/open logic in the test instead of running
  the shell. A divergence between that mirror and the real `open_walrus` would
  not be caught.
- The only tests that touch `WalkerApp` are the two in-crate unit tests in
  `crates/walker/src/app.rs:536-620`: `an_automerge_sniffed_file_reports_not_
  built_in_when_the_feature_is_off` and `a_non_automerge_file_routes_to_the_
  walrus_arm`. Both call `app.open_file(path)` and assert on `app.status` /
  `app.read_source` / `app.player`. They cover the SNIFF ROUTING in
  `open_file`, nothing downstream.
- `crates/walker/Cargo.toml` dev-deps note: "the eframe feature is not needed
  for the transport-control harness tests". So `egui_kittest` is pulled WITHOUT
  its eframe integration, which is exactly what a `Harness` driving an
  `eframe::App` (`WalkerApp`) would require. The capability to test the shell
  headlessly is deliberately not enabled.

Note on feasibility: this build's `eframe::App` (confirmed against
`docs.rs/eframe/0.36.0`) uses `fn ui(&mut self, ui, frame)` as the required
method with `fn logic(...)` provided -- `crates/walker/src/app.rs:362-385`
matches that. Driving `WalkerApp` headlessly is therefore feasible by enabling
`egui_kittest`'s eframe integration (or by calling `WalkerApp::new(..)` and
invoking `ui`/`logic` against a harness `Ui`/`Context`). This is a real,
closable coverage gap, NOT a manual-only path.

### 3.2 Per-method coverage of the `WalkerApp` shell (`crates/walker/src/app.rs`)

| Method | Lines | Covered by an automated test? | Driveable headless? |
|--------|-------|-------------------------------|---------------------|
| `open_file` (sniff routing) | 139-156 | YES -- unit tests app.rs:544-619 (feature-off automerge + junk-walrus arms) | n/a (covered) |
| `open_walrus` | 191-227 | NO directly. `WalrusSource::open` is covered (source/walrus.rs tests); the SHELL method that also builds the write handle, sets `source_kind`, binds the player, and sets status is not run by any test. The automerge_harness only MIRRORS it. | YES -- open a temp fixture, assert `player`/`source_kind`/`status`/`current_bytes`. REAL GAP. |
| `open_automerge` (feature-on) | 170-188 | NO (feature-off stub is tested; feature-on arm only via mirror) | YES with `--features automerge`. GAP. |
| `bind_player` | 229-245 | NO (mirrored in automerge_harness.rs:103-107, not run) | YES. GAP. |
| `refresh_bytes` | 253-271 | NO | YES -- after open + seek, assert `current_bytes` matches `source.reconstruct`. GAP. |
| `variant_str` | 274-281 | NO (status_bar formatting tested via chrome, but not this mapping on a real open) | YES. Low risk (pure match). |
| `prompt_open` | 287-299 | NO -- opens a real `rfd::FileDialog` | NO -- genuinely MANUAL (OS-native rfd dialog). Correctly manual. |
| `source` accessor | 305-307 | NO (trivial) | n/a trivial. |
| `show_history_panel` | 315-331 | PARTIAL -- `history_view::history_panel` is harness-tested (history_view.rs); the SHELL wiring (clone guard, right-panel show, save on export intent) is not. The `rfd` save inside is manual. | Panel wiring YES; the save dialog MANUAL. GAP on the wiring. |
| `export_context` | 339-345 | NO (`export::offered_formats` + `export_ui` covered via integration.rs, but not this shell assembly) | YES. GAP. |
| `logic` (tick/repaint/refresh) | 362-384 | NO -- `player.tick` is covered in player.rs unit tests; the shell's "version_changed -> refresh_bytes" wiring is not | YES -- advance time, assert refresh happened. GAP. |
| `ui` (full egui render path) | 385-534 | NO -- the whole menu/navigator/status/central render is never run as a unit | YES -- the biggest, highest-value headless target. GAP. |
| import wiring (`apply_import`) | app_mutate.rs | Import RESOLUTION + orchestration covered (import.rs, walrus_import tests); the SHELL `apply_import` + refresh not driven | YES. GAP. |
| mutate wiring (`handle_file_action`) | app_mutate.rs | FileAction emission covered (file_controls_ui.rs); shell dispatch + transactional write + refresh not driven as a unit | YES. GAP. |

### 3.3 Highest-risk untested paths (ordered)

1. `open_walrus` + `bind_player` + `refresh_bytes` round-trip through the real
   shell. This is precisely where last session's bug lived (the shell open path,
   not `WalrusSource` which was always tested directly). A headless test that
   opens a real multi-file `.wal` fixture through `WalkerApp` and asserts the
   first file is selected, the player is sized to `count`, and `current_bytes`
   equals `source.reconstruct(file, Index(0))` would have caught it.
2. The `ui` central-panel render selecting the right viewer for the active file
   and surviving an empty/zero-version archive without panicking.
3. `handle_file_action` / `apply_import` writing transactionally and refreshing
   -- these touch disk via `Archive::write`; drive with a `TempDir` fixture.

Genuinely MANUAL (reserve, per testing steering): `prompt_open` and the `rfd`
SAVE inside `show_history_panel` (OS-native dialogs); real OS window / GPU
paths. Everything else above is headless-driveable and is a real gap.

---

## 4. Efficiency

### 4.1 [Medium] Walker re-reconstructs the viewed document every frame

`crates/walker/src/app.rs:510-523` (central panel) calls, on EVERY `ui` pass:

```rust
(Some(source), Some(active), Some(version)) => {
    view::render(ui, source, active, version);
}
```

`view::render` -> `crates/walker/src/view/present.rs:40-50`:

```rust
pub fn render(ui, source, file, version) {
    let renderer = select_renderer_for(source, file);
    match source.reconstruct(file, version) {   // <-- full reconstruct every call
        Ok(bytes) => view::paint(ui, &renderer, &bytes),
        ...
```

`WalrusSource::reconstruct` (`crates/walker/src/source/walrus.rs:150-170`) has
NO cache: each call replays base+splices through `Archive::reconstruct`.
Meanwhile the shell already maintains a cached `current_bytes` refreshed only on
change (`refresh_bytes`, app.rs:253-271), but that cache is used ONLY on the
fallback arm (`_ => view::paint(ui, &self.renderer, &self.current_bytes)`,
app.rs:525-528), never on the primary path. So whenever a source+selection
exists, the cache is dead and the archive is re-replayed per repaint. During
playback the shell calls `ctx.request_repaint()` (app.rs:380), driving
continuous repaints and therefore continuous reconstruction.

Impact scales with document size and history depth; for the ASCII-walrus demo it
is negligible, for a large tracked file with a deep log it is not. Fix options:
(a) paint from the cached `current_bytes` on the primary arm too (reconstruct
only in `refresh_bytes`, i.e. on open/seek/version change), or (b) add a small
`(FileRef, VersionId) -> Vec<u8>` memo in `WalrusSource`/the shell. Option (a)
matches the existing cache design and is the smaller change. Effort: ~1-2 h incl.
a headless regression test asserting reconstruct is not called on an unchanged
frame. Respect INV-2: keep any cache in walker, not in `walrus-fmt`.

### 4.2 [Low] `file_info` reconstructs the current version for a size figure

`crates/walker/src/source/walrus.rs:181-199` computes `size_bytes` by calling
`self.reconstruct(f, &VersionId::Index(0))` inside `file_info`. The navigator
calls `file_info` while listing files; if that listing runs per frame this is a
full current-version reconstruct per file per frame. Lower severity than 4.1
(the navigator may not repaint continuously), but the same memo/cache from 4.1
would remove it. [Low]

### 4.3 Library hot loop (`reconstruct.rs`) -- CLEAN

`apply_first_splices` / `apply_record` / `apply_splice`
(`crates/walrus/src/reconstruct.rs:33-98`) do a single `base.to_vec()` to seed
the scratch doc, then `Vec::splice` in place per record -- no per-record
cloning. `invert.rs::invert_forward_splices` seeds one scratch `version.to_vec()`
and advances through the single `apply_record` path, capturing deleted bytes by
borrow (`&scratch[*at..end]`) -- no uncommented hot-loop clone. `rebase.rs`'s
`trim_to` clones `base`/`records` once at the trim boundary (per-trim, not
per-splice), which is acceptable. No O(n^2) version-addressing patterns were
found: `version::resolve_file` is O(records) and reconstruction is a single
linear replay. The Rust-steering "comment any clone in a hot loop" rule is
satisfied -- no uncommented hot-loop clones observed.

---

## 5. Correctness

### 5.1 No library `unwrap`/`expect`/`panic!` outside tests -- CONFIRMED

`grep` for `unwrap()|expect(|panic!|unreachable!|todo!|unimplemented!` across
`crates/walrus/src/**` returns matches ONLY inside `#[cfg(test)]` modules
(encoding.rs, manifest.rs, mutate_remove.rs, wchg.rs, diff.rs, invert.rs,
version.rs tests). Production paths return typed `WalrusError`. Steering
satisfied.

### 5.2 Splice bounds / overflow -- SOUND

`apply_splice` (`crates/walrus/src/reconstruct.rs:84-97`) uses
`at.checked_add(delete_len)` and returns `WalrusError::SpliceOutOfBounds` on
overflow or `end > doc.len()`, before any `doc.splice`. `invert.rs:52-74` does
the identical checked-add + bound check and additionally validates the captured
deleted bytes are UTF-8, returning a typed error (never a panic) if not. No raw
indexing on untrusted offsets in production.

### 5.3 `let _ =` discards -- both justified

Only two in `crates/walrus/src/**`: `atomic.rs:66` (`let _ = fs::remove_file(&temp_path);`
with an inline "cleanup only; original is intact" comment) and one in a test.
Steering ("never silently discard without a comment") satisfied.

### 5.4 Empty / zero-version / single-version edges -- SOUND

- Empty archive: `Archive::read` accepts zero tracked files; `manifest.rs` round-
  trips a zero-files manifest (`manifest_round_trips_with_zero_files` test).
- Zero-record file (`count == 0`, only index 0): `reconstruct_directional`
  computes `splices_to_apply = count.saturating_sub(index)` (forward) or `index`
  (reverse); with `count == 0` and `index == 0` it applies 0 splices and returns
  the base. `fold_oldest` on a zero-record file is a documented benign no-op
  (`rebase.rs:46-54`, test `fold_oldest_on_a_zero_record_file_is_a_noop...`).
- `trim_to_versions(.., 0)` is a TYPED error (`WalrusError::Rebase`), not a panic
  (`rebase.rs:145-150`).

### 5.5 Variant symmetry (FORMAT.md section 5, INV-3) -- ASSERTED

Byte-identical reconstruction across directions for the SAME index is directly
tested over the whole range, not just endpoints:
`crates/walrus/src/archive.rs` `forward_and_reverse_reconstruct_identically_for_
every_index` loops `for k in 0..=count` asserting `fwd == rev`;
`crates/walrus/src/invert.rs` `invert_forward_history_reconstructs_byte_
identically_at_every_index` does the same via the production inversion helper;
`crates/walrus/tests/deep_history.rs` extends this to a few-hundred-splice
history. This satisfies the testing-steering requirement to assert the
byte-identical invariant directly.

### 5.6 [Low] gzip rejection uses a generic error type (spec note)

`crates/walrus/src/container.rs:49-52`: an identified-but-unsupported gzip
container returns `WalrusError::Manifest("gzip containers are not yet
supported...")`. FORMAT.md section 2 says a reader "returns a clear 'not
supported by this build' error" for gzip -- which this does -- but overloading
`Manifest` (a manifest-parse error) for a codec-support condition is slightly
misleading for callers matching on error kind. Consider a dedicated
`WalrusError::UnsupportedCodec`/`CodecNotBuilt` variant. No behavioural bug;
purely error-taxonomy clarity. [Low]

### 5.7 Code vs FORMAT.md -- no divergence found

Index-0-is-current, per-file `direction` authority over the archive variant,
`f<N>` id scheme, LF-only `.wchg` framing with CRLF rejection, no-trim of record
content, and the splice-is-the-single-primitive rule are all implemented as the
spec states (archive.rs, container.rs `parse_wchg`, record.rs). The mixed-
direction archive case (a forward and a reverse file under one variant) is
implemented and tested (`reconstruct_in_a_mixed_archive_uses_each_files_own_
direction`), matching FORMAT.md's "a mixed-direction archive is legal".

---

## 6. Improvement recommendations

Separated into correctness/safety fixes and nice-to-have enhancements, each with
file/area, rationale, and rough effort. Priorities: High / Medium / Low.

### Correctness / safety fixes

- [HIGH] Add a headless `egui_kittest` test that drives the REAL `WalkerApp`
  through an open -> select -> seek -> render cycle over a temp multi-file `.wal`
  fixture, asserting `player` size, active selection, and `current_bytes ==
  source.reconstruct(file, Index(0))`. Area: `crates/walker/tests/` (new file,
  e.g. `app_shell.rs`) + enabling `egui_kittest`'s eframe integration in
  `Cargo.toml` dev-deps. Rationale: closes the exact class of bug that shipped
  green last session (Section 3.1). Effort: ~0.5-1 day (harness wiring is the
  cost; the assertions are small). Respect INV-2 (test lives in walker only).

- [MEDIUM] Also drive `handle_file_action` and `apply_import` through the shell
  against a `TempDir` fixture (they call `Archive::write` transactionally).
  Area: same new test file. Rationale: the mutate/import SHELL wiring (not just
  the orchestration seams) is untested. Effort: ~0.5 day.

### Nice-to-have enhancements

- [MEDIUM] Paint the central viewer from the cached `current_bytes` on the
  primary arm (reconstruct only in `refresh_bytes`), or add a per-(file,version)
  memo in `WalrusSource`. Area: `crates/walker/src/app.rs:510-528`,
  `crates/walker/src/view/present.rs`, `crates/walker/src/source/walrus.rs`.
  Rationale: removes per-frame re-reconstruction during playback (Section 4.1).
  Effort: ~1-2 h + a regression test. INV-2: keep the cache in walker.

- [MEDIUM] Fix ROADMAP.md (move walker step 9 to Done; correct the status banner
  and the "HISTORICAL ... still on legacy WalrusFile::open" line). Area:
  `ROADMAP.md`. Rationale: it misstates the central completed task (Section 1.4).
  Effort: ~15 min.

- [MEDIUM] Rewrite the README library example against `Archive` / `FileId`
  (mirror `crates/walker/tests/integration.rs`). Area: `README.md`. Rationale:
  it names the removed `WalrusFile` type (Section 1.5). Effort: ~15 min.

- [MEDIUM] Split the three NFR-1 breaches:
  - `crates/walrus/src/archive.rs` (~440 non-test lines; `#[cfg(test)]` begins
    at line 442). Candidate split: move the `read_members` / `read_tracked_file`
    / `wchg_member_name` / `bound_tail_bytes` container-collection helpers
    (archive.rs ~310-430) into a `archive_read.rs` sibling.
  - `crates/walker/src/source/mod.rs` (~472 non-test lines; `#[cfg(test)]` begins
    at line 473). The bulk is the `pub mod testsupport` InMemorySource (lines
    ~287-471) which is NOT `#[cfg(test)]`-gated and so counts against NFR-1 even
    though it is test support. Gate it behind `#[cfg(any(test, feature =
    "testsupport"))]` or move it to its own `source/testsupport.rs`; that alone
    brings the file comfortably under 400.
  - `crates/walker/src/app.rs` (~535 non-test lines; `#[cfg(test)]` begins at
    line 536). It already offloads to `app_mutate.rs`/`app_view.rs`; the `ui`
    method (app.rs:385-534) is a further split candidate (e.g. a `app_render.rs`
    holding the menu/navigator/status/central panel builders).
  Rationale: NFR-1 keeps files single-responsibility and AI-edit-reliable.
  Effort: ~1-2 h each, pure refactor (no behaviour change), verified by the
  existing scoped tests.

- [LOW] Resolve `rebase.rs::trim_to_versions` dead surface (Section 2.1): either
  route the Archive generations path through it or delete it and retest against
  `trim_to`. Effort: ~30 min.

- [LOW] Give gzip-not-supported its own `WalrusError` variant instead of
  overloading `Manifest` (Section 5.6). Area: `crates/walrus/src/error.rs`,
  `container.rs`. Effort: ~30 min. Note: a new public enum variant is an API
  change -- `WalrusError` should be `#[non_exhaustive]` (Rust steering) so this
  is additive.

### Explicitly NOT recommended

- Do NOT drop `.wbl` / reverse-log support -- it is a first-class variant and the
  symmetry is load-bearing (INV-3).
- Do NOT move export backends out from behind the `export-*` features or add GUI
  deps to `walrus-fmt` -- the dependency boundary is confirmed intact (below) and
  must stay so (INV-2).

---

## Appendix: dependency-boundary evidence

`cargo tree -p walrus-fmt` (default features) shows walrus-fmt pulls ONLY:
`serde`, `serde_json`, `tar`, `thiserror`, `zstd` (plus dev-deps `anyhow`,
`tempfile`). NONE of `eframe`, `egui`, `pulldown-cmark`, `scraper`, `csv`,
`printpdf`, `docx-rs` appear in the default graph -- they enter only via the
`export-*` features (the `--all-features` build compiles them, confirming they
exist but are gated). INV-2 holds. (Full log: `.gate/audit-tree-walrus.log`.)
