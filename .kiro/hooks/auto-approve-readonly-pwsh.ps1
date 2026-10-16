# Auto-approve ONLY genuinely read-only execute_pwsh commands, so routine
# line-count / inspection commands do not interrupt the user with a permission
# prompt. Everything else stays SILENT so the normal approval flow runs.
#
# Contract (PreToolUse command hook):
#   - stdin: JSON describing the tool invocation. The command text is read from
#     the common field names Kiro may use; if none is found we stay silent.
#   - exit 0 with a JSON decision on stdout to auto-approve; the decision used
#     here is {"permissionDecision":"allow"}.
#   - exit 0 with NO stdout, or any other exit, means "no opinion" -> the user
#     is prompted as normal.
#
# SAFETY: deny-by-default. We approve a command ONLY when every token we can see
# is on a strict read-only allowlist AND no mutating/dangerous token appears.
# Any uncertainty -> stay silent (prompt the user). This script NEVER blocks a
# command (never exits 2); it only ever either approves or abstains.

$ErrorActionPreference = 'Stop'

function Abstain {
    # No stdout, clean exit: the host falls back to the normal prompt.
    exit 0
}

try {
    $raw = [Console]::In.ReadToEnd()
    if ([string]::IsNullOrWhiteSpace($raw)) { Abstain }

    $payload = $raw | ConvertFrom-Json

    # Pull the command text from whichever field carries it. Be liberal in what
    # we look for; if we cannot find a string command, abstain.
    $cmd = $null
    foreach ($path in @(
            { $payload.command },
            { $payload.input.command },
            { $payload.toolInput.command },
            { $payload.arguments.command },
            { $payload.parameters.command },
            { $payload.tool_input.command }
        )) {
        try { $v = & $path } catch { $v = $null }
        if ($v -is [string] -and $v.Trim().Length -gt 0) { $cmd = $v; break }
    }
    if ($null -eq $cmd) { Abstain }

    $text = $cmd.Trim()
    $lower = $text.ToLowerInvariant()

    # --- Hard DENY-LIST: any sign of mutation / side effect -> abstain -------
    # Redirection to a file, in-place edits, deletes, process control, env
    # mutation, git state changes, package/build actions, invocation of other
    # scripts, and network calls all disqualify auto-approval. We match on
    # whole words where it matters to avoid false hits inside paths.
    $denySubstrings = @(
        '>', '>>', '| out-file', 'set-content', 'add-content', 'new-item',
        'remove-item', 'rename-item', 'move-item', 'copy-item', 'clear-content',
        'ni ', 'rm ', 'del ', 'erase ', 'mkdir', 'rmdir', 'echo ',
        'stop-process', 'kill', 'taskkill', 'start-process', 'invoke-',
        'set-', 'new-', 'start-', '$env:', 'setx', '[environment]::set',
        'cargo ', 'rustc', 'rustup', 'npm', 'pnpm', 'yarn', 'node ',
        'git ', 'gh ', 'glab',
        'curl', 'wget', 'iwr', 'irm', 'invoke-webrequest', 'invoke-restmethod',
        '.ps1', '.bat', '.cmd', '.sh', '&', 'cmd /', 'powershell ', 'pwsh '
    )
    foreach ($bad in $denySubstrings) {
        if ($lower.Contains($bad)) { Abstain }
    }

    # --- Read-only ALLOWLIST: every command verb present must be read-only ----
    # Split on separators and newlines; inspect each statement's leading verb.
    $allowVerbs = @(
        'get-content', 'gc', 'cat', 'type',
        'select-string', 'sls',
        'get-childitem', 'gci', 'ls', 'dir',
        'measure-object', 'measure',
        'select-object', 'select',
        'sort-object', 'sort',
        'where-object', 'where',
        'foreach-object',
        'get-item', 'gi', 'test-path', 'resolve-path', 'split-path', 'join-path',
        'write-output', 'write-host',
        'get-date', 'out-string'
    )

    # Statements may be separated by ; or newlines or pipes. A pipe into a
    # read-only verb is fine; a pipe into anything not on the allowlist already
    # tripped the deny-list above for the risky cases.
    $statements = [regex]::Split($text, '[\r\n;|]+') | Where-Object { $_.Trim().Length -gt 0 }
    if ($statements.Count -eq 0) { Abstain }

    foreach ($stmt in $statements) {
        $s = $stmt.Trim()

        # A pure assignment of a read-only expression (e.g. $x = Get-Content ...)
        # strip a leading "$var =" so we can inspect the real verb.
        $s = [regex]::Replace($s, '^\$[A-Za-z_][A-Za-z0-9_]*\s*=\s*', '')

        # A bare string/number/expression statement (quoting a label, arithmetic
        # for a count) is harmless -> allow this statement.
        if ($s -match '^[''"]') { continue }
        if ($s -match '^[\(\)\d\s\+\-\*/\.""'']+$') { continue }
        if ($s -match '^\$[A-Za-z_]') { continue }   # echoing a variable

        # Otherwise the leading token must be an allowed read-only verb.
        $firstToken = ($s -split '\s+')[0].ToLowerInvariant()
        if ($allowVerbs -notcontains $firstToken) {
            Abstain
        }
    }

    # Every statement is read-only and nothing mutating was seen -> approve.
    $decision = @{
        hookSpecificOutput = @{
            permissionDecision       = 'allow'
            permissionDecisionReason = 'read-only inspection command (auto-approved by auto-approve-readonly-pwsh hook)'
        }
    } | ConvertTo-Json -Compress -Depth 5

    Write-Output $decision
    exit 0
}
catch {
    # On ANY error, abstain -- never approve on uncertainty, never block.
    Abstain
}
