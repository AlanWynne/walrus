---
name: "Walrus background-process-status"
description: "Use when reporting status or liveness of any workflow, build, watcher, process, or background agent."
applyTo: "**"
---

# Background Process and Workflow Status -- Verify Liveness, Do Not Trust the Report

When the user asks for a status update on ANY background activity -- a workflow
run, a background shell process, a long-running build or watcher, a dispatched
agent -- you MUST verify that it is still ACTIVELY MAKING PROGRESS. Do NOT simply
repeat what the orchestrator or the process reports about itself. A "running"
status is a claim, not proof. A step or process can be hung, dead, or
disconnected while the system that tracks it still shows "running".

This rule exists because a workflow step reported "running" for over 13 hours
while its session had actually died on an agent restart; the only way the stall
was caught was by checking the artifact file's modification time on disk against
the current clock. Reporting the stale "running" status back to the user, status
request after status request, wasted real time and eroded trust.

## The mandatory liveness check (every status request)

Before telling the user a background activity is "still running" / "in
progress", gather INDEPENDENT evidence that it is alive and advancing:

1. **Check for fresh output or artifacts.** Compare the last-write time of the
   files the activity is supposed to be producing (logs, artifacts, reports,
   edited sources) against the current time. Use the actual clock, not memory.
   A process doing work leaves recent traces; one that has written nothing for
   far longer than a step normally takes is a red flag.
   - For a workflow: use `glob` to locate the step's artifacts, then check
     their `LastWriteTime` with PowerShell; read the newest artifact with `view`
     to confirm it is complete vs truncated.
   - For a background shell process: read its recent output
     (`read_powershell`) and confirm NEW lines have appeared since last time;
     an unchanging tail is not progress.
   - The standard script for this is the liveness-probe script (see "The
     liveness-probe script" below): one invocation prints a NOW clock, the
     newest write per bucket with its age, the latest git commit, and a one-word
     VERDICT (ALIVE / SLOWING / STALLED). Use it first; it does steps 1-2 in one
     shot and works even when the terminal is flaky.
2. **Establish a reasonable-duration baseline.** Note how long comparable steps
   took earlier in the same run (e.g. the first design review finished in about
   a minute). Pass that as the probe's `-StaleSeconds` so its verdict reflects
   the right baseline; without the probe, if the current step has run many
   multiples of that baseline with no new output, treat it as suspect, not
   healthy.
3. **Cross-check the tracker against the filesystem.** When the orchestrator says
   "running" but no artifact has changed in a span that dwarfs the baseline, the
   tracker is almost certainly stale (a dead/orphaned session the system has not
   noticed). Trust the filesystem evidence over the status string.

## The liveness-probe script (run this first)

`tools\powershell\liveness-probe.ps1` is the standard liveness check. Point it
at the directory a background activity writes to (a worktree root, the main
workspace, a task dir); it scans recursively and prints a report that already
does the step-1 and step-2 reasoning for you:
- a one-word VERDICT (ALIVE / SLOWING / STALLED) with the reason;
- the newest write in each of three buckets so build churn never masks the real
  signal: WORK (source/docs/tasks/config), GATE (the `.gate\` workflow logs),
  BUILD (`target\` compiler output -- shown but NOT a liveness signal on its own);
- the latest git commit (hash, time, subject), and a note when WORK is newer
  than the last commit -- the "produced output but has not committed yet" stall
  shape this project keeps hitting;
- the top N recent files per bucket, with mtimes and ages.
It writes the same report to a timestamped log under `<root>\tools\logs\` and
echoes it to the terminal.

Prefer it over ad-hoc `Get-ChildItem | Sort-Object` one-liners: it is a single
simple invocation (no `;`-chaining, no inline `$(...)`), so it does not trip the
terminal mangling described in the shell-and-gate steering, never fails to
format a path, and even if the terminal swallows stdout the log on disk still
holds the answer. The owner can run it themselves for a quick check.

Run it (one command, its own invocation; absolute script path is cwd-safe):
```
pwsh -NoProfile -NonInteractive -File .\tools\powershell\liveness-probe.ps1 -Path .worktrees\<name>
```
- `-Path` directory to scan (default: current directory).
- `-StaleSeconds` the liveness threshold (default 180). SET IT TO THE STEP's
  baseline (step 2): a quick model step ~60, a build+test+review loop 300+.
- `-Top` recent files listed per bucket (default 10).
- `-NoGit` skip the commit check when `-Path` is not inside a repo.

The VERDICT IS the baseline comparison, so read it directly:
- ALIVE (newest WORK/GATE write <= threshold): actively writing -- report it as
  alive, citing the age the probe prints.
- SLOWING (<= 2x threshold): quiet; may be mid build/review. Do not call it
  either way -- re-probe once after a short interval before deciding.
- STALLED (> 2x threshold, or no WORK/GATE files at all): treat as hung or
  agent-restart-paused. Cross-check the tracker (step 3) and RECOVER per the
  rules below -- do NOT keep re-probing.
The exit code mirrors the verdict (0 ALIVE, 3 SLOWING, 4 STALLED) so the probe
can gate a script. It only reports; it never commits or resumes -- act on its
evidence per the recovery rules.

## Reporting rules

- State the EVIDENCE, not just the label: "still running -- the review artifact
  was rewritten 40 seconds ago" or "flagged as running, but the newest artifact
  is 13 hours old and nothing has changed, so it is almost certainly hung".
- NEVER report "it is still working" on the strength of a "running" status alone.
  If you have not checked for fresh output, say you have not yet verified, then
  verify.
- If you cannot find any evidence of progress, say so plainly and treat the
  activity as stalled rather than giving the benefit of the doubt again.

## When a stall is detected -- recover, do not keep polling

Repeatedly re-polling a hung activity at short intervals is not a status update;
it just re-reports the same stale claim. Once liveness evidence shows a stall:

1. Diagnose: identify which step/process is stuck and what it was producing.
2. Attempt the least-destructive recovery first (resume/nudge), and RE-VERIFY
   with the liveness check that it actually started producing output again --
   do not assume the nudge worked.
3. If it stays dead, escalate deliberately (pause/cancel the stuck step and
   advance or relaunch from the last good artifact), preserving completed work.
4. For anything that changes scope, dependencies, or architecture, surface the
   stall and the proposed recovery to the user rather than guessing -- but do
   surface it; silence while a process is dead is the failure this rule prevents.

## Do not confuse "quiet" with "dead", or "noisy" with "alive"

Liveness is about NEW, RELEVANT progress since the last check. A process can be
legitimately mid-step and quiet for a few minutes (a real build, a thorough
review); the baseline from step 2 is what distinguishes "working, be patient"
from "hung, recover". Equally, a process can emit heartbeat noise while making no
real progress -- look for advancement of the actual work product, not mere
output.
