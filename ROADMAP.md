# Walrus roadmap / build state

This records the honest state of each piece so the next session can pick up
without re-discovering it.

STATUS (2026-10-13): the FORMAT-BREAKING single-file -> MULTI-FILE revision is
DONE for walrus-core and shipped on `main`. An archive holds many tracked files,
each with its own base, `.wchg` change log, and independent index range. The
authoritative spec is [`FORMAT.md`](FORMAT.md); design decisions are in
[`CLI-DESIGN.md`](CLI-DESIGN.md). The library (`walrus-fmt`), both CLIs (`walx`
extract-only, `walrus` full management), the shared `wal-cli-common`, the
regenerated examples, the `search` feature, and an exhaustive Python acceptance
suite + TCR are all on main. The historical gap report at
`.agents/tasks/format-breaking-gap-report.md` records the original file-by-file
audit; the sequence it drove is now complete (see "Format re-work sequence"
below, all steps DONE).

WALKER (the GUI frontend) is now MIGRATED and merged on `main`. It opens
archives via the `Archive` / `WalrusSource` API (commit `5726a00`), and the
legacy single-file `WalrusFile` shim was removed entirely in `58117df`; the old
`MissingMember("base")` integration failure is gone. Both streams (walrus-core
and walker) have converged on `main`; a full `cargo gate --build` runs clean.
The remaining walker work is the open items in the "Audit follow-ups" and
"Deferred / future ideas" sections below, not a migration.

## Going forward -- prioritized plan (updated 2026-10-16)

At `main` tip `31ecd20`, the headless `WalkerApp` shell coverage and import/mutate
fix are merged. The Tier A README corrections and `source::testsupport` sibling
module are implemented in the working tree. Scoped checks passed:
`cargo fmt -- --check`, `cargo test -p walker -j 1` (215 passed, 1 ignored),
`cargo build -p walker`, and `cargo clippy -p walker -- -D warnings`. The owner
also ran the full `cargo gate --build` on 2026-10-16; it completed CLEAN with
583/583 tests passed. Tier A is verified. walrus-core is feature-complete;
walker is migrated and merged; the search feature has shipped. What remains is
a short list of hygiene and OPTIONAL feature work -- NO known defects block a
release.

ENVIRONMENT NOTE (historical): an earlier whole-workspace `cargo test` on this
box crashed `rustc` with out-of-memory / `STATUS_STACK_BUFFER_OVERRUN`
(0xc0000409) while compiling heavier egui/eframe targets in parallel. The full
`cargo gate --build` has since completed cleanly using nextest: 583/583 tests
passed. If the older cargo-test invocation recurs, lower parallelism
(`cargo test -j 1` or `-j 2`) or test crate-by-crate.

The plan, highest value first. Tiers A-B close real debt and should land before
any new feature; tier C is optional/future and each item is its own worktree off
latest `main`.

### Tier A -- correctness coverage + stale docs

1. [DONE, merged `31ecd20`] Headless `WalkerApp` shell coverage drives the real
   app through open/select/seek/render and exercises import/mutate wiring,
   closing the coverage gap that allowed the legacy-reader bug to ship.
2. [DONE, full gate clean] The library examples now use `Archive` / `FileId`
   instead of the removed `WalrusFile` API in the top-level and crate READMEs;
   `examples/README.md` describes the current multi-file mutate API.

### Tier B -- hygiene / NFR (full gate clean)

3. [DONE, full gate clean] NFR-1 file splits: archive member readers now live in
   `crates/walrus/src/archive_read.rs`; Walker UI rendering lives in
   `crates/walker/src/app_render.rs`; and `source::testsupport` lives in its
   own feature-gated sibling. Both coordinators are below the 400 non-test-line
   cap.
4. [DONE, full gate clean] The Walker central panel paints the cached
   `current_bytes` instead of reconstructing on every frame. Selection changes
   refresh the cache, renderer selection remains source-derived, and
   reconstruction errors retain their inline error presentation. A headless
   regression test asserts rendering an unchanged frame makes no extra
   reconstruction calls.
