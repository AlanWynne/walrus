---
inclusion: always
---

# Walrus Project Conventions

Walrus is a toolkit for two self-describing document formats that carry their
own edit history: `.wal` (original base + forward log) and `.wbl` (current base
+ reverse log). The workspace has three crates:

| Crate | Kind | Role |
|-------|------|------|
| `walrus-fmt` (`crates/walrus`) | library | The format + reconstruct engine. Embeddable. |
| `walx` (`crates/walx`) | CLI | Inspect, extract, convert `.wal <-> .wbl`. |
| `walker` (`crates/walker`) | GUI (egui/eframe) | Viewer / player / exporter / CSV grid. |

Read `README.md`, `FORMAT.md`, and `ROADMAP.md` before making changes. `FORMAT.md`
is the authoritative, language-agnostic format spec; `ROADMAP.md` records the
honest done / stubbed state of each piece.

## Load-bearing invariants (treat as fixed unless the owner confirms a change)

1. **FORMAT.md is the source of truth.** The on-disk format is defined in
   `FORMAT.md`, independently of the Rust implementation, so it can be
   implemented in any language. Code MUST NOT diverge from the spec. If an
   implementation need requires a format change, update `FORMAT.md` first (it is
   a 0.1 draft, so changes are allowed pre-1.0) and call out the change
   explicitly -- never let the code silently define a different format.

2. **The dependency boundary is sacred.** `walrus-fmt` MUST stay GUI-free and
   free of any application dependency (no `eframe`/`egui`, no `ff-*`, no `walx`/
   `walker` types) so third parties -- and FileForgeWorkbench's planned
   CR-NR-107 consumer -- can embed it. GUI and export-heavy dependencies live
   ONLY in `walker`, or behind `walrus-fmt` optional cargo features
   (`export-html`, `export-md`, `export-all`, ...). A playback-only embedder
   must stay lean: do not make an export backend non-optional.

3. **The two variants are symmetric and inter-convertible.** `.wal` and `.wbl`
   differ only in which end is the base and which direction the log plays
   (FORMAT.md section 1). Reconstruction MUST produce byte-identical output for
   the same version index once the variant's index semantics are accounted for
   (FORMAT.md section 5). `walx convert` performs `.wal <-> .wbl` by inverting
   each splice and re-basing. Preserve this symmetry; do not special-case one
   variant with behaviour the other lacks.

4. **Splices are the change primitive.** A change record is a splice: at a byte
   offset, delete `delete_len` bytes and insert `insert` bytes (FORMAT.md
   section 4). A record is self-inverting given the version it applied to. Keep
   this the single primitive; propose line/col or binary-insert variants (the
   FORMAT.md section 7 open questions) only as a deliberate, spec-first change.

5. **Container reader sniffs the codec from magic bytes, not the extension**
   (FORMAT.md section 2): zstd `0x28 0xB5 0x2F 0xFD`, gzip `0x1F 0x8B`. The file
   extension (`.wal` / `.wbl`) selects the variant semantics, not the
   compression codec.

## Naming note

The crates.io name `walrus` is taken by an unrelated WebAssembly crate, so the
library is published as `walrus-fmt`. The repository, the GUI binary (`walker`),
and the mascot keep the name "Walrus". Do not rename the published crate.

## Working on roadmap items

`ROADMAP.md` is the task list. When you complete a stubbed item, update
`ROADMAP.md` to move it from "Stubbed / TODO" to "Done", and keep the honest
state accurate for the next session. The natural next unlock is the container
WRITER (item 1): most other TODOs -- the demo fixture, `.wal <-> .wbl`
conversion, save support -- depend on it.

## Branch hygiene -- REBASE ONTO LATEST main BEFORE STARTING DEVELOPMENT

Work happens on many parallel streams/worktrees (walrus-core, walker, planning
branches). Before you START implementing on any feature or planning branch --
and again before you merge it -- REBASE the branch onto the latest `main`:

```sh
git fetch origin
git rebase origin/main        # from the branch's worktree
```

Why this is mandatory:
- A branch cut from an older `main` carries a STALE tree. Its diff against
  current `main` shows PHANTOM deletions of files that landed on `main` after
  the branch was cut (e.g. a planning branch once appeared to delete
  `input_glob.rs`, the deep-history/at-timeline/glob tests, etc. -- purely
  because it predated them). Merging such a branch without rebasing first can
  REVERT other streams' merged work.
- Rebasing replays only the branch's OWN commits on top of current `main`, so
  the result is "latest main PLUS this branch's additions", with no accidental
  reversions. Resolve any conflicts in shared files (FORMAT.md, CLI-DESIGN.md,
  ROADMAP.md) by keeping main's content and layering the branch's change on top.
- After rebasing, VERIFY: `git diff --stat main <branch>` must show ONLY the
  files the branch legitimately changes -- no surprise deletions of main's work.

If the branch has already been pushed and is shared with another stream, do NOT
force-push the rebased history without coordinating -- a rebase rewrites commit
ids and will disrupt anyone else who has the old branch. Rebase locally, verify,
and agree on publishing with the branch's owner.

## Relationship to FileForgeWorkbench (FFWB)

Walrus is a standalone project. FFWB is one consumer: when it adds `.wal`/`.wbl`
support (its CR-NR-107) it will depend on the `walrus-fmt` crate rather than
reimplement the format. That is exactly why invariant 2 (the dependency
boundary) matters -- keep `walrus-fmt` embeddable so there stays one source of
truth for the format.
