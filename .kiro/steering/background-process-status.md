---
inclusion: always
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
   - For a workflow: `list_directory` / check `LastWriteTime` on the step's
     artifact files in its task dir; read the newest artifact to confirm it is
     complete vs truncated.
   - For a background shell process: read its recent output
     (`get_process_output`) and confirm NEW lines have appeared since last time;
     an unchanging tail is not progress.
2. **Establish a reasonable-duration baseline.** Note how long comparable steps
   took earlier in the same run (e.g. the first design review finished in about
   a minute). If the current step has run many multiples of that with no new
   output, treat it as suspect, not healthy.
3. **Cross-check the tracker against the filesystem.** When the orchestrator says
   "running" but no artifact has changed in a span that dwarfs the baseline, the
   tracker is almost certainly stale (a dead/orphaned session the system has not
   noticed). Trust the filesystem evidence over the status string.

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
