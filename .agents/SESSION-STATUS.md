# Walrus session status -- resume point

Last updated: 2026-10-10 (orchestrator) -- DESIGN SESSION snapshot.

## One-line state

A long DESIGN discussion settled the full MULTI-FILE format + CLI + GUI design and
wrote it into the docs. All decisions are captured in `CLI-DESIGN.md` and the
authoritative spec `FORMAT.md` (both rewritten). `ROADMAP.md` and the READMEs were
brought up to date. NO code was changed; NO commits were made. Everything below is
in the working tree, unstaged, for review.

## What this session produced (all in the working tree, uncommitted)

- `FORMAT.md` -- REWRITTEN for the multi-file format (authoritative spec).
- `CLI-DESIGN.md` -- NEW design-decision doc (the source of all decisions).
- `ROADMAP.md` -- rewritten to the multi-file reality + 10-step re-work sequence.
- `README.md` and `crates/walrus/README.md` -- updated to multi-file + walrus/walx
  split + .wchg + scope.
- `.agents/tasks/format-breaking-gap-report.md` -- NEW read-only audit of the
  codebase vs the new spec (file-by-file, with the ordered re-work sequence).
- Deleted a stray `gate-test4.txt` scratch file from the repo root.

## Decisions LOCKED this session (all recorded in CLI-DESIGN.md / FORMAT.md)

1. MULTI-FILE archives: a .wal/.wbl holds MANY tracked files, each with its own
   base, its own self-describing `.wchg` log, and its own independent index range.
   (Was single-file.) This is a FORMAT-BREAKING change.
2. Inner log = `.wchg` (one extension both variants) with a mandatory first-line
   JSONL HEADER carrying direction (forward/reverse), generations, name, encoding,
   count. Self-describing so a bare `.wchg` replays standalone.
3. INDEX 0 = CURRENT for both variants; higher index = older. (Flipped from the
   old index-0=base=original.)
4. GENERATIONS cap per file (`.wchg` header): 0 = unlimited (default); N = rolling
   window, oldest dropped via RE-BASE on add/update. Same op as condense.
5. CLI tools: `walrus` (full mgmt: query/extract/cat/add/update/remove/condense/
   convert) and `walx` (extract-only: extract + cat). POSIX/GNU conventions,
   SUBCOMMAND shape. Name `wal` REJECTED (collides with pywal/wal-lang/wall).
6. `extract` = RESTORE TO DISK (archive-tool convention; -o dir; refuse-on-exists
   unless --force). `cat` = write ONE file to stdout for piping. Return-code table:
   0 ok, 2 usage, 4 index-beyond-last (original+warn), 8 target-not-found.
7. remove needs --force for multi-match. condense has --keep N (default 0).
8. Transactional writes: temp-file-then-atomic-rename for ALL mutations.
9. DIFF ENGINE: `similar` crate (dependency-free) behind a diff-to-splice adapter
   in walrus-fmt; `imara-diff` (git-grade) is the escalation path. Do NOT hand-roll.
   Adaptive granularity (line vs word/char), UNIFORM byte-splice output. Diff
   non-uniqueness does not threaten byte-identical reconstruction.
10. Line-orientation is a diff GRANULARITY feature only; the byte splice stays the
    SINGLE record primitive. NO {line,col} record type (ambiguous).
11. BINARY files OUT OF SCOPE now (reject on add); binary-delta is a documented
    FUTURE op (extensible `op` enum). base64 insert deferred.
12. INTERNATIONAL: UTF-8 is the universal internal storage. Non-UTF-8 sources
    (GBK/GB18030/Shift-JIS/Big5/windows-125x) transcoded via `encoding_rs` on add,
    original encoding recorded per file, restorable on extract. Grapheme-aware diff
    so multi-byte scripts never split mid-character.
13. EXPORT (content transformation to html/docx/pdf/md/...) is WALKER-ONLY. CLI
    never transforms. `.wal <-> .wbl` convert is a VARIANT change, not export, and
    stays in the CLI.
14. WALKER = full GUI frontend: two-pane file navigator (external import pane +
    internal tracked-files pane, multi-select), import=ONE action resolving to
    add(new name)/update(existing), per-file generations control, condense,
    remove, content viewers, BLOAT INDICATOR (green R<2 / amber 2<=R<5 / red R>=5
    on R=log_bytes/current_bytes, 50-version floor), and a `.wchg` HISTORY VIEWER
    (stepped before/after diff; opens standalone; EXPORTABLE as a Markdown change
    report). Walker calls the same walrus-fmt mutate API; boundary preserved.
15. LF record terminator (STRICT) for `.wchg` framing: write LF only, reject CRLF
    on read. Governs log framing ONLY, not tracked-document content. Record content
    is NEVER trimmed (trailing spaces are data, e.g. Markdown hard breaks).
16. RE-BASE leaves NO log marker; the audit trail is manifest per-file metadata
    (the `rebase` object: count/last/dropped). Resolved the re-baseline open Q.

## FORMAT.md open questions -- NOW RESOLVED this session

The three "still open" items in FORMAT.md section 7 were answered and folded in:
- line/col records -> NO (granularity instead; splice stays the one primitive).
- binary base64 insert -> deferred, future extension.
- re-baseline marker -> manifest `rebase` metadata, not a log record; plus the
  strict LF framing + no-trim rules in section 4.1.

NOTE: FORMAT.md section 7 prose may still list these as "still open" -- a small
edit to mark them resolved was IN PROGRESS when the session was interrupted. Verify
section 7 reads as resolved; if not, update it to match sections 3 (rebase), 4.1
(LF/no-trim), 4.4 (line = granularity not a record).

## Two work streams (unchanged, still the plan)

1. FORMAT RE-WORK (the breaking multi-file change) -- NOT STARTED. Library-first per
   the 10-step sequence in ROADMAP.md / the gap report. Belongs in its own worktree.
2. VIEWERS + EXPORT on `feature/csv-viewer` -- ACTIVE, 2 commits ahead of main
   (FEAT-001 export backends `21ec603`, FEAT-002 content viewer `2edb93e`), nearly
   current. Built on the OLD single-file lib, so it inherits the index-flip +
   multi-file obligations when it lands.

Recommended merge order: land `feature/csv-viewer` first, THEN start the format
re-work in a fresh worktree off updated main (so one combined tree migrates to the
new format rather than two branches racing to edit app.rs/player.rs).

## RESUME CHECKLIST (next session)

1. Read `CLI-DESIGN.md` (decisions) and `FORMAT.md` (spec) -- they are the source
   of truth. Then `ROADMAP.md` (plan) and `.agents/tasks/format-breaking-gap-report.md`
   (file-by-file re-work audit).
2. Confirm FORMAT.md section 7 marks the three questions RESOLVED (see NOTE above);
   fix if the interrupted edit did not land.
3. Decide/confirm next action with the user. Natural options:
   - Finish + merge `feature/csv-viewer`, then start the format re-work (step 1 of
     the ROADMAP sequence: manifest + .wchg header types) in a NEW worktree.
   - Or start the format re-work now in parallel in its own worktree.
4. No workflow is currently running for the format work. The viewer/export workflow
   from earlier is done (its commits are on feature/csv-viewer).

## Known environment gotchas

- `execute_pwsh` intermittently returns exit code -1 with the real output present
  (or swallows output). Trust printed output over the exit code; route through a
  log file if empty. Clean up scratch/probe files before any commit.
- Verify background-activity liveness from disk mtimes, never trust a "running"
  label (see background-process-status steering).