5. [DONE, full gate clean] Gzip-not-supported has its own
   `WalrusError::UnsupportedCodec` variant. Archive trimming now uses the
   version-count `trim_to_versions` entry point, removing its dead-code
   exception while retaining its zero-version and no-op tests.

Scoped checks passed: `cargo fmt --all`, `cargo test -p walrus-fmt -p walker
-j 1`, `cargo clippy -p walrus-fmt -p walker --all-targets -- -D warnings`,
and `cargo build -p walrus-fmt -p walker`. The owner then ran the full
`cargo gate --build`, which completed CLEAN on 2026-10-16 with 585/585 tests
passed and 0 failed. Report: `.gate/gate.report.log`.

### Tier C -- OPTIONAL features (each its own worktree off latest `main`)

6. [IMPLEMENTED, scoped checks pass; full gate pending] ALL-VERSIONS search:
   both CLIs now expose `--all-versions`, search current through original for
   every selected file, and report matches grouped by version with timestamps.
   Specified in CLI-DESIGN section 5.8; combining the flag with a selector is
   RC 2.
7. [IMPLEMENTED; scoped validation in progress] OFFICE-CONTAINER (ODF / OOXML):
   on `feature/office-container` in a fresh worktree based on the then-current
   `main`, the spec, grouped ZIP member ingest, versioning, binary blob storage,
   byte-identical repack, opt-in CLI routing, and generated minimal ODF/OOXML
   fixtures have been implemented test-first. Scoped feature-boundary,
   structural storage/delta, and clippy/build checks plus the owner's full gate
   remain before merge.
   Walker integration is deliberately deferred to a separate phase.
8. WALKER git-history source and the Automerge/Yrs multi-format history player,
   both behind the source-agnostic `HistorySource` trait -- Walker-side /
   feature-gated only, never into `walrus-fmt` (invariant 2). Design notes under
   `.agents/tasks/walker-git-source/` and
   `.agents/tasks/history-source-automerge.md`.
9. OPTIONAL real-git-history `--at` fixture: only if authentic content/dates are
   specifically wanted; the synthetic `test_at_timeline.py` already proves the
   `--at` behaviour deterministically.

### Suggested next move

Tier A and B are full-gate verified. Tier C item 6 is implemented on its
feature branch; run the owner full gate before merging. Office-container is
being completed on its own feature branch; finish scoped validation and the
owner full gate before merging. Then choose another optional Tier C feature in a
fresh worktree based on the latest `main`.

## Design decisions locked (see CLI-DESIGN.md)

- MULTI-FILE archives: many tracked files per `.wal`/`.wbl`, each with its own
  base, `.wchg` log, and independent index range.
- Inner log is `.wchg` (one extension, both variants) with a self-describing
  header carrying `direction` (forward/reverse) and `generations`.
- INDEX 0 = CURRENT for both variants; higher index = older.
- GENERATIONS cap per file (0 = unlimited default; N = rolling window via
  re-base on the oldest).
- CLI tools: `walrus` (full management: query/extract/cat/add/update/remove/
  condense/convert) and `walx` (extract-only: extract + cat). POSIX/GNU conventions.
- `extract` RESTORES to disk; `cat` writes one file to stdout. Overwrite needs
  `--force`. Return-code table in CLI-DESIGN section 6.
- DIFF ENGINE: `similar` crate (dependency-free) behind a diff-to-splice adapter
  in `walrus-fmt`; `imara-diff` is the escalation path. Adaptive granularity
  (line vs word/char), uniform byte-splice output.
- BINARY files out of scope for now (reject on add); binary-delta is a future op.
- INTERNATIONAL: UTF-8 internal storage; non-UTF-8 sources transcoded via
  `encoding_rs` on add, original encoding recorded per file; grapheme-aware diff.
- Transactional writes (temp-file-then-atomic-rename) for all mutations.
- EXPORT (content transformation) is WALKER-ONLY; the CLI never transforms.
- WALKER becomes a full GUI frontend: two-pane file navigator, import=add/update,
  per-file generations, condense, bloat indicator, content viewers, and a `.wchg`
  history viewer (exportable as a Markdown change report).

