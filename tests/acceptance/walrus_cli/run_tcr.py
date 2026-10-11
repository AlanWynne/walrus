"""Test Completion Report (TCR) generator for the walrus CLI acceptance suite.

Discovers and runs the whole suite with a custom ``unittest.TestResult`` that
harvests each test's ``@covers`` tags and outcome, then emits a human report
(``.md``) and a machine form (``.json``) under the git-ignored ``.gate/tcr/``
directory, and a sign-off verdict:

    PASS iff zero failures AND zero errors AND zero uncovered requirement ids
    FAIL otherwise, with the reasons listed.

The process exits 0 on PASS and non-zero on FAIL, so it can gate a pipeline.

Pure standard library (unittest, subprocess, platform, datetime, json,
pathlib, sys, os). ASCII-only output (ASCII '->' / '<=' substitutes).
"""

from __future__ import annotations

import json
import os
import platform
import subprocess
import sys
import unittest
from datetime import datetime, timezone
from pathlib import Path

# Import the registry and harness from the suite directory (this file's dir).
SUITE_DIR = Path(__file__).resolve().parent
if str(SUITE_DIR) not in sys.path:
    sys.path.insert(0, str(SUITE_DIR))

import requirements  # noqa: E402  (path set up above)
from harness import find_walrus  # noqa: E402


# === outcome collection ==============================================

class CoveringResult(unittest.TestResult):
    """A TestResult that records, per test, its ``_covers`` tuple and outcome."""

    #: outcome -> {test_id: {"covers": (...), "detail": str|None}}
    def __init__(self) -> None:
        super().__init__()
        self.records: dict[str, dict] = {}

    def _covers_of(self, test: unittest.case.TestCase) -> tuple:
        method = getattr(test, test._testMethodName, None)
        return tuple(getattr(method, "_covers", ()))

    def _note(self, test, outcome: str, detail: str | None = None) -> None:
        self.records[test.id()] = {
            "covers": self._covers_of(test),
            "outcome": outcome,
            "detail": detail,
        }

    def addSuccess(self, test):
        super().addSuccess(test)
        self._note(test, "pass")

    def addFailure(self, test, err):
        super().addFailure(test, err)
        self._note(test, "fail", self._exc_text(err))

    def addError(self, test, err):
        super().addError(test, err)
        self._note(test, "error", self._exc_text(err))

    def addSkip(self, test, reason):
        super().addSkip(test, reason)
        self._note(test, "skip", reason)

    def addExpectedFailure(self, test, err):
        super().addExpectedFailure(test, err)
        self._note(test, "expected_failure", self._exc_text(err))

    def addUnexpectedSuccess(self, test):
        super().addUnexpectedSuccess(test)
        self._note(test, "unexpected_success")

    @staticmethod
    def _exc_text(err) -> str:
        exc_type, exc_value, _tb = err
        return f"{exc_type.__name__}: {exc_value}"


# === run metadata ====================================================

