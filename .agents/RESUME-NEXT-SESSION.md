# Resume note -- Walrus session hand-off

Last updated: 2026-10-16. Tier A and Tier B passed their full gates.

## Git state (verified)
- `HEAD` is `main` == `origin/main` == `31ecd20` before the current uncommitted
  follow-up changes.
- The working tree is dirty with this Tier A follow-up and other pre-existing
  changes. Do not reset or stage the unrelated changes.
- Owner ran `cargo gate --build` on 2026-10-16; `.gate/gate.report.log` records
  CLEAN, 583 tests passed, 0 failed, completed at 22:10:36 in 2:27.
- Owner reran `cargo gate --build` after Tier B on 2026-10-16;
  `.gate/gate.report.log` now records CLEAN, 585 tests passed, 0 failed,
  completed at 22:36:48 in 1:14.

The remainder of this file is a historical session log. Use the final
"NEXT STEP" section for the current hand-off; re-check `git status` before
staging anything.
- Other worktrees present (OTHER STREAMS, do not touch): walker-gui,
  walker-automerge, walrus-core. These are not this work's concern.

## Earlier additions (2026-10-13, Python acceptance suite on main)
- test_roundtrip_scale.py: 50-file mixed-depth tree round-trips byte-identical
  for BOTH .wal and .wbl (ids CLI-DESIGN-5.3-add-scale, FR-W11-AC7/AC7b).
- test_determinism.py: no-op re-add of unchanged content leaves the archive
  BYTE-IDENTICAL -- invariant HELD (PASS), writes .gate/determinism-finding.txt
  with hexdump evidence only if ever violated (id -add-noop-deterministic).
- TCR now 110/110 PASS, 0 uncovered. Reports at <repo>\.gate\tcr\.
- NOTE: the py-scale-tests workflow (wf_8dfc9137cf8ce982) STALLED ~7h at the
  commit boundary (agent-restart pause); work was complete + verified, so I
  aborted it and finished the commit (0726fec) + ff-merge + push MANUALLY.

## Deep-history + harness unification -- DONE (2026-10-13, main dd52730, pushed)
All four landed via wf_f118c15798b2c2bc (which again STALLED at the merge step
~33min after committing but before ff-merge; recovered manually):
1. test_deep_history.py: one file, 50 mixed-size versions via `update -R`,
   byte-exact restore at EVERY --index, --version original/current/+k spot-checks,
   --at far-past sentinel; mid-history --at is skipTest'd (suite can't pin per-
   version stamps). id CLI-DESIGN-5-deep-history PASS. Reconstruct held byte-
   exact at all 50.
2. crates/walrus/tests/deep_history.rs: 300 mixed-size versions, reconstruct ==
   oracle at every index + container round-trip. 2 tests PASS. Byte-exact at 300.
3. OPTION A done: both harness reports now write <repo>/.gate/tcr/
   WalrusTestHarness-Report.md (mkdir guard; TEMPDIR scratch unchanged); README
   updated. So BOTH the Python TCR and the manual harness report now live in
   <repo>/.gate/tcr/.
4. Guarded STEP 19 in both .bat and .sh runs the Python suite + run_tcr.py,
   skips gracefully if python absent, contributes a PASS/FAIL row when present.
TCR: 113 tests, 112 pass, 0 fail, 1 skip (the mid-history --at), 0 uncovered,
verdict PASS.
NOT RUN HERE (owner to confirm): the harness scripts' new STEP 19 was not
executed end-to-end (.sh can't run on this box's busybox bash; .bat not run);
syntax-checked + visually reviewed only.
Two pre-existing OUT-OF-SCOPE items disclosed by the agents (NOT caused by this
work): (a) a FLAKY test_convert.py "target already exists" on extract --version
original (intermittent); (b) a clippy manual_str_repeat lint at
crates/walrus/src/archive.rs:1107 under --all-targets. Both left for the owner.

## IMPORTANT git-state change observed this session
The main checkout c:\workspace\Walrus is NO LONGER on `main` -- it now has
`docs/office-container-planning` (d42626e) checked out, and new worktrees
appeared (walker-automerge, walker-git-source, walker-gui, walrus-core) from
OTHER streams/sessions. Because `main` was not checked out anywhere, the usual
`git merge --ff-only` into the working tree FAILED ("diverging") -- it was
trying to ff `docs/office-container-planning`, not main. RECOVERY PATTERN that
worked and did NOT disturb the other branch: `git -C <repo> fetch .
feature/deep-history:main` to fast-forward the main REF directly, then
`git push origin main`. Use this when main isn't the checked-out branch. Do NOT
switch the main checkout's branch out from under the other stream's work.

## Earlier git state (superseded, kept for reference)
- Merged stream branches (`feature/wc-listing`, `feature/walrus-core`) were
  deleted local + origin earlier; walrus-core worktree later reappeared from
  another stream's activity -- leave it alone.

## What shipped this session (walrus-core, all on main)
- Change 1: name-independent `f<N>` tracked-file ids -> fixes the ustar
  "provided value is too long" crash on deeply-nested / long relative paths
  (`d5b81b9`). VERIFIED: a ~140-char nested path now adds cleanly (exit 0).
- Change 2: `query` terse is now `ls`-style (one relative path per line);
  `query -l` is an aligned table `Versions | LastWriteTime | Bytes | Name`
  with header + `----` separator (`054afa2`). VERIFIED against main binary.
- Change 3: `LastWriteTime` is the SOURCE FILE's mtime, read by the CLI
  (`clock::file_mtime_utc`, now_utc fallback) and stored via the clock-free /
  fs-free library seam `Archive::set_attributes_mtime` (gated on `!was_tracked`
  in add only) (`fce4a5e`). VERIFIED: single-version file shows a real time.
- Acceptance suite + TCR: 107/107 PASS, 0 uncovered (`c7611a9`).
- FORMAT.md section 2.1/3 updated for the `f<N>` id scheme (spec-first).