## Two work streams

1. FORMAT RE-WORK (the breaking multi-file change). DONE on `main`. Library-first
   per the sequence below; all steps complete. Delivered across multiple worktrees
   merged to main (f<N> ids, index flip, container reader/writer, re-base/condense,
   diff + mutate API, convert, i18n, regenerated examples, both CLIs, CLI glob
   self-expansion).
2. VIEWERS + EXPORT. The content viewers (text/code/markdown/html/csv/richtext)
   and export backends (csv/docx/html/json/markdown/pdf) are WALKER-side per the
   export-is-Walker-only decision; they operate on reconstructed bytes and
   re-point to the new per-file reconstruct API. This lives on the walker
   branches and lands with the walker migration (separate stream).

## Format re-work sequence (library-first; TDD; keep walrus-fmt GUI-free)

ALL STEPS 1-8 DONE on `main` for walrus-core. Step 9 (walker frontend) is the
outstanding separate-stream item. Step 10 (docs) is ongoing -- this ROADMAP
update is part of it. Each step left the workspace buildable; development used
scoped `-p <crate>` checks with the full gate as the owner's manual step.

1. DONE -- Spec-anchored types: reshape `manifest.rs` (top-level single-doc fields ->
   per-file `files[]` entries with id/attributes/generations), add the
   `.wchg` header type + `Direction` enum; fix `base_index` semantics per variant.
2. DONE -- Container READER for the per-file layout (`files/<id>/base`,
   `files/<id>/<name>.wchg` with its header line); reshape `WalrusFile`/`Archive`
   to the per-file collection.
3. DONE -- INDEX FLIP in reconstruct + version addressing: `Index(0) == current`
   for both variants; byte-identical output across variants asserted (strict TDD;
   plus the ~300-version Rust deep-history test and ~50-version Python one).
4. DONE -- Container WRITER for the per-file layout + `.wchg` header +
   transactional temp-file-then-rename writes. (Also: short `f<N>` member ids so
   deep/long relative paths stay ustar-legal -- the long-path crash fix.)
5. DONE -- RE-BASE primitive, then `condense(keep)` and the automatic generations
   trim sharing it.
6. DONE -- DIFF (`similar` + adapter, adaptive granularity, grapheme-aware) + the
   mutate API (add/update/remove/set-generations) + `.wal <-> .wbl` convert +
   `encoding_rs` transcoding. All mutations transactional.
7. DONE -- Regenerated examples + fixtures (`generate_*.rs`, `examples/*.wal`);
   forward-then-backward byte-identical integration test re-established.
8. DONE -- CLI: `walx` re-scoped to extract-only (extract + cat, multi-file +
   mask, RC table); new `walrus` binary (query/add/update/remove/condense/
   convert) with POSIX/GNU conventions. `add`/`update` take `-R`/`--recurse`
   (CLI-DESIGN 5.4b, requirements FR-W11 AC7): a CLI-layer walk that expands each
   directory argument into the file batch, storing each file under its path
   relative to the recursion root (root name included) as the archive `name`.
   This reuses the existing `Archive::add`/`update` mutate API (step 6) -- no new
   library primitive -- so a recursed add + `walx extract` round-trips the tree.
9. DONE (merged to `main`) -- WALKER full frontend: migrated to the `Archive` /
   `WalrusSource` API (the legacy `WalrusFile::open` is gone; `58117df` removed
   the shim). Two-pane navigator, per-file position player binding,
   import=add/update, remove/condense/set-generations, bloat indicator, and the
   `.wchg` history viewer (Markdown report export) are all on main with headless
   `egui_kittest` tests for the widget-level logic. FR-K11 Automerge is behind a
   feature. KNOWN COVERAGE GAP (see "Audit follow-ups"): no test drives the real
   `WalkerApp` shell end to end -- the harness tests mirror app logic in
   standalone closures rather than running `WalkerApp::update`/`ui`. The
   git-source history idea is DEFERRED (see "Deferred / future ideas").
