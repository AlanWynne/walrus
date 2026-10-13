"""No-op re-add byte-determinism (CLI-DESIGN 5.3 / 5.4).

Re-adding the EXACT same unchanged source into an archive must be a no-op: the
CLI reports no change and records no new version (CLI-DESIGN-5.4-nochange), so
the archive bytes must be byte-identical before and after the second `add`.

This asserts the TRUE invariant. If it fails, that is a genuine walrus-core
determinism finding, not a test bug -- the assertion stays as byte-identical
and the divergence evidence (lengths plus the first differing offset with a
hexdump window) is captured for the reviewer and owner.
"""

from __future__ import annotations

import unittest
from pathlib import Path

from harness import WalrusCase, covers


# Where the evidence file lands if the invariant is ever violated:
# <repo-root>/.gate/determinism-finding.txt. SUITE is tests/acceptance/
# walrus_cli, so the repo root is three parents up.
_FINDING_PATH = Path(__file__).resolve().parents[3] / ".gate" / "determinism-finding.txt"


def _first_diff_offset(a: bytes, b: bytes) -> int:
    """The first byte offset where *a* and *b* differ, or the length of the
    shorter one if they match up to that point (lengths then differ)."""
    limit = min(len(a), len(b))
    for i in range(limit):
        if a[i] != b[i]:
            return i
    return limit


def _hexdump_window(data: bytes, center: int, radius: int = 16) -> str:
    """A short hex view of *data* around *center* (for the finding report)."""
    start = max(0, center - radius)
    end = min(len(data), center + radius)
    window = data[start:end]
    hex_part = " ".join(f"{byte:02x}" for byte in window)
    return f"[{start}:{end}] {hex_part}"


def _render_evidence(before: bytes, after: bytes) -> str:
    offset = _first_diff_offset(before, after)
    lines = [
        "no-op re-add determinism VIOLATION",
        f"len(before) = {len(before)}",
        f"len(after)  = {len(after)}",
        f"first differing offset = {offset}",
        f"before {_hexdump_window(before, offset)}",
        f"after  {_hexdump_window(after, offset)}",
    ]
    return "\n".join(lines) + "\n"


class DeterminismTests(WalrusCase):

    @covers("CLI-DESIGN-5.3-add-noop-deterministic", "CLI-DESIGN-5.4-nochange")
    def test_noop_re_add_of_unchanged_content_leaves_archive_byte_identical(self):
        # Create the archive from an unchanged fixture tree.
        arc, _src, _initial = self.seed_archive("--wal")
        before = arc.read_bytes()

        # Re-add the EXACT same, unchanged source. The CLI reports no change
        # and records no new version.
        src = self.work / "src"
        r = self.run_walrus("add", arc, src, "-R", "--wal")
        self.assert_code(r, 0)

        after = arc.read_bytes()

        if before != after:
            # Genuine walrus-core finding: capture evidence, do NOT weaken the
            # assertion. Write it where the reviewer and owner can see it.
            evidence = _render_evidence(before, after)
            _FINDING_PATH.parent.mkdir(parents=True, exist_ok=True)
            _FINDING_PATH.write_text(evidence, encoding="utf-8")

        self.assertEqual(
            before, after,
            "a no-op re-add of unchanged content must leave the archive "
            "byte-identical; see .gate/determinism-finding.txt for evidence",
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