## Uncommitted working-tree items (snapshot from 2026-10-13; historical)
- `tests/acceptance/TestData/` -- the manual `.bat` round-trip harness
  (`WalrusTestHarness.bat`, `compareDir.bat`) we fixed this session. Currently
  UNtracked/uncommitted. Decision needed: commit to main or leave local-only.
  - Harness now: rebuilds a clean source tree (robocopy *.md/*.rs, excludes
    target/.worktrees/.git/.gate/TestData/Work), archives it OUTSIDE the repo
    (C:\workspace\Walrus-TestData-Work) to avoid re-ingesting its own output,
    extracts, then byte-compares via `compareDir.bat "<DIR1>" "<DIR2>"`.
  - It should now run clean end-to-end because the Change-1 crash is fixed and
    the main `walrus.exe` is rebuilt/current. NOT yet re-run after the fix.
- `.kiro/steering/background-process-status.md`, `.kiro/steering/shell-and-gate.md`
  -- modified (pre-existing from before this session). Decision needed:
  review/commit or discard.
- `.kiro/hooks/`, `tools/`, `.worktrees/` -- untracked, pre-existing; ignore.

## Manual .bat harness run (2026-10-12 21:18) -- analysed
User ran `WalrusTestHarness.bat` > `WalrusTestHarness.txt`. Result: WALRUS IS
WORKING; all failures were HARNESS bugs, now fixed in the .bat:
- NO "provided value is too long" anywhere (long-path fix confirmed on the real
  tree); robocopy copied 123 files, 0 FAILED.
- The MISSING/EXTRA flood = compare pointed DIR2 at %WORK0000% but extract writes
  the tree under %WORK0000%\Base\... (walrus roots stored names at the source
  dir's last segment). Fixed: every compareDir call now targets ...\Base.
- The Work0000\Work0000\Base\... self-nesting = `set "TEMPDIR==..."` double `=`
  made rmdir/cd target a bogus path so cleanup never ran. Fixed (+ mkdir/cd /d).
- Also fixed: step 6 re-adds %BASE% not %WORK0000% (idempotency); step 9 change
  script now recurses %WORK0000%\Base and only rewrites files containing 'tar'.

OPEN QUESTION for next run (step 7): it does `fc /b` on two .wal files after a
no-op re-add of identical content, expecting byte-identical. If that FAILS it is
NOT a harness bug -- investigate whether walrus's no-change add is byte-
deterministic, especially now that Change 3 stamps source-file mtime. Could be a
real walrus-core finding (determinism vs mtime stamping). Ask user to re-run and
share the result before assuming.

## Harness report feature + 18-step run (2026-10-12 ~22:12) -- DONE + triage
WalrusTestHarness.bat now (a) prints a [STEP N] banner before each step, and
(b) writes a Markdown PASS/FAIL table to %TEMPDIR%\WalrusTestHarness-Report.md
(H1 + metadata list + GFM table + ## Totals; table pipes escaped ^|; no parens
in echoed text) via
:report / :reportnz / :manual subroutines (goto-based, NO parens in echoed text
-- parens in an if()else() body crashed cmd with "( was unexpected at this
time"; fixed). copy /Y replaced the hanging xcopy. Final CLEAN single-process
run: PASS=10 FAIL=2 (OVERALL FAIL).

TRUSTWORTHY ONLY FROM A SINGLE RUN: running the .bat several times concurrently
leaves multiple cmd.exe writing the SAME report -> garbled/out-of-order lines
and bogus rc=2s. Kill stray cmd (Get-Process cmd | Stop-Process) and run ONCE.

PASS (10): steps 5, 7a, 7b, 8, 10, 12, 13, 17, 18. Notably 7b PASS = no-op
re-add IS byte-deterministic (the determinism question is answered: OK), and 13
= .wbl round-trip OK.

TRIAGE RESOLVED (2026-10-12 ~23:03) -- all 3 were HARNESS bugs, walrus is CORRECT:
- STEP 16: findstr /X was fragile; changed to `findstr /L /C:"README.md"`
  (literal substring). Verified walrus query prints exactly "README.md".
- STEP 14: the description text contained "->", and ">" is a cmd REDIRECTION
  operator inside the :report echo, so the row silently redirected to a stray
  file and never appeared. Changed "->" to "to". (convert + extract both work:
  verified CONVERT=0 EXTRACT=0 in isolation.)
- STEP 11: harness bug, NOT walrus. Step 11 extracted into %WORK0001% which
  already held step 8's PRISTINE extract; re-extracting mutated current over a
  populated dir left stale files. Fixed: extract into a FRESH %WORK0001%-cur.
  Minimal isolated repro (fresh dir) was byte-identical -> walrus add+extract-
  current is correct.
- STEP 15 (MANUAL): same ">" redirection trap -- verify text had "Versions > 0";
  changed to "nonzero Versions count". MANUAL row now renders.
LESSON: never put >, <, |, & in :report/:manual description/expectation args --
they are cmd metachars in the echo. Grep guard: no such chars remain.

FINAL CLEAN RUN: 12 PASS, 0 FAIL, Overall PASS, + STEP 15 MANUAL row present.
Report is Markdown at <TEMPDIR>\WalrusTestHarness-Report.md.
Harness + report feature COMPLETE and all automated checks green.

COMMITTED + CROSS-PLATFORM (2026-10-12, main 25b4568, pushed): the manual
harness now lives in the repo at tests/acceptance/TestData/ with BOTH platforms:
- WalrusTestHarness.bat + compareDir.bat (Windows/cmd)
- WalrusTestHarness.sh  + compareDir.sh  (Linux/macOS/bash -- faithful ports;
  robocopy->find+cp, fc->cmp, copy->cp, findstr->grep -Fxq, pwsh sed-mutation;
  REPO/WALRUS/TEMPDIR overridable via env; REPO inferred from script location)
- README.md explaining both platforms + relationship to the Python suite
- .gitattributes: *.sh eol=lf, *.bat eol=crlf (verified via git ls-files --eol)
compareDir kept SEPARATE from the harness (reusable one-job utility), per
decision. The .sh passed `bash -n` syntax check; it could NOT be executed on
this Windows box (only a stripped busybox bash with no POSIX path mapping is
present) -- it needs a real Linux/macOS run to confirm end-to-end. Cleaned stale
cruft: removed tests/acceptance/TestData/Base/, WalrusTestHarness.txt, and a
stray walrus_cli/WalrusTestHarness.bat.

OPEN (next session): run WalrusTestHarness.sh on a real Linux/macOS host to
confirm the port executes (syntax is verified; execution is not). Also still
offered: adding harness steps 19-24 (update/multi-version relative addressing/
remove/condense/cat/--at) and the Python TCR readable test-centric report.

## Standing follow-ups (not started, user's call)
1. Re-run `tests\acceptance\TestData\WalrusTestHarness.bat` to confirm the real
   round-trip now completes + compares clean (the original blocker).
2. walker -> Archive migration: clears the ONE known-red integration test on
   main (`crates/walker/tests/integration.rs` uses legacy `WalrusFile::open`
   on the regenerated multi-file fixtures -> `MissingMember("base")`).
   This is WALKER-GUI stream work (`.worktrees/walker-gui`), NOT walrus-core.
3. `walrus --version` flag: TCR records the CLI has no version (falls back to
   git short-sha). Small walrus-cli follow-up.
4. Optional: input-shape stress tests (long paths, deep nesting, max-length
   names, odd-but-legal chars) to make the acceptance suite exhaustive over
   INPUT shape, not just feature list -- this is why the long-path bug slipped
   past the original suite.

## Key environment traps (carry forward)
- pwsh tool intermittently returns Exit Code -1 with real output present, and
  sometimes swallows stdout. Trust printed output / `.gate` log files over the
  exit code. Route commands to `.gate\*.log` with an `EXIT=` sentinel and read
  back. One command per invocation; no `;`-chaining.
- Always absolute paths: `git -C <abs>`, `cargo ... --manifest-path <abs>`.
- Stream boundary: walrus-core work must NOT touch `crates/walker` or
  `.worktrees/walker-gui`, and vice versa. Scope cargo with `-p`.
- Known-red walker test on main is EXPECTED until follow-up 2; scoped
  `-p walrus-fmt/wal-cli-common/walx/walrus-cli` runs stay green.

## How to re-run the acceptance TCR
```
cargo build --manifest-path c:\workspace\Walrus\Cargo.toml -p walrus-cli
$env:WALRUS_BIN="c:\workspace\Walrus\target\debug\walrus.exe"
python c:\workspace\Walrus\tests\acceptance\walrus_cli\run_tcr.py
# then read c:\workspace\Walrus\.gate\tcr\walrus-cli-TCR.md (run_tcr has no stdout)
```

## CLI glob self-expansion + harness update -- DONE (2026-10-13, main 58f7619, pushed)
- 4a34e49: feat(walrus-cli) self-expand wildcard file inputs for add/update via
  the `glob` crate in wal-cli-common (CLI-ONLY; walrus-fmt dep tree unchanged,
  invariant 2 held). Spec-first commit f3cbd0d added CLI-DESIGN section 5.4f:
  no-match -> RC 2; an existing literal path wins (never re-globbed, so a POSIX
  shell's pre-expansion is not double-expanded); `*` stays single-level, `**`
  recurses; backslash/drive-qualified patterns normalized. `add *.md` now works
  identically on Windows (cmd, no shell glob) and POSIX. 7 Rust unit tests +
  Python test_glob.py (5 new CLI-DESIGN-5.4f-* ids). TCR 121 tests, 120 pass,
  0 fail, verdict PASS. CLI-DESIGN spec change FLAGGED for owner review.
- 58f7619: test(harness) corrected the stale .bat step-3 comment that claimed
  walrus rejects a literal `*` (os error 123) -- no longer true. Added STEP 20
  to BOTH WalrusTestHarness.bat and .sh: `add glob.wal *.md` with a LITERAL
  pattern stores exactly the matching .md files (20a: add rc 0; 20b: 3 stored
  names). Verified by hand against the built binary: add rc 0, query lists
  a.md/b.md/c.md, d.txt excluded. Step 3 still uses -R on the directory for the
  whole-tree round-trip (a bare *.md would only grab top-level files).

## RECURRING INFRA ISSUE (flag to user / investigate)
All FOUR workflow runs this session (wc-listing earlier, py-scale, deep-history,
cli-glob) STALLED at the finalization boundary -- either mid-code-step before
running the full suite, or after committing but before the ff-merge/push. Each
time the actual WORK completed correctly; only the runner's finalization hung
(agent-restart pause pattern). Recovery each time: inspect_workflow + filesystem
liveness to confirm work done, read the .gate evidence/TCR, abort the stalled
run, then commit/verify/merge MANUALLY. This is now a consistent systemic
finalization failure in the workflow runner, not a one-off -- worth addressing
before the next big piece of work.

## MERGE MECHANIC REMINDER (still true)
The main checkout c:\workspace\Walrus is on `docs/office-container-planning`
(another stream), NOT `main`, and is divergent (ahead 2 / behind main). So
`git merge --ff-only` into the working tree FAILS. Advance main via the ref:
`git -C <repo> fetch . <branch>:main` then `git push origin main`. Delete merged
feature branches with `-D` (because `-d` checks against the checked-out branch,
not main) ONLY after verifying `rev-parse main` == branch tip.

## STILL QUEUED (confirmed earlier, not started)
The "test base .wal" fixture: snapshot the real git history of the top-level .md
files (FORMAT/README/CHANGELOG/CONTRIBUTING/ROADMAP/CLI-DESIGN) into a committed
fixture under tests/fixtures/, merge their timelines, replay into one multi-file
.wal stamping each version with its real commit date, then a Python test asserts
`extract --at <date>` returns the exact historical state at points across the
~4-day span. ~20-28 interleaved versions. This is the realistic timestamp-
addressing test that the synthetic deep-history test had to skipTest. Generator
reads live git ONCE; the test replays the committed fixture (deterministic).

## --at RESOLUTION SEMANTICS (verified 2026-10-13, important for the fixture test)
Confirmed how `walrus cat/extract --at <T>` resolves, against the built binary +
CLI-DESIGN 5.2.1 / FORMAT 5.3:
- `--at` matches CHANGE RECORDS (splices), and picks the version whose record is
  the NEWEST at-or-before T.
- The ORIGINAL/base version has NO change record and NO ts, so `--at` never
  resolves to the base by timestamp.
- A T BEFORE the earliest *record* resolves to CURRENT (index 0), NOT to the base.
- Mechanism to pin per-version stamps in a black-box test: `os.utime(src_file,
  (epoch, epoch))` BEFORE each add/update -- walrus stores the source mtime as the
  record ts (proven by test_query.py::test_long_lastwrite_is_the_source_file_mtime).
Probe (forward .wal, one file, add@2020 then update@2023 then update@2026):
  --at 2024 -> the 2023 version (V2)   [newest record <= 2024]
  --at 2022 -> CURRENT (V3)            [before earliest RECORD; 2020 was the base add, no record]
  --at 2027 -> CURRENT (V3)
  --at 2019 -> CURRENT (V3)
So the deep-history test's "cannot pin per-version timestamps" skip was overcautious
-- os.utime CAN pin them. The fixture test must assert --at against UPDATE stamps
(not the add/base time) and expect before-earliest-update -> current. This is all
to-spec; NO walrus bug here.

## Session finish (2026-10-13 cont.) -- walrus-core wrap-up, main = 46a2116
Checkout c:\workspace\Walrus is now ON `main` (switched off docs/office-container-
planning after confirming no uncommitted feature work; the other stream can
re-checkout its branch). Fresh glob build in target/debug. Three items landed:
- 337499a: fixed the flaky test_convert (id()-collision extract-dir bug; 7
  consecutive clean runs). THIS WAS the "flaky test_convert" -- now resolved.
- 6d008bb: ROADMAP.md status refresh -- multi-file format rework marked DONE
  (steps 1-8), walker migration is the one outstanding step 9 (separate stream),
  small walrus-core follow-ups listed. The old "not started" framing was stale.
- 46a2116: test_at_timeline.py -- the realistic --at timestamp-addressing test
  the deep-history test had to skip. Builds a multi-file .wal with known
  spread-out per-version stamps (os.utime before each add -R) and asserts
  extract --at <T> gives each file's newest-record-at-or-before-T content;
  before-first-record -> current. 2 tests, both PASS.
TCR now: 123 tests, 122 pass, 0 fail, 1 skip (deep-history mid-at placeholder),
0 uncovered, verdict PASS.

## WALRUS-CORE REMAINING (small, optional)
- `walrus --version` flag (TCR finding; identity falls back to git short-sha).
- Pre-existing clippy manual_str_repeat lint at crates/walrus/src/archive.rs
  (surfaces under cargo clippy --all-targets).
- Optional: a FULL real-git-history fixture (snapshot the .md files' actual git
  history into a committed fixture + generator) -- discussed but NOT built; the
  os.utime synthetic-timeline approach in test_at_timeline.py covers the --at
  behavior deterministically without the fixture bloat. Only build the real-git
  fixture if authentic content/dates are specifically wanted.

## WALKER (separate stream, user driving in another chat)
- Known-red: crates/walker/tests/integration.rs:32 uses legacy WalrusFile::open
  on the regenerated multi-file fixtures -> MissingMember("base"). The one
  failure in `cargo test --workspace` / `cargo gate`.
- walker-gui is ~13 commits behind main; needs to merge current main and migrate
  WalrusFile::open -> Archive API. In-flight FR-K6 (done), FR-K11 Automerge +
  git-source history source (branches: walker-gui, walker-automerge,
  walker-git-source). All walrus-core APIs walker needs are on main.

## walrus --version -- IMPLEMENTED + manually proven, TESTS UNRUN, NOT COMMITTED
User hit `walrus --version` -> `unknown option '--version'` (the TCR finding).
Implemented the flag in crates/walrus-cli/src/dispatch.rs:
- added wants_top_level_version() (mirrors wants_top_level_help; matches -V and
  --version before any `--`; -v is NOT version, reserved for future --verbose),
- added print_version() -> prints "walrus {CARGO_PKG_VERSION}" to STDOUT,
- wired into run_with() AFTER the help check (help wins when both present), RC 0,
- added a #[cfg(test)] mod tests with 5 tests (long/short detect, -v-not-version,
  after-`--` not treated as flag, run_with RC 0, help-precedence).
STATUS:
- cargo build -p walrus-cli = clean (BUILD=0).
- RAN THE BINARY (proven working): `walrus --version` -> "walrus 0.1.0" RC 0;
  `-V` -> same; `--help --version` -> help banner wins RC 0. The user's bug is
  FIXED behaviourally.
- NOT YET DONE (terminal went unresponsive mid-session -- empty output, logs not
  writing, plus a cargo build-dir file-lock from a lingering process):
  1. run `cargo test -p walrus-cli` to WITNESS the 5 new unit tests pass (code
     compiles; logic is trivial; but not seen green yet -- do not skip this).
  2. regenerate the TCR: run_tcr.py's collect_binary_identity ALREADY handles a
     working --version (returncode 0, no "USAGE", non-empty) -> it will pick up
     "walrus 0.1.0" and DROP the no-version finding automatically; no code change
     to run_tcr.py needed. Confirm the finding is gone + totals.
  3. COMMIT (staging ONLY crates/walrus-cli/src/dispatch.rs) + push to main.
     The change is uncommitted in the working tree on `main` right now.
- If a cargo "Blocking waiting for file lock on build directory" appears, a
  stray cargo/rustc from a timed-out run holds it; wait or Stop-Process the
  stray, then retry.
NEXT SESSION FIRST STEP: `cargo test -p walrus-cli` (confirm green), run_tcr.py
(confirm finding gone), then commit dispatch.rs. Everything else this session is
already committed+pushed (main was at d7b7253 before this uncommitted change).

## walrus --version -- DONE + a regression caught and fixed (main a28d95a)
RESOLVED the user's `walrus --version -> unknown option`. Sequence:
- d79edd2: added --version / -V. BUG: it scanned the whole argv for --version,
  which COLLIDED with the existing extract/cat SELECTOR
  `--version <current|original|+/-n>`. `extract <arc> --version original` then
  printed the version and exited 0 instead of extracting. The walrus-cli unit/
  integration tests PASSED (they don't hit that selector combo), but the PYTHON
  acceptance suite caught it: 9 regressions (6 fail + 3 error). This is a strong
  reminder: run the FULL Python suite / TCR after a dispatch/arg-parsing change,
  not just `cargo test -p walrus-cli`.
- a28d95a: fixed -- top-level --version/-V is honored ONLY as the FIRST token
  (walrus --version with no op); a --version after a subcommand is left to the
  selector parser. Added a regression-guard unit test. Also cleaned a pre-
  existing manual_flatten clippy lint in input_glob.rs.
VERIFIED CLEAN: walrus-cli unit tests (19) green incl. the new dispatch tests;
wal-cli-common (25) green; clippy -p walrus-cli -p wal-cli-common -D warnings
clean; Python TCR = 123 tests, 122 pass, 0 fail, 0 errored, 1 skip, 0 uncovered,
Findings: none (the no-version finding is GONE -- TCR now shows
"Walrus version: walrus 0.1.0"), verdict PASS. `walrus --version` -> walrus 0.1.0.

## main now also has WALKER (merged from the other chat)
Observed in git log: 25f7bc0 "merge: Walker GUI frontend -- Phase 1 (FR-K1..K10)
+ Phase 2 Automerge (FR-K11)" landed on main. So main now carries both the
walrus-core work and the walker frontend. (Did not verify walker's state here;
that is the other chat's stream. If the known-red walker integration test was
addressed as part of that merge, `cargo test --workspace` may now be green --
worth re-checking.)

## walrus-core status: effectively complete
All planned walrus-core items done + merged. Remaining is only the pre-existing
clippy manual_str_repeat lint at crates/walrus/src/archive.rs (surfaces under
cargo clippy --all-targets; not addressed). Office-container is parked as an
approved proposal at docs/proposals/office-container/ (planning only).

## clippy --all-targets now CLEAN across the whole workspace (main bac0b4c)
User ran `cargo clippy --all-targets -- -D warnings` and saw 3 lints:
- input_glob.rs manual_flatten: was a STALE BUILD CACHE -- the source already had
  the flatten fix (a28d95a); a fresh clippy run does not show it.
- archive.rs:1107 manual_str_repeat + manual_repeat_n (the long-deferred lint,
  now flagged twice by rust 1.98): FIXED in bac0b4c -- replaced
  std::iter::repeat('\u{00E9}').take(50).collect() with "\u{00E9}".repeat(50)
  in the bound_tail_bytes test fixture (behaviour identical, 100 bytes).
VERIFIED: `cargo clippy --all-targets -- -D warnings` = CLIPPY=0 across all five
crates (walrus-fmt, wal-cli-common, walker, walrus-cli, walx) + all test targets.
walrus-fmt tests 151 pass. So the whole workspace is now clippy-clean -- the last
walrus-core housekeeping item is cleared.

NOTE for next time: when clippy flags something you believe is already fixed,
suspect a stale build cache first -- a fresh `cargo clippy` (or touching the
file) clears it; the source is the authority.

## MILESTONE: FULL GATE CLEAN (2026-10-14, main f33742e)
Owner ran `cargo gate --build` -> CLEAN: 533 run, 533 passed, 0 failed.
All stages green: fmt ok, clippy clean, build ok, test 533/533, app-build ok
(the GUI binary builds -- app-build only runs when the gate is otherwise clean,
so this is the first fully-green gate). Independently confirmed: `cargo fmt --
--check` clean, `cargo clippy -p walker --all-targets -D warnings` clean.

SIGNIFICANCE:
- The known-red walker integration test (MissingMember("base"), legacy
  WalrusFile::open vs multi-file fixtures) is GONE -- walker was migrated to the
  Archive API by the walker stream (other chat). Both streams have CONVERGED on
  main; the whole workspace is green end to end.
- Nothing outstanding on the walrus side. walrus-core is complete + clean; walker
  is merged + passing; office-container is a parked proposal
  (docs/proposals/office-container/); fmt/clippy/tests all green.
- main == origin/main == f33742e.

Remaining ideas are OPTIONAL/future only (no open defects):
- office-container feature implementation (approved design, fresh worktree off
  main when started, spec-first via format-change-proposal.md).
- optional real-git-history --at fixture (synthetic timeline test already covers
  the behaviour).

## WALKER OPEN BUG FIXED (2026-10-14, main 5726a00, pushed)
User reported: opening a valid multi-file archive in the GUI failed with
"Failed to open ...Work0000.wal: missing required container member: base".
ROOT CAUSE: `crates/walker/src/app.rs::open_walrus` still called the LEGACY
single-file `WalrusFile::open` purely to sniff the archive variant for the
status bar. `WalrusFile::open` cannot read a multi-file archive (it expects a
single top-level `base` member) and errored with MissingMember("base") BEFORE
the correct `WalrusSource::open` (Archive-based, multi-file aware) ran. The
archive itself was fine -- `walrus query ...Work0000.wal` returned RC 0.

WHY THE FULL GATE (533/533) MISSED IT: the walker integration test exercises
`WalrusSource`/`Archive` directly; nothing drove the GUI `open_walrus` entry
point, so the vestigial `WalrusFile::open` call there was never executed by a
test. `WalrusFile::open` is SUPERSEDED by `Archive::open`; its last walker use
was this variant sniff.

THE FIX (commit 5726a00, 2 files, +73/-11):
- crates/walker/src/source/walrus.rs: added `pub fn variant(&self) -> Variant`
  returning `self.archive.manifest().variant`, and added `Variant` to the
  `walrus_fmt` import. This keeps the FR-K1 AC2 boundary -- only
  `source/walrus.rs` names walrus_fmt archive types; the app shell reads the
  variant through this accessor.
- crates/walker/src/app.rs: `open_walrus` now opens `WalrusSource` first (the
  single fail-fast point) and derives `let variant = read.variant();` from that
  same open source. Removed the `WalrusFile::open` block and dropped `WalrusFile`
  from the import (now `use walrus_fmt::Variant;`).
- Added two regression tests in source/walrus.rs: variant() on a single-file
  forward container returns Wal; and (the exact shape that broke the GUI) a
  two-file MULTI-FILE archive opens as a WalrusSource and reports its variant --
  closing the GUI-path test gap.

VERIFIED (scoped, all clean):
- cargo check -p walker: CHECK_EXIT=0.
- cargo test -p walker: all pass -- 159 lib unit tests incl. the two new
  variant_* tests, + file_controls 7, file_controls_ui 6, harness 5,
  history_view 3, import 7, integration 1. Zero failed.
- cargo clippy -p walker --all-targets -- -D warnings: CLIPPY_EXIT=0.
- cargo fmt --all -- --check: FMT_EXIT=0.
- cargo build -p walker: BUILD_EXIT=0 (fixed walker.exe now in target/debug;
  the earlier "Access is denied" on walker.exe was a running GUI instance
  holding the exe lock, NOT a compile error).
Committed (staged ONLY the two code files) + pushed: f33742e..5726a00 main.

STILL OWNER-MANUAL: launch the GUI and open C:\workspace\Walrus-Temp-TestData\
Work0000.wal to confirm end-to-end (OS-native rfd dialog / real window can't be
driven headlessly). The fixed code path is the same WalrusSource/Archive path
the passing integration tests and the working `walrus query` CLI use. Owner may
also re-run the full `cargo gate --build` for the two-staged "done" signal
(empty .gate/gate.review.log).

---

## POST-REORG VERIFICATION (2026-10-15) -- READ BEFORE RESUMING

The workspace was reorganized and a copy script ran out of disk space mid-run.
This project was moved to `c:\workspace\apps\Walrus` (was `c:\workspace\Walrus`).
A read-only verification pass (`_steering\verify-repos.ps1`) confirmed:

- Repo is INTACT. `HEAD = main`, tip `3b6916a` ("test(walker): drive the real
  WalkerApp shell headlessly ...").
- Uncommitted work PRESERVED: modified `.kiro/steering/background-process-status.md`
  and `.kiro/steering/shell-and-gate.md`; untracked `.agents/RESUME-NEXT-SESSION.md`,
  `.kiro/hooks/`, `tools/`, `tests/acceptance/TestData/WalrusTestHarness.txt`.
- `git fsck` shows only dangling objects (normal, not corruption).

CHANGED SINCE THE LAST HAND-OFF ABOVE: `git worktree list` now shows ONLY
`c:\workspace\apps\Walrus [main]`. The other-stream worktrees this file mentions
(walker-gui, walker-automerge, walker-git-source, walrus-core,
docs/office-container-planning) are NO LONGER registered -- the reorg copied only
the main working tree, not the `.worktrees\` siblings. The BRANCHES themselves
are unaffected (they live in `.git`); only the linked worktree checkouts are
gone. Re-add any you still need with `git worktree add .worktrees/<name> <branch>`.

FIRST STEP THIS SESSION (local verification before continuing):
    git -C c:\workspace\apps\Walrus status
    git -C c:\workspace\apps\Walrus worktree list
    git -C c:\workspace\apps\Walrus log -1
Confirm tip == 3b6916a and the uncommitted items above are present, then proceed.

---

## SESSION 2026-10-15 (autonomous) -- status audit + ROADMAP forward plan

User asked for a project-status audit and a forward plan in ROADMAP, then went
offline. No git commits were made (docs edits left in the working tree for the
owner to review). Work done:

### Verified current state (not just trusted the labels)
- Git: `main` == `origin/main` == `3b6916a`. Working tree has the same
  pre-existing uncommitted items as the last hand-off (two steering files,
  untracked `.agents/`, `.kiro/hooks/`, `tools/`, harness `.txt`) PLUS my docs
  edits this session (README.md, ROADMAP.md).
- `cargo build` (whole workspace): CLEAN (rc=0).
- `cargo test -p walrus-fmt --lib`: 162 passed / 0 failed (authoritative green;
  includes all the search matcher + line_col tests). Core engine is solid.
- The SEARCH feature is SHIPPED on main (not WIP as the old ROADMAP claimed):
  `crates/walrus/src/search/{mod,line_col}.rs`, `wal-cli-common/src/search.rs` +
  `search_render.rs`, `cmd_search.rs`, and tests in both CLI crates all present.
- Audit follow-ups still OPEN and unfixed: README `WalrusFile` stale example
  (NOW FIXED, see below); 3 NFR-1 line-cap breaches (archive.rs ~440,
  source/mod.rs ~472, app.rs ~535) all still present; per-frame reconstruct;
  the HIGH walker-shell test gap. Office-container is still planning-only.

### ENVIRONMENT FINDING (not a code defect) -- carry forward
A whole-workspace `cargo test` on THIS box crashes `rustc` with OOM
(`memory allocation ... failed`) and `STATUS_STACK_BUFFER_OVERRUN` (0xc0000409)
while compiling the heavier egui/eframe test + example targets in parallel. The
downstream `can't find crate for walker/walrus_fmt` errors are a CONSEQUENCE of
rustc crashing mid-compile, not real missing-crate bugs. `cargo build` and
scoped `-p walrus-fmt --lib` succeed, and the owner's `cargo gate --build` was
533/533 at this commit, so this is memory pressure, NOT a regression. Mitigation:
run the full suite with lowered parallelism (`-j 1`/`-j 2`) or crate-by-crate.
Recorded this in ROADMAP's "Going forward -> ENVIRONMENT NOTE".
Terminal was also in heavy swallow/dead-episode territory this session (many
`-1` with no sentinel); routed through `.gate\resume-*.log` and read back.

### Docs updated (UNCOMMITTED, in working tree for owner review)
- ROADMAP.md: added a dated "Going forward -- prioritized plan" section near the
  top (Tier A correctness+docs, Tier B hygiene/NFR, Tier C optional features,
  plus a "suggested first move"). Corrected the stale "SEARCH feature -- WIP"
  section to "DONE (shipped on main)". Added the search feature to the top
  STATUS line. ASCII-clean (verified).
- README.md: rewrote the "Using the library" example off the removed
  `WalrusFile` onto `Archive`/`FileId`/`Version` (mirrors
  crates/walker/tests/integration.rs; field names verified against manifest.rs:
  TrackedFile.id/.name/.versions.count). Updated the top status blockquote, the
  fixtures parenthetical, and the bottom "## Status" to drop "mid-migration".
  ASCII-clean (verified).

### Suggested next action (for the owner / next session)
Tier A item 1: a headless `egui_kittest` test that drives the REAL `WalkerApp`
shell (open -> select -> seek -> render over a temp multi-file `.wal`), asserting
player size / active selection / `current_bytes == reconstruct(file, Index(0))`.
This closes the exact gap that twice let a GUI bug ship green. Pair with the
cheap `source/mod.rs` testsupport-gating NFR fix. All walker-stream, self-
contained. Consider committing these doc edits first.

---

## SESSION 2026-10-16 cont. -- Tier A walker shell test: CODE DONE, VERIFY BLOCKED

Workflow wf_3e54ec6fc68f0a27 (walker-app-shell-test) ran in worktree
`.worktrees\walker-app-shell-test` (branch feature/walker-app-shell-test, cut
from main 3b6916a). The workflow PAUSED (agent-restart interruption) with the
code step mid-flight, AFTER writing files but BEFORE committing/final verify.

### State of the worktree (UNCOMMITTED, verified by reading diffs)
- `crates/walker/src/app_mutate.rs`: ONE-LINE PRODUCTION BUG FIX applied.
  `apply_import` now writes to `self.path` (the opened archive) with a
  `let Some(path) = self.path.clone() else { return; };` guard, matching
  `handle_file_action`, instead of the old `&outcome.path` (the user-picked
  SOURCE file). The old code wrote the mutated archive OVER the picked file and
  left the opened archive untouched -> imports were invisible in the GUI AND
  clobbered the picked file. Fix verified correct by reading the diff + the
  WalrusSource::import contract (its `path` param IS the archive write target)
  + ImportOutcome.path doc ("the source path the bytes came from").
- `crates/walker/src/app.rs`: 4 new inline #[cfg(test)] tests with // Validates
  comments: apply_import add/update through the shell, handle_file_action
  remove/set_generations through the shell, + an open_shell_over_canonical
  helper. (Points 1-3 of the task were ALREADY on HEAD 3b6916a -- that commit's
  message is literally "drive the real WalkerApp shell headlessly"; this run
  adds only the import/mutate SHELL coverage, point 4.)

### Verification status: NOT yet confirmed on the FIXED tree
- The coder's pre-restart log `.gate/app-shell-test2.log` shows 165 passed /
  2 failed -- the 2 failures are the apply_import add+update tests, which is the
  BUG BEING CAUGHT (pre-fix: re-opened notes.wal unchanged -> "notes.txt" still
  selected / version count 3 not 4). That log PREDATES the fix.
- I could NOT run `cargo test -p walker --lib` against the FIXED tree this
  session: the terminal fell into a long dead/swallow episode (commands not
  executing -- even file-writing probes left no file). This is the documented
  box instability, not a code problem.
- LOGICAL analysis: the fix writes to exactly the archive (notes.wal) the two
  failing tests re-open and assert against, so it directly resolves both
  assertions (app.rs:994 selection, app.rs:1071 version count). Expected result
  on the fixed tree: 167 passed / 0 failed. BUT THIS MUST BE RUN TO CONFIRM --
  do not commit/merge on the logical argument alone.

### NEXT STEP (resume here)
1. When the terminal is healthy, run (serial -j1 to avoid the egui/eframe
   parallel-compile OOM on this box):
   `cargo test -p walker --lib --manifest-path <worktree>\Cargo.toml -j 1`
   route to `<worktree>\.gate\verify-fix-lib.log`, read back; expect
   167 passed / 0 failed, specifically the 2 apply_import tests now PASS.
2. Then `cargo clippy -p walker --all-targets ... -D warnings` and
   `cargo fmt -- --check` (both scoped to the worktree manifest).
3. If clean: commit ONLY the two files
   (`git -C <worktree> add crates/walker/src/app.rs crates/walker/src/app_mutate.rs`)
   with a message like
   "fix(walker): persist imports to the opened archive, not the picked source file"
   that calls out BOTH the production fix and the new regression tests.
4. Rebase onto main + ff-merge from the user's main checkout:
   `git -C <worktree> rebase main` then
   `git -C c:\workspace\apps\Walrus merge --ff-only feature/walker-app-shell-test`.
5. Remove the worktree:
   `git -C c:\workspace\apps\Walrus worktree remove .worktrees/walker-app-shell-test`
   then delete the branch once rev-parse confirms main == branch tip.
6. The paused workflow wf_3e54ec6fc68f0a27 can be aborted (its work is done
   manually) OR resumed to let its merge step run -- but given the restart
   stalls, finishing manually is cleaner. Update ROADMAP Tier A item 1 -> DONE
   and record the apply_import bug fix.

### Owner's independent signal (good news, orthogonal)
Owner did a full cargo clean + rebuild + retest + ran WalrusTestHarness.bat:
PASS=15 FAIL=0 Overall PASS (round-trips both variants, convert, determinism,
version addressing, recursion-refusal, glob, Python acceptance STEP 19). That
exercises the CLI binary, NOT the walker GUI, so it is orthogonal to the
apply_import fix -- but it confirms the CLI/format core is healthy end to end.

---

## SESSION 2026-10-16 cont. -- Search test coverage added (harness + Python)

User asked to add search coverage to BOTH the manual test harness and the
Python acceptance suite. Implementation DONE; final run-verification BLOCKED by
a long terminal dead/wedge episode (see env note above). Nothing committed.

### Files added/edited (uncommitted, on main checkout c:\workspace\apps\Walrus)
- NEW `tests/acceptance/walrus_cli/test_search.py`: 9 tests over the built
  walrus binary, mirroring crates/walrus-cli/tests/search.rs contract exactly:
  literal-found (RC 0 + Markdown header "Search report"/"Version index: 0"/
  "Version timestamp:" + "| Line | Col | Byte offset | Match |"), no-match
  (RC 1), case-insensitive -i, --format plain (omits table), file-mask
  restriction, two-selectors (RC 2), index-beyond-last (RC 4), --regex, and a
  read-only guard (archive bytes unchanged after search). Fixture built in-test:
  notes.txt ("cats"/"Cats") + src/main.rs ("main"), added via `add <work> -R
  --wal` so stored names are notes.txt and src/main.rs (same -R storage pattern
  proven in test_glob.py).
- EDITED `tests/acceptance/walrus_cli/requirements.py`: registered 9 new ids so
  the TCR tracks them (no coverage gap): CLI-DESIGN-5.6-search-found/-nomatch/
  -ignore-case/-plain/-mask/-selector/-beyond-last/-regex and CLI-DESIGN-6-rc1
  (the search no-match RC-1 case; RC 1 was not previously in the rc0/2/4/8 set).
- EDITED `tests/acceptance/TestData/WalrusTestHarness.bat` and `.sh`: new STEP 21
  with three sub-checks against pristine $WORK0000.wal (the real .md/.rs tree,
  certain to contain "walrus"): 21a found "walrus" -> RC 0; 21b absent
  "zzqqxxnotpresent" -> RC 1; 21c "-i WALRUS" case-insensitive -> RC 0. Uses the
  existing :report/report helper; NO cmd metacharacter in any description text
  (return code is the sole pass criterion); search output routed to
  $TEMPDIR\search-*.log. All ASCII-clean (verified, no non-ASCII in
  tests/acceptance/**).

NOTE: the .bat/.sh STEP 19 already runs `python -m unittest discover`, so once
test_search.py lands it is ALSO picked up by STEP 19 automatically -- the new
STEP 21 is the DIRECT CLI-level harness coverage, test_search.py is the
acceptance-suite coverage, and STEP 19 ties them together.

### VERIFICATION STATUS: NOT yet run on this box (terminal wedged)
- walrus-cli binary WAS rebuilt clean this session (.gate/search-build.log:
  "Finished dev profile ... in 32.24s", rc 0) after the owner's cargo clean.
- test_search.py was NOT yet executed: every attempt to run
  `python -m unittest test_search` (foreground AND managed background) failed to
  launch -- the terminal fell into an extended dead episode (no log/marker file
  produced). This is the documented box instability, not a test error.
- Assertions were cross-checked by READING against the shipped Rust test
  crates/walrus-cli/tests/search.rs (identical header/table literals + RCs) and
  the proven -R storage behaviour in test_glob.py, so confidence is high, but
  THEY MUST BE RUN to confirm. The residual unknowns are purely fixture-name /
  report-header-format details the Rust test already pins.

### NEXT STEP (resume here)
1. When the terminal is healthy, run (binary already built):
   `$env:WALRUS_BIN="c:\workspace\apps\Walrus\target\debug\walrus.exe";
    python -m unittest test_search -v`
   from tests/acceptance/walrus_cli, route to .gate\search-pytest.log, read
   back. Expect 9 tests, all OK. Fix any fixture-name/header-literal mismatch
   (most likely spot: the archive-name or stored-name assertions) and re-run.
2. Optionally run the full suite + TCR to confirm the 9 new ids show COVERED and
   totals rise: `python run_tcr.py` then read .gate\tcr\walrus-cli-TCR.md.
3. The .bat/.sh STEP 21 is best verified by the owner running the harness (the
   .bat the way the owner already does); it needs the real multi-file tree. The
   logic matches the existing steps' proven patterns.
4. Then commit: test_search.py + requirements.py + the two harness scripts, msg
   like "test(search): add search coverage to the acceptance suite and harness".

---

## SESSION 2026-10-16 cont. -- Search coverage VERIFIED GREEN

The terminal recovered. Ran the new search coverage against the freshly built
walrus.exe:
- `test_search.py` alone: Ran 9 tests, OK (after one fixup -- see below).
- Full Python acceptance suite: Ran 132 tests, OK (skipped=1). Up from 123.
- `run_tcr.py` regenerated: 132 run / 131 pass / 0 fail / 1 skip; the 9 new
  CLI-DESIGN-5.6-search-* ids + CLI-DESIGN-6-rc1 all show PASS; Uncovered: none;
  Sign-off verdict: PASS. (.gate/tcr/walrus-cli-TCR.md, timestamp 15:03:36Z.)

FIXUP applied during verification: the mask test first failed because `add
<dir> -R` stores names with the recursion-ROOT basename prefix (e.g.
`wsearch/src/main.rs`, confirmed by querying a hand-built fixture), so a mask of
`src/main.rs` matched 0 files. Fixed `seed_search_archive` to add the two files
as LONE FILES (stored under basename: `notes.txt`, `main.rs`) via two
single-file adds into one archive, and changed the mask-test needle file to
`main.rs`. Re-ran: all 9 green.

ROADMAP.md updated: the SEARCH section now records the acceptance + harness
coverage and the 132/131/PASS TCR. All edits ASCII-clean.

STILL UNCOMMITTED (owner to review/commit): test_search.py, requirements.py,
WalrusTestHarness.bat, WalrusTestHarness.sh, ROADMAP.md (+ the earlier README.md
and the pre-existing steering edits). Suggested commit message:
"test(search): add search coverage to the acceptance suite and manual harness".
The .bat/.sh STEP 21 was NOT executed end-to-end here (needs the owner's real
multi-file harness run), but its logic matches the existing proven steps and the
same search RCs the Python tests confirmed.

OUTSTANDING (unchanged): the Tier A walker shell test worktree still has 1
failing test (apply_import_update refresh of current_bytes, app.rs:1085) -- a
separate stream, not touched this round.

---

## SESSION 2026-10-16 cont. -- harness STEP 21b self-reference bug FIXED

Owner ran WalrusTestHarness.bat: PASS=17 FAIL=1. The only failure was STEP 21b
(search of an "absent" string -> expected RC 1, got RC 0).

ROOT CAUSE (self-referential test bug, not a walrus bug): the harness searched
the pristine WORK0000.wal, which STEP 2 builds by robocopy-ing every *.md/*.rs
in the repo. The old absent-needle literal had been written INTO a tracked .md
(this resume note documents the harness step), so STEP 2 ingested that .md into
the archive and search CORRECTLY found the needle -> RC 0. Confirmed by
grep: the literal lived in .agents/RESUME-NEXT-SESSION.md.

FIX: both WalrusTestHarness.bat and .sh now assemble the absent needle at
RUNTIME from two halves (NEEDLE_A="qzx7" + NEEDLE_B, concatenated into ABSENT)
that exist ONLY in the .bat/.sh (excluded from STEP 2's *.md/*.rs copy), so the
whole token can never appear in the archived tree. Do NOT write the concatenated
token into any tracked .md/.rs file or the bug returns.

VERIFIED against the exact failing scenario (archive built from the repo's
.md/.rs tree via robocopy, same as the harness): the assembled token ->
absent_rc=1, and 'walrus' -> found_rc=0 (.gate/search-fix-verify.log). So STEP
21a/21b/21c are now all correct; the next harness run should be PASS=18 FAIL=0
(plus the STEP 15 MANUAL row).

The Python test_search.py no-match test is NOT affected: it builds its own
isolated fixture (notes.txt/main.rs) and searches "penguin" against THAT, not
the repo tree -- already verified green (9/9, TCR 132/131/PASS).

---

## SESSION 2026-10-16 cont. -- Tier A COMPLETION (item 1 DONE+pushed; items 2+3 code done, test-verify blocked)

### Item 1 (headless WalkerApp shell test) -- DONE + MERGED + PUSHED
- Fixed the apply_import bug (wrote the mutated archive over the user-picked
  SOURCE file instead of self.path, and never refreshed current_bytes / rebound
  the player after a mutate). Added rebind_player_to_active() in app.rs and
  called it + refresh_bytes() from refresh_after_import in app_mutate.rs.
- Owner ran `cargo gate --build` in the worktree: CLEAN 583/583, fmt/clippy/
  build/app-build all green. Committed 31ecd20, ff-merged to main, worktree +
  branch removed, PUSHED. main == origin/main == 31ecd20 (VERIFIED).

### Item 2 (README WalrusFile example) -- CODE DONE (uncommitted)
- crates/walrus/README.md: rewrote the "Using the library" example off the
  removed WalrusFile type onto Archive::open / archive.files() / FileId::from /
  reconstruct(Version::Index(0)), mirroring crates/walker/tests/integration.rs
  (verified against the real API in archive.rs). Dropped the stale
  "being implemented / in progress" disclaimer (multi-file surface has shipped).
- examples/README.md: fixed the one WalrusFile prose line (generate_story.rs is
  now an Archive mutate-API example: seed Archive, add base, update_with_splices,
  write) -- verified against the actual generator source. (Left the walx
  info/positional-index command examples alone -- out of Tier A scope.)
- (Top-level README.md was already fixed in an earlier session.)

### Item 3 (NFR-1: isolate source testsupport) -- CODE DONE (uncommitted)
- `source::testsupport` now lives in `crates/walker/src/source/testsupport.rs`;
  `source/mod.rs` exposes it only under `#[cfg(any(test, feature =
  "testsupport"))]`, keeping the coordinator below the 400-line cap.
- crates/walker/Cargo.toml: added `testsupport = []` feature + a self
  dev-dependency `walker = { path = ".", features = ["testsupport"] }` so the
  five integration-test crates under tests/ (which link walker with cfg(test)
  false) still see source::testsupport during `cargo test`.
- VERIFIED: `cargo fmt -- --check`, `cargo test -p walker -j 1` (215 passed,
  1 ignored), `cargo build -p walker`, and `cargo clippy -p walker -- -D
  warnings` all pass. Logs are under `.gate/resume-tierA-*`.

### NEXT STEP (resume here)
1. Tier A and B are full-gate clean (585/585). Review and commit only intended
   Tier A/B files if desired; the working tree contains unrelated dirty files,
   so do not stage them. No commit or push was made in this resume.
2. For Tier C, choose one optional feature and start a fresh worktree from the
   latest `main` per the branch hygiene rules.