10. ONGOING -- Docs: keep README.md / crate READMEs aligned with the shipped
    reality. (FORMAT.md / CLI-DESIGN.md are the source of truth.) This ROADMAP
    status refresh is part of step 10.

## Walker frontend (step 9) -- per-FR honest state

Tracked in detail in `.agents/tasks/requirements-walker.md`; the load-bearing
cross-stream note:

- FR-K1 source seam, FR-K2 WalrusSource adapter, FR-K4 two-pane navigator: done.
- FR-K5 import = add/update (resolved automatically): DONE and LIVE. The
  FilePicker seam (real `rfd` + test fake), the pure `ImportResolution` state
  machine (add / update-prompt / add-as), the `WalrusSource` import
  orchestration via the `MutateArchive` seam, and the navigator/app wiring are
  all done and headless-tested GREEN. Byte-mutation is no longer gated: FR-W7's
  `walrus_fmt::Archive::add` is merged on this branch, so the real
  `MutateArchive` impl and `WalrusSource::import` are unconditional (the
  `import-mutate` feature was removed), and the on-disk AC1 +
  transactional-guarantee tests (`tests/import.rs::on_disk`) compile and PASS by
  default. Both create and append route through `Archive::add` (a tracked-name
  `add` records a save), so the `diff`-gated `Archive::update(bytes)` is not
  needed. The only MANUAL carve-out remains the real `rfd` pick. The write path
  is deliberately CONCRETE (off the read-only `HistorySource` trait).

## Walrus-core follow-ups still open (small)

- `--at` timestamp addressing: COVERED by `tests/acceptance/walrus_cli/
  test_at_timeline.py` (multi-file archive with known per-version mtimes set via
  `os.utime`, asserts `extract --at <T>` resolves each file to the newest change
  record at-or-before T; before-first-record -> current). The deep-history test's
  mid-history `--at` skip is thus closed by a sibling test. OPTIONAL still: a
  fixture built from the top-level `.md` files' REAL git history (authentic
  content/dates) -- only if authentic data is specifically wanted; the synthetic
  timeline already proves the behaviour deterministically.
- `walrus --version` flag (the acceptance TCR records the CLI has none; binary
  identity currently falls back to the git short-sha).
- Pre-existing clippy `manual_str_repeat` lint at `crates/walrus/src/archive.rs`
  (surfaces under `cargo clippy --all-targets`).

## SEARCH feature -- DONE (shipped on `main`)

STATUS (verified 2026-10-15): the search feature is LANDED on `main`, not WIP.
The library matcher is at `crates/walrus/src/search/mod.rs` +
`search/line_col.rs` (re-exported from `lib.rs` as `search_version`,
`search_versions`, `Match`, `MatchMode`, `SearchOptions`); the shared CLI engine
+ renderer are at `crates/wal-cli-common/src/search.rs` +
`search_render.rs`; the `search` subcommand is wired into BOTH binaries
(`crates/walrus-cli/src/cmd_search.rs`, `crates/walx/.../search`) with
integration tests (`crates/walx/tests/search.rs`,
`crates/walrus-cli/tests/search.rs`). All library search unit tests pass
(part of the 162 green `walrus-fmt --lib` tests). Original plan preserved for
reference at `.agents/tasks/walrus-search/plan.md`.

ACCEPTANCE + HARNESS COVERAGE (added 2026-10-16): the Python acceptance suite
has `tests/acceptance/walrus_cli/test_search.py` (11 tests: single-version
found/no-match, `-i`, `--format plain`, file-mask, selectors, index clamp,
`--regex`, read-only guard, all-version reporting, and selector conflict), with
the matching requirement ids registered in `requirements.py`
(`CLI-DESIGN-5.8-search-*` + `CLI-DESIGN-6-rc1`). The latest TCR: 134 tests,
133 pass, 0 fail, 1 skip, 0 uncovered, verdict PASS. The manual harness
(`WalrusTestHarness.bat` + `.sh`) gained STEP 21: search a known literal (RC 0),
an absent string (RC 1), and case-insensitive `-i` (RC 0); its STEP 19 also
picks up `test_search.py` automatically.

