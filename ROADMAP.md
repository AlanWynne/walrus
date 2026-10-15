# Walrus roadmap / build state

This records the honest state of each piece so the next session can pick up
without re-discovering it.

STATUS (2026-10-13): the FORMAT-BREAKING single-file -> MULTI-FILE revision is
DONE for walrus-core and shipped on `main`. An archive holds many tracked files,
each with its own base, `.wchg` change log, and independent index range. The
authoritative spec is [`FORMAT.md`](FORMAT.md); design decisions are in
[`CLI-DESIGN.md`](CLI-DESIGN.md). The library (`walrus-fmt`), both CLIs (`walx`
extract-only, `walrus` full management), the shared `wal-cli-common`, the
regenerated examples, and an exhaustive Python acceptance suite + TCR are all on
main. The historical gap report at
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

## SEARCH feature -- WIP (on the `walrus-core` worktree, not yet merged)

Search a `.wal` / `.wbl` archive for a string and report WHERE it was found
(file name/path, version index, byte offset, 1-based line, 1-based column) as a
Markdown report (default) or plain text, from BOTH `walrus` and `walx`. Full
plan: `.agents/tasks/walrus-search/plan.md` (carried into main alongside this
entry). KEEP -- this is real, wanted work-in-progress; do not discard.

Shape (as planned/implemented in the worktree):
- MATCHER lives in `walrus-fmt` as pure logic over reconstructed bytes
  (`crates/walrus/src/search.rs`: `search_version` single-version primitive +
  `search_versions` range stub for the deferred all-versions path). Honors the
  dependency boundary -- it opens no archives and touches no CLI.
- REGEX is behind a new OPT-IN `search-regex` cargo feature on `walrus-fmt`
  (pulls the pure-Rust `regex` crate), mirroring the `diff` / `i18n` /
  `export-*` feature pattern so the default build and a playback-only embedder
  stay lean and `cargo tree` stays auditable. Literal + case-insensitive are in
  the default build; `--regex` without the feature is a typed, documented refusal.
- CLI engine + renderer in `wal-cli-common` (`search.rs` + `search_render.rs`),
  wired as a `search` subcommand into both `walx` and `walrus-cli`. Reuses the
  existing selector / mask / `reconstruct_with_fallback` helpers.
- RETURN CODES (grep convention): 0 found, 1 no matches, 2 usage, 4 beyond-last
  numeric-index clamp. The RC-1 "no matches" row is search-specific and is to be
  documented in CLI-DESIGN section 6.
- LINE/COLUMN rule: byte offset authoritative; line = 1 + LF count before the
  match; column = 1-based in Unicode scalar values; CR is an ordinary char.
- ALL-VERSIONS search is DESIGNED-FOR but DEFERRED: the `search_versions` range
  primitive and the version-range renderer seam exist, but the CLI wires only
  single-version search for now.

STATUS: lives on the `feature/walrus-core` worktree as uncommitted/staged work
(new `search.rs` / `cmd_search.rs` / `search_render.rs` + tests across
`walrus-fmt`, `wal-cli-common`, `walrus-cli`, `walx`, plus `CLI-DESIGN.md` and
`ROADMAP.md` edits). NEXT: finish per the plan's TDD item list, run the scoped
checks, then land it on `main` via a fresh worktree off current `main` (the
branch tip predates the legacy-removal, so rebase/re-apply onto `main` rather
than merging the stale branch -- per the "Branch hygiene" rule).

## Future features -- DESIGNED, not yet started

- OFFICE-CONTAINER (ODF / OOXML): store an office document in a `.wal` by
  UNPACKING it into its inner zip parts, tracking each part as its own file
  grouped as one logical document, and REPACKING byte-identically on extract.
  Approved, design-reviewed planning lives at `docs/proposals/office-container/`
  (requirements, design, a spec-first FORMAT.md change proposal, ADRs, and a
  phase-two Walker view-only playback plan blocked on phase one). PLANNING ONLY
  -- no code yet. When development starts: create a FRESH worktree off the
  latest `main` (per the "Branch hygiene" rule in
  `.kiro/steering/project-conventions.md`) and begin with the spec-first
  FORMAT.md change in `format-change-proposal.md`.

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
clippies clean, dependency boundary intact, no library `unwrap`/`panic`, variant
symmetry asserted). Open items, prioritized:

- [HIGH] No test drives the REAL `WalkerApp` shell (`update`/`ui`, `open_walrus`,
  `refresh_bytes`, `bind_player`, export/import/mutate wiring). Every walker
  harness test rebuilds a slice of app logic in a standalone closure that
  MIRRORS `app.rs` rather than running it -- the exact shape of the bug that
  shipped green (the `open_walrus` legacy-reader bug). FIX: a headless
  `egui_kittest` test that opens a temp multi-file `.wal` through `WalkerApp` and
  asserts player size / active selection / `current_bytes == reconstruct(file,
  Index(0))`; also drive `handle_file_action` / `apply_import` against a
  `TempDir`. Feasible headless (only the `rfd` dialogs are genuinely manual).
  Effort ~0.5-1 day.
- [MEDIUM] Walker re-reconstructs the viewed document EVERY FRAME
  (`app.rs` central panel -> `view::render` -> `source.reconstruct`), while the
  cached `current_bytes` is used only on a dead fallback arm. Wasteful during
  playback repaints. FIX: paint from `current_bytes` on the primary arm too
  (reconstruct only in `refresh_bytes`), or add a per-(file,version) memo in
  `WalrusSource`. Keep any cache in walker (invariant 2). Effort ~1-2 h + a
  regression test.
- [MEDIUM] NFR-1 (400-non-test-line cap) breaches: `crates/walrus/src/archive.rs`
  (~440; split the container-collection read helpers into an `archive_read.rs`
  sibling), `crates/walker/src/source/mod.rs` (~472, inflated by the
  NON-`cfg(test)` `testsupport` module -- gate it behind `cfg(any(test, feature
  = "testsupport"))` or move to `source/testsupport.rs`, which alone fixes it),
  `crates/walker/src/app.rs` (~535; split the `ui` render builders into an
  `app_render.rs`). Pure refactors, verified by existing scoped tests.
- [MEDIUM] README.md "Using the library" example still names the removed
  `WalrusFile` type -- rewrite against `Archive` / `FileId` (mirror
  `crates/walker/tests/integration.rs`). (This ROADMAP update covers the stale
  walker status the audit also flagged.)
- [LOW] `rebase.rs::trim_to_versions` is dead production surface (test-only,
  justified `#[allow(dead_code)]`): either route the Archive generations path
  through it or drop it and keep its zero-versions guard test against `trim_to`.
- [LOW] gzip-not-supported returns `WalrusError::Manifest(...)` -- give it a
  dedicated `UnsupportedCodec` variant for cleaner error taxonomy (`WalrusError`
  should be `#[non_exhaustive]`, so additive).

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
