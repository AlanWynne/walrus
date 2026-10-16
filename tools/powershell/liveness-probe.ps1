<#
.SYNOPSIS
    Liveness probe -- answers "is a background task still working or has it
    stalled?" by comparing the newest file write (and the latest git commit)
    under a directory against the clock, and printing a one-word VERDICT.

.DESCRIPTION
    Writes a timestamped report to a log file AND echoes it to the terminal.
    The report separates the signal from the noise by bucketing recent writes:

      WORK  -- source, docs, tasks, configs (real work product)
      GATE  -- the .gate\ workflow logs (red/green/review/build traces)
      BUILD -- target\ compiler output (churns constantly; NOT a liveness signal
               on its own, but shown so a build-in-progress is visible)

    For each bucket it reports the newest write and its age. It also reads the
    latest git commit (time + subject) so the common "work done but not committed
    yet" stall shape is obvious at a glance.

    It then prints a VERDICT based on the freshest WORK-or-GATE write age vs the
    -StaleSeconds threshold:
      ALIVE    -- newest work/gate write is within the threshold (actively writing)
      SLOWING  -- between 1x and 2x the threshold (quiet; may be mid build/review)
      STALLED  -- older than 2x the threshold, or nothing written at all
    The verdict is also the exit code: 0 = ALIVE, 3 = SLOWING, 4 = STALLED, so
    the probe can gate a script, not just inform a human.

    Run it against a worktree root, the main workspace, or any task dir. It scans
    recursively from -Path.

.PARAMETER Path
    Root to scan. Defaults to the current directory.

.PARAMETER Top
    How many recent files to list PER BUCKET. Default 10.

.PARAMETER StaleSeconds
    Liveness threshold in seconds. Newest work/gate write within this = ALIVE;
    within 2x = SLOWING; beyond 2x (or nothing) = STALLED. Default 180.
    Set it to a comparable step's typical duration for the activity you watch
    (a quick model step: 60; a thorough build+test+review loop: 300+).

.PARAMETER LogFile
    Where to write the report. Defaults to a timestamped file under the scan
    root's tools\logs\ (created if missing), falling back to the scan root.

.PARAMETER NoGit
    Skip the git-commit check (use when -Path is not inside a repo).

.EXAMPLE
    # Quick check of a worktree (absolute script path is cwd-independent):
    pwsh -NoProfile -NonInteractive -File C:\workspace\Walrus\tools\powershell\liveness-probe.ps1 -Path C:\workspace\Walrus\.worktrees\walker-gui

.EXAMPLE
    # Watch a slow build+test+review loop; treat 5 min of silence as the baseline:
    pwsh -NoProfile -NonInteractive -File C:\workspace\Walrus\tools\powershell\liveness-probe.ps1 -Path .worktrees\walker-gui -StaleSeconds 300 -Top 15
#>
[CmdletBinding()]
param(
    [string]$Path = ".",
    [int]$Top = 10,
    [int]$StaleSeconds = 180,
    [string]$LogFile,
    [switch]$NoGit
)

$ErrorActionPreference = "Stop"

# Resolve the scan root to an absolute path.
$root = (Resolve-Path -LiteralPath $Path).Path
$now  = Get-Date

# Decide the log file location. Prefer <root>\tools\logs\, create if needed.
if (-not $LogFile) {
    $stamp   = $now.ToString("yyyyMMdd-HHmmss")
    $logsDir = Join-Path $root "tools\logs"
    if (-not (Test-Path -LiteralPath $logsDir)) {
        try { New-Item -ItemType Directory -Path $logsDir -Force | Out-Null } catch { $logsDir = $root }
    }
    $LogFile = Join-Path $logsDir "liveness-probe-$stamp.txt"
}

# Directories whose writes are never a liveness signal on their own.
$vcsDirs = @('.git', 'node_modules')

# Classify a relative path into a bucket: 'build', 'gate', 'vcs', or 'work'.
function Get-Bucket([string]$rel) {
    $seg = $rel -split '[\\/]'
    if ($seg | Where-Object { $vcsDirs -contains $_ }) { return 'vcs' }
    if ($seg -contains 'target') { return 'build' }
    if ($seg -contains '.gate')  { return 'gate' }
    return 'work'
}