Shape (as shipped):
- MATCHER lives in `walrus-fmt` as pure logic over reconstructed bytes
  (`crates/walrus/src/search/mod.rs`: `search_version` single-version primitive
  + `search_versions` range composition). Honors the dependency boundary -- it
  opens no archives and touches no CLI.
- REGEX is behind a new OPT-IN `search-regex` cargo feature on `walrus-fmt`
  (pulls the pure-Rust `regex` crate), mirroring the `diff` / `i18n` /
  `export-*` feature pattern so the default build and a playback-only embedder
  stay lean and `cargo tree` stays auditable. Literal + case-insensitive are in
  the default build; `--regex` without the feature is a typed, documented refusal.
- CLI engine + renderer in `wal-cli-common` (`search.rs` + `search_render.rs`),
  wired as a `search` subcommand into both `walx` and `walrus-cli`. Reuses the
  existing selector / mask / `reconstruct_with_fallback` helpers.
- RETURN CODES (grep convention): 0 found, 1 no matches, 2 usage, 4 beyond-last
  numeric-index clamp in single-version mode. All-versions mode rejects a
  selector with RC 2 and has no clamp case; both details are in CLI-DESIGN
  sections 5.8 and 6.
- LINE/COLUMN rule: byte offset authoritative; line = 1 + LF count before the
  match; column = 1-based in Unicode scalar values; CR is an ordinary char.
- ALL-VERSIONS search is implemented by both CLIs using the `search_versions`
  primitive. `--all-versions` searches every version and groups report output
  by version index; a selector with the flag is a usage error (RC 2).

## Future features -- DESIGNED, not yet started

- WALKER office-document playback (phase two): view historical ODF/OOXML member
  content in Walker through its source/player surfaces. This is intentionally
  deferred until phase one is validated and merged; it must remain Walker-side
  and must not add GUI dependencies to `walrus-fmt`. Planning lives in
  `docs/proposals/office-container/walker-playback.md`.

## Deferred / future ideas (not scheduled)

- WALKER git-history source: let `walker` "play" a file's history from a GIT
  repository (ordered blobs, one per commit) through the same player, viewers,
  and transport it uses for `.wal`/`.wbl`. Feasible as a CONTAINED change now
  that the player is already source-agnostic: the `HistorySource` trait and the
  position-only `Player` landed with the multi-file re-work, so the remaining
  work is a new `git2`-backed `HistorySource` impl plus generalizing the
  `app.rs` open/sniff touch points -- NOT the player refactor the original note
  assumed. MUST stay in `walker`; `walrus-fmt` keeps no git dependency
  (invariant 2). Design note: `.agents/tasks/walker-git-source/design.md`.
- WALKER multi-format history player (Automerge, later Yrs) behind the same
  `HistorySource` trait. Design note:
  `.agents/tasks/history-source-automerge.md`. `automerge` is a Walker-side /
  feature-gated dependency ONLY; it must not enter `walrus-fmt` (invariant 2).

## Audit follow-ups (from the 2026-10-15 read-only audit)

Full report: `.agents/tasks/walrus-walker-audit.md`. The `58117df` legacy
removal is clean at the code level (no removed-symbol residue, library builds +
clippy clean, dependency boundary intact, no library `unwrap`/`panic`, variant
symmetry asserted).

Resolved since the audit:

- [DONE, merged `31ecd20`] The headless `WalkerApp` shell test drives open,
  select, seek, render, and import/mutate wiring.
- [DONE in working tree] The top-level and crate README library examples now
  use `Archive` / `FileId`; `examples/README.md` reflects the mutate API.
- [DONE in working tree] The in-memory `testsupport` fixture is isolated in its
  own feature-gated sibling module, rather than counting against `source/mod.rs`.
