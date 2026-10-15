# Walker git-history source (DEFERRED future development)

Status: DEFERRED -- not scheduled. This is a design note captured for a future
session. No code change is part of this document; it exists so the idea and its
feasibility assessment are not lost.

Branch: `docs/walker-git-source` (this doc). Implementation, when it happens,
should branch fresh off the updated main at that time -- the walker internals
referenced below WILL have changed by the multi-file format re-work (see
`ROADMAP.md`), so re-confirm the touch points before coding.

## Goal

Let `walker` "play" the version history of a file taken from a git repository,
the same way it plays a `.wal` / `.wbl` archive: scrub a timeline, auto-play
forward/backward, loop, speed control, through the existing content-aware
viewers. The source of the versions would be `git log` for one file rather than
a Walrus change log.

## Why it is feasible (as assessed against the current code)

The player is already decoupled from everything except three things it asks of
the opened file. In `crates/walker/src/player.rs` the `Player` only calls:

- `file.len()` -- the far-end index (number of versions).
- `file.reconstruct(Version::Index(k))` -- the bytes of version `k`.
- `file.manifest()` / `variant()` -- document name, MIME, variant, for the
  viewer selection and status bar.

Everything else in `Player` (step / play / loop / direction / speed) is pure
arithmetic over `index` and `len`. It does not care where the bytes come from.
A git file history is exactly an ordered list of blobs (one per commit that
touched the file), so it maps directly onto "give me version k as bytes".

Index semantics line up with Walrus's convention already: index 0 = current
(HEAD), higher index = older. The seek slider, loop, and direction logic carry
over unchanged.

## The coupling that must be broken first

NOTE (2026-10-15): the coupling described below is ALREADY BROKEN on `main`.
The multi-file format re-work introduced the `HistorySource` trait in
`crates/walker/src/source/` (see `.agents/tasks/history-source-automerge.md`)
and `Player` is now position-only (it no longer holds a `WalrusFile`; that type
was removed in `58117df`). So steps 1-3 below are substantially DONE; a git
source now means a new `HistorySource` impl, not a player refactor. Re-read the
current `source/mod.rs` trait and `WalrusSource` before coding.

Historically (pre-re-work) `Player` held a concrete `WalrusFile` by value and
`app.rs` called `WalrusFile::open(&path)` directly, so a git source could not be
dropped in. That coupling is gone.

## Proposed approach

1. Extract a trait in `walker` capturing exactly what the player consumes.
   (DONE on main as `HistorySource` in `crates/walker/src/source/`.)

2. Impl the trait for the Walrus archive. (DONE on main as `WalrusSource`.)

3. Make `Player` source-agnostic. (DONE on main -- `Player` is position-only
   over `index`/`len`.)

4. Add a `GitFileHistory` source in `walker` implementing `HistorySource`:
   - build the ordered commit list for one path via `git2` (preferred) or by
     shelling out to `git log --follow -- <path>`;
   - `versions()` / `len()` = number of commits that touched the file;
   - `reconstruct(index)` = the blob of that file at commit `index`;
   - `content_hint()` = name/MIME from the path + extension (same viewer
     selection the archive path uses today).

5. Generalize the remaining `app.rs` open/sniff touch points so a git source can
   back the player alongside `WalrusSource` and the Automerge source.

## Caveats to design around (not blockers)

- Granularity differs: a Walrus log is one net splice per SAVE; git history is
  one blob per COMMIT that touched the file. Git playback is chunkier than the
  per-save walrus demo. Property of the source, not a defect.
- Reconstruction cost: git hands a whole blob per version (simpler, no splice
  engine) vs Walrus replaying splices. Fine for a player.
- `variant` and the `.wchg` history viewer are Walrus-specific; a git source
  reports neutral values there, or that chrome is hidden when the source is not
  a Walrus archive.
- Rename/merge topology: `git log --follow` handles renames; flatten to
  first-parent or `--follow` order to get a single linear playable sequence.

## Dependency boundary (MANDATORY)

This belongs ENTIRELY in `walker` (or a small sibling crate). `walrus-fmt` MUST
stay git-free and GUI-free per project invariant 2 -- the `git2` / shell-out
dependency and the `GitFileHistory` source MUST NOT enter the format library.

## Scope of the change

Contained: add a `GitFileHistory` `HistorySource` impl and the git-to-ordered-
blobs adapter, then generalize the remaining `app.rs` open/sniff touch points.
The player state machine and its existing tests are unaffected. The real work is
the git-to-ordered-blobs adapter. (The trait/player decoupling that the original
note listed as the hard part is already done on main.)
