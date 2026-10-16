---
name: "Walrus shell-and-gate"
description: "Use for workspace shell commands, build and test verification, gate workflows, and scratch-file handling."
applyTo: "**"
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

- the PowerShell terminal tool intermittently returns `Exit Code: -1` with the
  REAL output still fully present above it, and occasionally swallows stdout
  entirely.
- A `-1` exit code is therefore NOT proof of failure, and a clean exit code is
  NOT proof of success. The printed output is the authority.
- Use a command-finished marker as the actionable technique: append
  `; $rc=$LASTEXITCODE; Write-Output "VSCODE_COMMAND_FINISHED rc=$rc"` to any
  command whose status matters and trust the printed
  `VSCODE_COMMAND_FINISHED rc=<n>` marker over a tool-reported `Exit Code: -1`.
  Prefer that sentinel for pass/fail gates here.

## Shell discipline -- prevent the mangling at the first command

### Prefer dedicated tools over the shell for inspection
For reading, finding, counting, or searching files, use `view`, `rg`, and
`glob`. Do NOT reach for `Get-Content`,
`Select-String`, `Get-ChildItem`, or `Measure-Object`. Most inspection never
needs the terminal at all, and the dedicated tools never mangle output.

### HARD RULE: line-count / 400-line-cap checks never go through a chained shell command
The 400-non-test-line cap check (NFR-1) is the single most common reason an
agent reaches for a `foreach`/`$(...)`/`;`-chained PowerShell one-liner. DO NOT.
Those multi-statement, inline-subexpression commands do not match the command-
approval allowlist, so EVERY ONE triggers an interactive permission prompt for
the user -- a real, repeated interruption -- even though the command only reads
files. This is forbidden regardless of the command being read-only.

To measure a file's non-test line count, do ONE of these instead:
- `view` the file (or `rg` for `^#\[cfg\(test\)\]` to find where
  the test module starts) and reason about the count directly -- no shell at all.
  This is the DEFAULT and needs no approval.
- If you must use the shell, run ONE simple, un-chained command whose output you
  read back, e.g. a single `rg`/`rg` for the `#[cfg(test)]` line number.
  NEVER a `foreach` loop, NEVER inline `$( ... )`/arithmetic, NEVER `;`-chained
  statements, NEVER `Select-String | Select-Object` pipelines on one line.

A read-only command can still cost the user a prompt purely because of its
SHAPE. Keep shell commands to the simple, single-job form the allowlist
recognises; push all counting/searching/reading to the dedicated tools.

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
file and read it back with `view`:

```
<command> > out.log 2>&1
```

Then read `out.log`. An empty or missing log means the command did not run as
expected -- do NOT assume success. Put a sentinel at the end of a script you
control (e.g. echo `EXIT=$LASTEXITCODE` then `FINISHED`) so a truncated capture
is obvious.

LOG LOCATION (MANDATORY): write every log/scratch file INSIDE the repo under the
git-ignored `.gate/` folder (e.g. `.gate/frk1-clippy.log`). NEVER write to the
system temp dir (`$env:TEMP`, `C:/Users/.../AppData/Local/Temp/...`) or any path
outside the workspace: writes outside the workspace trigger a permission prompt
for the user on every run, and the agent cannot clean them up afterwards without
reaching outside the workspace again. `.gate/` is already git-ignored, so these
files are never committed -- that satisfies the "do not commit scratch" rule
below without leaving the workspace. In a worktree, use that worktree's own
`.gate/` folder.

These log/scratch files are EPHEMERAL -- never commit a
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