- [DONE in working tree] The archive reader helpers and Walker app renderer are
  split into focused siblings; both coordinator files are now under the
  non-test line cap.
- [DONE in working tree] The Walker central panel paints cached bytes and has a
  no-reconstruct-on-unchanged-frame regression test.
- [DONE in working tree] Gzip decode deferral uses a typed
  `WalrusError::UnsupportedCodec`; the archive rebase path uses
  `trim_to_versions` directly.

Remaining audit items, prioritized:
No outstanding items remain from this audit. Remaining project work is listed
in Tier C and the deferred/future ideas below.

Explicitly NOT recommended by the audit: dropping `.wbl` / reverse-log support
(first-class variant, load-bearing symmetry); moving export backends out from
behind the `export-*` features or adding GUI deps to `walrus-fmt` (boundary
confirmed intact).

## HISTORICAL -- the OLD single-file format work that was re-worked (now superseded)

This section is kept for context only. The items below were the OLD single-file
implementation; they have ALL been re-worked into the multi-file format per the
now-complete sequence above. Neither walrus-core NOR walker uses any of this any
more: walker was migrated to the `Archive` API (`5726a00`) and the legacy
`WalrusFile` shim was removed from `walrus-fmt` entirely (`58117df`):

- `walrus-fmt` single-document model (`WalrusFile { base, log }`), the single-doc
  manifest (top-level `document`/`versions`/`checkpoints`), the fixed tar members
  `manifest.json`/`base`/`log.jsonl`, and the headerless log parser. -> steps 1-2.
- Reconstruct + version addressing on the OLD index convention (index 0 = base =
  original). -> step 3 (index flip).
- Container WRITER (`write.rs`) for the three fixed members; plain `std::fs::write`
  (not transactional). -> step 4.
- `walx` `info`/`extract` over the single document; `convert` is a stub. -> step 8.
- `walker` single-document open + transport (the loop/stop/wrap state machine in
  `player.rs` is pure and reusable, but its binding to one `WalrusFile` and the
  transport's index semantics flip). -> steps 3, 9.
- Example generators + committed fixtures (`examples/walrus.wal`,
  `walrus-story.wal`) encode the old on-disk layout. -> step 7 (regenerate).
- All current unit tests and the harness test encode the old shape/index and will
  break; the new behaviours (multi-file, `.wchg` header, generations re-base,
  transactional writes, convert round-trip, diff, i18n) have no coverage yet.

## Still genuinely reusable (format-agnostic)

- GUI test approach: egui/eframe/egui_kittest 0.36; the headless harness pattern
  in `crates/walker/tests/harness.rs`. (Workspace MSRV 1.95 from egui 0.36;
  edition 2021.)
- Export backends on `feature/csv-viewer` (csv/docx/html/json/markdown/pdf) and
  the content viewers -- they operate on reconstructed bytes, so they survive the
  format change; they move to Walker-side per the export-is-Walker-only decision
  and re-point to the new per-file reconstruct API.
- Codec sniffing (zstd/gzip magic bytes) in `container.rs` is unchanged by the
  revision. (gzip decompress is still deferred; zstd-only today.)

## Build

GUI stack is egui/eframe 0.36 (egui_kittest 0.36 as a walker dev-dependency),
workspace MSRV 1.95, edition 2021.

```sh
cargo build
cargo test -p walrus-fmt
cargo test -p walker
cargo run -p walker
```

Scoped `-p <crate>` runs are the preferred inner loop; the full gate
(`cargo gate --build`, or the explicit sequence in `.agents/SESSION-STATUS.md`)
is the owner's manual step.

## Dependency boundary (keep this true)

`walrus-fmt` must stay GUI-free and free of any `ff-*` / application dependency
so third parties (and FFWB's CR-NR-107 consumer) can embed it. The entire mutate
surface (diff, re-base, add/update/remove/condense/convert, transcoding) lives in
`walrus-fmt` with no app/GUI dependency, so the `walrus` CLI and `walker` call the
identical surface. GUI and export-heavy deps live only in `walker` / behind
`walrus-fmt` optional features.