# Gather every file once; bucket as we go. VCS files are dropped entirely.
$all = Get-ChildItem -LiteralPath $root -Recurse -File -Force -ErrorAction SilentlyContinue
$work  = New-Object System.Collections.Generic.List[object]
$gate  = New-Object System.Collections.Generic.List[object]
$build = New-Object System.Collections.Generic.List[object]
foreach ($f in $all) {
    $rel = $f.FullName.Substring($root.Length).TrimStart('\','/')
    switch (Get-Bucket $rel) {
        'work'  { $work.Add([pscustomobject]@{ Rel = $rel; Time = $f.LastWriteTime }) }
        'gate'  { $gate.Add([pscustomobject]@{ Rel = $rel; Time = $f.LastWriteTime }) }
        'build' { $build.Add([pscustomobject]@{ Rel = $rel; Time = $f.LastWriteTime }) }
        default { }
    }
}

# Helper: age in whole seconds of a mtime vs now.
function Age-Seconds($time) { [int]($now - $time).TotalSeconds }

# Newest write across a bucket, or $null if the bucket is empty.
function Newest($list) {
    if ($list.Count -eq 0) { return $null }
    $list | Sort-Object Time -Descending | Select-Object -First 1
}

$newestWork  = Newest $work
$newestGate  = Newest $gate
$newestBuild = Newest $build

# The liveness signal is the FRESHER of newest-work and newest-gate (real work
# product), NOT build output. $null if neither bucket has any file.
$signalTime = $null
foreach ($cand in @($newestWork, $newestGate)) {
    if ($cand -and (-not $signalTime -or $cand.Time -gt $signalTime)) { $signalTime = $cand.Time }
}

# Latest git commit (best-effort). Non-fatal if not a repo or git is absent.
$gitLine = $null
$gitAge  = $null
if (-not $NoGit) {
    try {
        $gitRaw = & git -C $root log -1 --format="%cd|%h|%s" --date=format:"%Y-%m-%d %H:%M:%S" 2>$null
        if ($LASTEXITCODE -eq 0 -and $gitRaw) {
            $parts = $gitRaw -split '\|', 3
            $gitLine = $parts[1] + "  " + $parts[0] + "  " + $parts[2]
            try { $gitAge = Age-Seconds ([datetime]::ParseExact($parts[0], "yyyy-MM-dd HH:mm:ss", $null)) } catch { $gitAge = $null }
        }
    } catch { $gitLine = $null }
}

# Verdict from the signal age vs the threshold.
if ($null -eq $signalTime) {
    $verdict = "STALLED"; $exit = 4
    $verdictWhy = "no work/gate files found under the scan root -- nothing is being written here"
} else {
    $sigAge = Age-Seconds $signalTime
    if ($sigAge -le $StaleSeconds) {
        $verdict = "ALIVE"; $exit = 0
        $verdictWhy = "newest work/gate write is " + $sigAge + "s old (<= " + $StaleSeconds + "s threshold)"
    } elseif ($sigAge -le (2 * $StaleSeconds)) {
        $verdict = "SLOWING"; $exit = 3
        $verdictWhy = "newest work/gate write is " + $sigAge + "s old (between 1x and 2x the " + $StaleSeconds + "s threshold) -- may be mid build/review; re-check before calling it"
    } else {
        $verdict = "STALLED"; $exit = 4
        $verdictWhy = "newest work/gate write is " + $sigAge + "s old (> 2x the " + $StaleSeconds + "s threshold) -- treat as hung or restart-paused; recover, do not keep polling"
    }
}

# Build the report. NO -f operator anywhere; plain concatenation + PadRight/
# PadLeft only, so a path can never break formatting.
$lines = New-Object System.Collections.Generic.List[string]

function Add-BucketHeadline([string]$label, $newest, $count) {
    if ($null -eq $newest) {
        $lines.Add($label.PadRight(7) + ": (none)")
    } else {
        $age = Age-Seconds $newest.Time
        $lines.Add($label.PadRight(7) + ": " + $newest.Time.ToString("yyyy-MM-dd HH:mm:ss") +
                   "  (" + $age + "s ago)  " + $newest.Rel + "   [" + $count + " files]")
    }
}

function Add-BucketList([string]$title, $list) {
    if ($list.Count -eq 0) { return }
    $lines.Add("")
    $lines.Add("---- " + $title + " (top " + $Top + ") ----")
    $lines.Add("LAST WRITE".PadRight(21) + "  " + "AGE(s)".PadLeft(10) + "  " + "RELATIVE PATH")
    $lines.Add("-" * 78)
    $list | Sort-Object Time -Descending | Select-Object -First $Top | ForEach-Object {
        $age = Age-Seconds $_.Time
        $lines.Add($_.Time.ToString("yyyy-MM-dd HH:mm:ss").PadRight(21) + "  " +
                   ([string]$age).PadLeft(10) + "  " + $_.Rel)
    }
}

$lines.Add("================ LIVENESS PROBE ================")
$lines.Add("VERDICT          : " + $verdict + " -- " + $verdictWhy)
$lines.Add("Probe run at (NOW): " + $now.ToString("yyyy-MM-dd HH:mm:ss"))
$lines.Add("Scan root        : " + $root)
$lines.Add("Stale threshold  : " + $StaleSeconds + "s (ALIVE <= 1x, SLOWING <= 2x, STALLED > 2x)")
$lines.Add("")
$lines.Add("---- NEWEST WRITE PER BUCKET ----")
Add-BucketHeadline "WORK"  $newestWork  $work.Count
Add-BucketHeadline "GATE"  $newestGate  $gate.Count
Add-BucketHeadline "BUILD" $newestBuild $build.Count
$lines.Add("")
if ($NoGit) {
    $lines.Add("GIT last commit  : (skipped -- -NoGit)")
} elseif ($gitLine) {
    $gitAgeTxt = if ($null -ne $gitAge) { "  (" + $gitAge + "s ago)" } else { "" }
    $lines.Add("GIT last commit  : " + $gitLine + $gitAgeTxt)
    if ($null -ne $signalTime -and $null -ne $gitAge) {
        $sigAge = Age-Seconds $signalTime
        if ($gitAge -gt $sigAge + 60) {
            $lines.Add("GIT note         : work is NEWER than the last commit -- uncommitted work in flight" +
                       " (the 'produced output but has not committed' stall shape -- check this if STALLED).")
        }
    }
} else {
    $lines.Add("GIT last commit  : (no commit found / not a git repo)")
}

Add-BucketList "WORK files"  $work
Add-BucketList "GATE files"  $gate
Add-BucketList "BUILD files" $build

$lines.Add("")
$lines.Add("Report written to: " + $LogFile)
$lines.Add("================ END PROBE (" + $verdict + ") ================")

# Write the log, then echo it to the terminal.
$text = $lines -join [Environment]::NewLine
Set-Content -LiteralPath $LogFile -Value $text -Encoding utf8
Write-Output $text

exit $exit
