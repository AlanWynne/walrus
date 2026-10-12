# Walrus manual round-trip harness

This folder holds a **manual, real-filesystem round-trip harness** for the
`walrus` CLI. It backs up a real source tree (this repository's own `*.md` and
`*.rs` files) into a `.wal` archive, extracts it, and byte-compares the trees,
then exercises variant / conversion / version-addressing / error paths.

It complements -- it does not replace -- the automated Python acceptance suite
in `../walrus_cli/`. The Python suite asserts fine-grained CLI behaviour against
small fixtures with exact assertions (and generates the Test Completion Report).
This harness instead stresses **whole-tree, real-filesystem round-trips**: deep
paths, hundreds of files, the actual repo content. The two are deliberately
different lenses on the same binary.

## Files

| File | Platform | Role |
| ---- | -------- | ---- |
| `WalrusTestHarness.bat` | Windows (cmd) | The 18-step harness. |
| `WalrusTestHarness.sh`  | Linux / macOS (bash) | Faithful bash port of the same steps. |
| `compareDir.bat` | Windows (cmd) | Byte-compare two directory trees. |
| `compareDir.sh`  | Linux / macOS (bash) | Bash port of the comparator. |

`compareDir` is kept as a separate, reusable utility (one job: compare two
trees, report MISSING / DIFFERENT / EXTRA, exit 0 = identical, 1 = differences,
2 = usage error). The harness calls it; it is not inlined.

## Prerequisites

Build the CLI first so the binary exists:

```sh
cargo build -p walrus-cli      # produces target/debug/walrus(.exe)
```

## Running

### Windows

```bat
cd tests\acceptance\TestData
WalrusTestHarness.bat > WalrusTestHarness-stdout.txt
```

### Linux / macOS

```sh
cd tests/acceptance/TestData
chmod +x WalrusTestHarness.sh compareDir.sh   # first time only
./WalrusTestHarness.sh | tee WalrusTestHarness-stdout.txt
```

## Overrides (environment variables)

Both scripts honour the same overrides; defaults are shown.

| Variable | Default (Windows) | Default (Unix) | Meaning |
| -------- | ----------------- | -------------- | ------- |
| `REPO`    | `C:\workspace\Walrus` | inferred from the script location | Repo root to copy source from. |
| `WALRUS`  | `%REPO%\target\debug\walrus.exe` | `$REPO/target/debug/walrus` | Binary under test. |
| `TEMPDIR` | `C:\workspace\Walrus-Temp-TestData` | `$TMPDIR/Walrus-Temp-TestData` | Scratch work area (wiped each run). |

The `.bat` currently hard-codes its paths at the top (`set` lines); edit those
if your checkout is elsewhere. The `.sh` reads the overrides from the
environment and infers `REPO` from its own location.

## Output

- **Console / stdout:** full step-by-step chatter, with a `[STEP N] ...` banner
  before each step and the per-step comparison detail.
- **Report:** a Markdown PASS/FAIL ledger at
  `<TEMPDIR>/WalrusTestHarness-Report.md` -- one table row per measurable step
  (PASS / FAIL / MANUAL), plus a totals section and an overall verdict. This is
  the easy-to-read summary; the stdout is the detail.

A run exits non-zero if any automated check failed. `MANUAL` steps (currently
the query-listing render check) have no automated pass/fail and must be eyeballed
in stdout; the report states what to confirm.

## What each step checks

| Step | Checks |
| ---- | ------ |
| 1-4  | Build the source tree, create a `.wal`, extract it. |
| 5    | Round-trip: extracted tree == original, byte-for-byte. |
| 6-7  | A no-op re-add records no new version and the archive stays byte-identical (determinism). |
| 8    | A copied archive extracts back to the original. |
| 9-10 | Mutating some files makes the tree differ from the original. |
| 11   | Adding the mutated tree makes the current version equal the mutation. |
| 12   | `--version original` restores the pristine pre-change tree. |
| 13   | A `.wbl` (reverse) archive round-trips identically to `.wal`. |
| 14   | `convert --to wbl` preserves content across variants. |
| 15   | Query terse (ls-style) and long (`-l` table) render (MANUAL). |
| 16   | A single-file add is stored under its basename, not its full path. |
| 17   | Adding a directory without `-R` is refused (RC 2). |
| 18   | Querying a missing archive fails cleanly (non-zero RC, no crash). |

## Notes / gotchas baked into the scripts

- Scratch lives OUTSIDE the repo (`TEMPDIR`) and the source copy excludes
  `target`, `.worktrees`, `.git`, `.gate`, `node_modules`, and this `TestData`
  folder, so the harness never re-ingests its own output.
- walrus roots stored names at the source dir's last path segment, so an
  extract to `-o <dir>` lands under `<dir>/Base/...`; every compare targets that
  sub-path.
- Windows only: the `.bat` uses `copy /Y` (not `xcopy`, which prompts and hangs
  under redirection) and keeps shell metacharacters (`>`, `<`, `|`, `&`,
  parentheses) out of report text.