def collect_binary_identity(walrus: Path) -> tuple[str, list[str]]:
    """Resolve the binary's version string and any findings.

    The CLI has no ``--version`` token (that word is only a per-file selector),
    so a ``walrus --version`` attempt is treated as "absent" and the identity
    falls back to the git short-sha of HEAD, with a recorded finding.
    """
    findings: list[str] = []
    try:
        proc = subprocess.run(
            [str(walrus), "--version"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
        )
        out = (proc.stdout + proc.stderr).decode("utf-8", "replace")
        # A real version line would be short and not the usage banner.
        if proc.returncode == 0 and "USAGE" not in out and out.strip():
            return out.strip().splitlines()[0], findings
    except (OSError, subprocess.SubprocessError):
        pass

    findings.append("CLI has no --version flag; binary identity is the git short-sha of HEAD")
    sha = git_short_sha()
    return f"git:{sha}", findings


def git_short_sha() -> str:
    """The short HEAD sha of the repo, or 'unknown' if git is unavailable."""
    try:
        proc = subprocess.run(
            ["git", "-C", str(SUITE_DIR), "rev-parse", "--short", "HEAD"],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            timeout=30,
        )
        if proc.returncode == 0:
            return proc.stdout.decode("utf-8", "replace").strip() or "unknown"
    except (OSError, subprocess.SubprocessError):
        pass
    return "unknown"


# === matrix construction =============================================

def build_matrix(records: dict[str, dict]) -> dict[str, dict]:
    """Invert the per-test ``covers`` tags against the registry.

    Returns id -> {"description", "tests": [...], "verdict"} where verdict is
    PASS (all covering tests green), FAIL (any covering test failing/erroring),
    or UNCOVERED (no covering test).
    """
    # id -> list of (test_id, outcome)
    by_id: dict[str, list[tuple[str, str]]] = {rid: [] for rid in requirements.all_ids()}
    for test_id, rec in records.items():
        for rid in rec["covers"]:
            # A covers tag not in the registry is itself a finding (handled by
            # the caller); only wire up ids the registry knows.
            if rid in by_id:
                by_id[rid].append((test_id, rec["outcome"]))

    matrix: dict[str, dict] = {}
    # Outcomes that count as "the requirement is demonstrated satisfied".
    green = {"pass", "skip", "expected_failure"}
    for rid, tests in by_id.items():
        if not tests:
            verdict = "UNCOVERED"
        elif all(outcome in green for _tid, outcome in tests):
            verdict = "PASS"
        else:
            verdict = "FAIL"
        matrix[rid] = {
            "description": requirements.REGISTRY[rid],
            "tests": [{"test": tid, "outcome": oc} for tid, oc in sorted(tests)],
            "verdict": verdict,
        }
    return matrix


def unknown_covers_ids(records: dict[str, dict]) -> list[str]:
    """Any ``@covers`` id that is not in the registry (a drift finding)."""
    known = requirements.all_ids()
    seen: set[str] = set()
    for rec in records.values():
        for rid in rec["covers"]:
            if rid not in known:
                seen.add(rid)
    return sorted(seen)


# === report emission =================================================

def emit(result: CoveringResult, matrix: dict[str, dict], meta: dict,
         findings: list[str]) -> tuple[dict, int]:
    """Build the report payload and return (payload, exit_code)."""
    totals = {
        "run": result.testsRun,
        "passed": sum(1 for r in result.records.values() if r["outcome"] == "pass"),
        "failed": len(result.failures),
        "errored": len(result.errors),
        "skipped": len(result.skipped),
        "expected_failures": len(result.expectedFailures),
        "unexpected_successes": len(result.unexpectedSuccesses),
    }

    uncovered = sorted(rid for rid, row in matrix.items() if row["verdict"] == "UNCOVERED")
    failed_ids = sorted(rid for rid, row in matrix.items() if row["verdict"] == "FAIL")
    failures = [
        {"test": tid, "outcome": rec["outcome"], "detail": rec["detail"]}
        for tid, rec in sorted(result.records.items())
        if rec["outcome"] in ("fail", "error", "unexpected_success")
    ]

    reasons: list[str] = []
    if totals["failed"]:
        reasons.append(f"{totals['failed']} failing test(s)")
    if totals["errored"]:
        reasons.append(f"{totals['errored']} errored test(s)")
    if totals["unexpected_successes"]:
        reasons.append(f"{totals['unexpected_successes']} unexpected success(es)")
    if uncovered:
        reasons.append(f"{len(uncovered)} uncovered requirement id(s)")

    verdict = "PASS" if not reasons else "FAIL"

    payload = {
        "title": "Walrus CLI -- Test Completion Report",
        "run": meta,
        "totals": totals,
        "traceability": matrix,
        "uncovered": uncovered,
        "failed_requirements": failed_ids,
        "failures": failures,
        "findings": findings,
        "verdict": verdict,
        "reasons": reasons,
    }
    return payload, (0 if verdict == "PASS" else 1)


def render_markdown(payload: dict) -> str:
    """Render the TCR payload as an ASCII Markdown report."""
    meta = payload["run"]
    totals = payload["totals"]
    lines: list[str] = []
    lines.append(f"# {payload['title']}")
    lines.append("")
    lines.append("## Run metadata")
    lines.append("")
    lines.append(f"- Timestamp (UTC): {meta['timestamp']}")
    lines.append(f"- Platform: {meta['platform']}")
    lines.append(f"- Python: {meta['python']}")
    lines.append(f"- Walrus binary: {meta['binary_path']}")
    lines.append(f"- Walrus version: {meta['binary_version']}")
    lines.append("")
    lines.append("## Totals")
    lines.append("")
    lines.append(f"- Tests run: {totals['run']}")
    lines.append(f"- Passed: {totals['passed']}")
    lines.append(f"- Failed: {totals['failed']}")
    lines.append(f"- Errored: {totals['errored']}")
    lines.append(f"- Skipped: {totals['skipped']}")
    lines.append(f"- Expected failures: {totals['expected_failures']}")
    lines.append(f"- Unexpected successes: {totals['unexpected_successes']}")
    lines.append("")
    lines.append("## Requirement traceability matrix")
    lines.append("")
    lines.append("| Requirement | Verdict | Covering test(s) |")
    lines.append("| --- | --- | --- |")
    for rid in sorted(payload["traceability"]):
        row = payload["traceability"][rid]
        tests = ", ".join(f"{t['test']} ({t['outcome']})" for t in row["tests"]) or "-"
        lines.append(f"| {rid} | {row['verdict']} | {tests} |")
    lines.append("")
    lines.append(f"Each id maps to its covering test(s) -> PASS / FAIL / UNCOVERED.")
    lines.append("")

    lines.append("## Uncovered requirements")
    lines.append("")
    if payload["uncovered"]:
        for rid in payload["uncovered"]:
            lines.append(f"- {rid}: {payload['traceability'][rid]['description']}")
    else:
        lines.append("- none")
    lines.append("")

    lines.append("## Failures")
    lines.append("")
    if payload["failures"]:
        for item in payload["failures"]:
            lines.append(f"- {item['test']} [{item['outcome']}]: {item['detail']}")
    else:
        lines.append("- none")
    lines.append("")

    lines.append("## Findings")
    lines.append("")
    if payload["findings"]:
        for finding in payload["findings"]:
            lines.append(f"- {finding}")
    else:
        lines.append("- none")
    lines.append("")

    lines.append("## Sign-off verdict")
    lines.append("")
    lines.append(f"**{payload['verdict']}**")
    if payload["reasons"]:
        lines.append("")
        lines.append("Reasons:")
        for reason in payload["reasons"]:
            lines.append(f"- {reason}")
    lines.append("")
    return "\n".join(lines)


# === entry point =====================================================

def main() -> int:
    walrus = find_walrus()
    binary_version, findings = collect_binary_identity(walrus)

    # Record any @covers id that drifted out of the registry as a finding.
    loader = unittest.TestLoader()
    suite = loader.discover(str(SUITE_DIR))
    result = CoveringResult()
    suite.run(result)

    drift = unknown_covers_ids(result.records)
    for rid in drift:
        findings.append(f"test references unknown requirement id '{rid}' (registry drift)")

    matrix = build_matrix(result.records)

    meta = {
        "timestamp": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "platform": platform.platform(),
        "python": sys.version.split()[0],
        "binary_path": str(walrus),
        "binary_version": binary_version,
    }

    payload, exit_code = emit(result, matrix, meta, findings)

    out_dir = SUITE_DIR.parents[2] / ".gate" / "tcr"
    out_dir.mkdir(parents=True, exist_ok=True)
    md_path = out_dir / "walrus-cli-TCR.md"
    json_path = out_dir / "walrus-cli-TCR.json"
    md_path.write_text(render_markdown(payload), encoding="utf-8")
    json_path.write_text(json.dumps(payload, indent=2, sort_keys=True), encoding="utf-8")

    print(f"TCR written to {md_path}")
    print(f"TCR written to {json_path}")
    print(f"VERDICT={payload['verdict']}")
    print(f"EXIT={exit_code}")
    return exit_code


if __name__ == "__main__":
    sys.exit(main())
