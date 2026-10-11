# Walrus roadmap / build state

This records the honest state of each piece so the next session can pick up
without re-discovering it.

IMPORTANT -- a FORMAT-BREAKING revision is in progress. The format moved from
SINGLE-FILE to MULTI-FILE (an archive now holds many tracked files, each with its
own base and its own `.wchg` change log and its own index range). The authoritative
spec is [`FORMAT.md`](FORMAT.md) (already rewritten) and the design decisions are
in [`CLI-DESIGN.md`](CLI-DESIGN.md). Much of the "Done" work below was built for
the OLD single-file format and must be re-worked; it is marked accordingly.

See the gap report at `.agents/tasks/format-breaking-gap-report.md` for the
file-by-file audit and the ordered re-work sequence this roadmap follows.

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

1. FORMAT RE-WORK (the breaking multi-file change). Not started. Library-first,
   per the ordered sequence below. Belongs in its own worktree/branch.
2. VIEWERS + EXPORT (`feature/csv-viewer` branch). Active and nearly current:
   content-aware viewers (text/code/markdown/html/csv/richtext) and real export
   backends (csv/docx/html/json/markdown/pdf). Built on the OLD single-file
   library, so it inherits the index-flip + multi-file obligations when it lands.

Recommended merge order: land `feature/csv-viewer` first, then start the format
re-work in a fresh worktree off the updated main, so the re-work updates one
combined viewer+export+library tree to the new format rather than two branches
racing to change the same files.

## Format re-work sequence (library-first; TDD; keep walrus-fmt GUI-free)

Each step leaves the workspace buildable. Write failing tests first for the pure
logic (reconstruct, version addressing, re-base, diff, player). Scoped
`-p <crate>` checks during development; the full gate is the owner's manual step.

1. Spec-anchored types: reshape `manifest.rs` (top-level single-doc fields ->
   per-file `files[]` entries with id/attributes/generations), add the
   `.wchg` header type + `Direction` enum; fix `base_index` semantics per variant.
2. Container READER for the per-file layout (`files/<id>/base`,
   `files/<id>/<name>.wchg` with its header line); reshape `WalrusFile`/`Archive`
   to the per-file collection.
3. INDEX FLIP in reconstruct + version addressing: `Index(0) == current` for both
   variants; assert byte-identical output across variants (strict TDD).
4. Container WRITER for the per-file layout + `.wchg` header + transactional
   temp-file-then-rename writes.
5. RE-BASE primitive, then `condense(keep)` and the automatic generations trim
   sharing it.
6. DIFF (`similar` + adapter, adaptive granularity, grapheme-aware) + the mutate
   API (add/update/remove/set-generations) + `.wal <-> .wbl` convert +
   `encoding_rs` transcoding. All mutations transactional.
7. Regenerate examples + fixtures (`generate_*.rs`, `examples/*.wal`); re-establish
   the forward-then-backward byte-identical integration test.
8. CLI: re-scope `walx` to extract-only (extract + cat, multi-file + mask, RC
   table); create the new `walrus` binary (query/add/update/remove/condense/
   convert) with POSIX/GNU conventions. `add`/`update` take `-R`/`--recurse`
   (CLI-DESIGN 5.4b, requirements FR-W11 AC7): a CLI-layer walk that expands each
   directory argument into the file batch, storing each file under its path
   relative to the recursion root (root name included) as the archive `name`.
   This reuses the existing `Archive::add`/`update` mutate API (step 6) -- no new
   library primitive -- so a recursed add + `walx extract` round-trips the tree.
9. WALKER full frontend: two-pane navigator, per-file player binding, import=
   add/update, remove/condense/set-generations, bloat indicator, and the `.wchg`
   history viewer (Markdown report export). Headless `egui_kittest` tests.
10. Docs: keep README.md / crate READMEs aligned with the shipped reality.
    (FORMAT.md / CLI-DESIGN.md are the source of truth.)

## Previously "done" -- now obsolete or needs re-work (OLD single-file format)

These compiled and were unit-tested against the OLD single-file format. They are
NOT a working base for the new format and are superseded by the sequence above:

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
