---
inclusion: always
---

# Shell Output and the Verification Gate

Two recurring failures on this machine waste real time: the terminal silently
swallows or mangles command output, and the full verification gate is slow and
flaky. These rules exist to work around both. They are adapted from the
FileForgeWorkbench project's `tooling.md` and `testing.md`, keeping only what
applies to Walrus (no invented tooling: Walrus has no pwsh7 wrapper, no guard
hook, and no `cargo full-gate` alias -- do not reference machinery that is not
here).

## The known flakiness (observed, not hypothetical)

- `execute_pwsh` intermittently returns `Exit Code: -1` with the REAL output
  still fully present above it, and occasionally swallows stdout entirely.
- A `-1` exit code is therefore NOT proof of failure, and a clean exit code is
  NOT proof of success. The printed output is the authority.

## Shell discipline -- prevent the mangling at the first command

### Prefer dedicated tools over the shell for inspection
For reading, finding, counting, or searching files, use `read_file`,
`grep_search`, `list_directory`, `file_search`. Do NOT reach for `Get-Content`,
`Select-String`, `Get-ChildItem`, or `Measure-Object`. Most inspection never
needs the terminal at all, and the dedicated tools never mangle output.

### One command, one job
- NEVER `;`-chain unrelated steps onto a real command. Chaining is a primary
  cause of the character-by-character echo and the glued-command `-1` exit.
- NEVER glue a process-kill (`Stop-Process` / `taskkill`) onto another command.
  If a cleanup is genuinely needed, run it as its OWN separate invocation.
- NEVER pipe a real command into `Format-Table` / `Select-Object` or embed
  inline `$( ... )` subexpressions on the same line. If logic is needed, write a
  small script and run that.
- Independent commands go in separate tool calls, not a single chained line.

### Trust the printed output, not the exit code
When a command prints a complete, coherent result but reports `Exit Code: -1`,
treat the output as the truth. Do NOT "recover" from a phantom failure by
re-running a more complex form -- that reactive path reproduces the mangling.

### Route anything non-trivial through a log, then read the log
Terminal execution frequently swallows stdout from scripts and multi-step runs.
For any script or run whose output you must inspect, redirect stdout+stderr to a
file and read it back with `read_file`:

```
<command> > out.log 2>&1
```

Then read `out.log`. An empty or missing log means the command did not run as
expected -- do NOT assume success. Put a sentinel at the end of a script you
control (e.g. echo `EXIT=$LASTEXITCODE` then `FINISHED`) so a truncated capture
is obvious. These log/scratch files are EPHEMERAL: write them outside the repo
or delete them before any commit (see cleanup below) -- never commit a
`*-out.txt` / `gate-test*.txt` / `probe.txt` style scratch file.

### If output is empty
Re-run once routed through a log file (above). If still empty, state plainly
that you could not capture output rather than guessing at the result.

## The verification gate

Walrus writes gate artifacts to `.gate/` (git-ignored). The canonical full gate
is `cargo gate --build`.

### The full gate is the OWNER's manual step, not an automated one
The full `cargo gate --build` (and any `--workspace` build/test) is a
multi-minute run and is exactly where the flaky exits and dropped connections
bite. Do NOT run it as a routine automated step. Instead run the SCOPED checks
for what changed, which return in seconds:

```
cargo build -p <crate>
cargo test  -p <crate>
cargo test  -p <crate> <test_name>        # a single test by name
cargo clippy -p <crate> -- -D warnings
cargo fmt -- --check
```

Prefer `-p <crate>` scoping during active development (`-p walrus-fmt`,
`-p walker`, `-p walx`); whole-workspace runs are slower and flakier.

### Completion is two-staged
A clean SCOPED run certifies "code-complete pending full gate". The task is
"done" only after the full `cargo gate --build` runs clean -- an EMPTY
`.gate/gate.review.log` is the done signal. A clean scoped run alone is NOT
sufficient to claim "done".

### Hand-off protocol
1. Finish the task's scoped checks and confirm they are clean.
2. STOP and state which scoped commands ran, plus the exact full-gate command
   for the owner to run: `cargo gate --build` (or the explicit 5-command
   sequence below if `cargo gate` is unavailable).
3. The owner runs the full gate and replies "clean" or pastes the failing output
   / `.gate/gate.review.log`.
4. Act on the result: if clean, the task is done; if it fails, fix with scoped
   checks and hand off again.

Do NOT declare a task complete before step 3 returns clean, and do NOT run the
full gate yourself to "save a round-trip".

### The explicit 5-command gate (fallback when `cargo gate` is unavailable)
Documented in `.agents/SESSION-STATUS.md` and run in the relevant checkout:

```
cargo build                                   # whole workspace
cargo build -p walrus-fmt                     # no features ...
cargo tree  -p walrus-fmt                      # ... must show NONE of
                                               # pulldown-cmark/scraper/csv/
                                               # printpdf/docx-rs/egui/eframe
                                               # (the GUI-free dependency boundary)
cargo build -p walrus-fmt --features export-all
cargo test                                     # whole workspace
cargo fmt -- --check
cargo clippy -- -D warnings
```

Do NOT trust a workflow's or process's self-reported "gate passed": verify from
the gate artifacts / printed output per the liveness rule in
`background-process-status.md`.

## Cleanup before committing
Scratch capture and probe files (`*-out.txt`, `gate-test*.txt`, `probe.txt`,
`net*.txt`, `fetch*.txt`, `tree-nofeat.txt`, `status.txt`, and similar) MUST NOT
be committed. Preview with `git clean -n`, confirm they are gone before staging,
and prefer writing such scratch files outside the repository in the first place.
