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

WALKER (the GUI frontend) is the one dependent piece NOT yet migrated: it still
opens archives via the legacy single-file `WalrusFile::open` and so fails on the
new multi-file fixtures (`MissingMember("base")`). Walker lives on its own
branches (`feature/walker-gui`, `feature/walker-automerge`,
`docs/walker-git-source`) and is worked in a separate stream. Migrating walker to
the `Archive` API is the outstanding cross-stream task; walrus-core already
exposes everything walker needs.

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
9. OUTSTANDING (separate walker stream) -- WALKER full frontend: migrate off the
   legacy `WalrusFile::open` to the `Archive` API (clears the known-red
   `MissingMember("base")` integration test), then two-pane navigator, per-file
   player binding, import=add/update, remove/condense/set-generations, bloat
   indicator, and the `.wchg` history viewer (Markdown report export). Headless
   `egui_kittest` tests. In-flight on walker branches (FR-K6 controls done;
   FR-K11 Automerge + a git-source history source in progress).
10. ONGOING -- Docs: keep README.md / crate READMEs aligned with the shipped
    reality. (FORMAT.md / CLI-DESIGN.md are the source of truth.) This ROADMAP
    status refresh is part of step 10.

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

## HISTORICAL -- the OLD single-file format work that was re-worked (now superseded)

This section is kept for context only. The items below were the OLD single-file
implementation; they have ALL been re-worked into the multi-file format per the
now-complete sequence above. walrus-core no longer uses any of this; `walker` is
the one place still on the legacy `WalrusFile::open` (step 9, separate stream):

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
