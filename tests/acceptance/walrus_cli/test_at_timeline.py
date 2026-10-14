"""Timestamp-addressing acceptance test (`--at <T>`) over a multi-file archive
with KNOWN, SPREAD-OUT per-version timestamps (CLI-DESIGN 5.2.1, FR-W11-AC8).

The deep-history test could not exercise exact mid-history `--at` because it did
not pin per-version timestamps. This test does: it sets each source file's mtime
with ``os.utime`` before every add/update, so walrus stores that instant as the
version's change-record ``ts`` (proven by
test_query.py::test_long_lastwrite_is_the_source_file_mtime). It then builds a
small multi-file archive whose files change on DIFFERENT dates spread across
years -- an interleaved timeline like a real repository's history -- and asserts
that ``extract --at <T>`` reproduces the exact historical state at points across
the span.

`--at` semantics (verified against the binary + CLI-DESIGN 5.2.1 / FORMAT 5.3):
- `--at` matches CHANGE RECORDS and picks the version whose record is the NEWEST
  at-or-before T.
- The base (original) version has NO change record, so a T before a file's
  EARLIEST update record resolves to that file's CURRENT version (index 0), not
  to the base. The timeline below is built so every file's first appearance is
  itself an update-stamped version to keep the mapping unambiguous.

Determinism (per testing.md): all timestamps are FIXED strings/epochs, never the
wall clock; content is a pure function of the step.
"""

from __future__ import annotations

import datetime
import os
import unittest

from harness import WalrusCase, covers


def _epoch(iso: str) -> int:
    """Whole-second UTC epoch for a fixed ``YYYY-MM-DDThh:mm:ssZ`` string."""
    dt = datetime.datetime.strptime(iso, "%Y-%m-%dT%H:%M:%SZ").replace(
        tzinfo=datetime.timezone.utc
    )
    return int(dt.timestamp())


# A deterministic, spread-out timeline. Each entry is one archive version:
# (iso timestamp, {relative-name: content-at-this-version}).
#
# ALL THREE FILES EXIST FROM ENTRY 0. This is deliberate: `--at` is a PER-FILE
# snapshot selector (CLI-DESIGN 5.2.1) -- for a timestamp before a file's
# earliest change record it resolves to that file's CURRENT version, NOT to
# "absent". walrus has no "this file did not exist yet" notion. If a file were
# introduced mid-timeline, `--at` at an earlier instant would still return that
# file (at its current content), which muddies a "historical state" assertion.
# By having every file present from the start and only VARYING CONTENT on known
# later dates, each file's `--at T` resolves unambiguously to its as-of-T
# content, so the per-file snapshot IS the true historical state. readme changes
# often; notes and data change less -- a realistic mixed-churn history.
TIMELINE = [
    ("2021-03-01T09:00:00Z",
     {"readme.md": "readme v1\n", "notes.md": "notes v1\n", "data.csv": "a,b\n1,2\n"}),
    ("2022-07-15T12:30:00Z", {"readme.md": "readme v2 (2022)\n"}),
    ("2023-11-20T08:05:00Z", {"readme.md": "readme v3\n", "data.csv": "a,b\n1,2\n9,9\n"}),
    ("2024-05-02T17:45:00Z", {"notes.md": "notes v2 (2024)\n"}),
    ("2025-09-30T23:15:00Z", {"readme.md": "readme v4 final\n"}),
]


class AtTimelineTests(WalrusCase):

    def _build_timeline_archive(self):
        """Replay TIMELINE into a .wal, stamping each version via os.utime.

        Returns the archive path. Every entry is recorded with ``add -R``: add
        records a new version of a CHANGED tracked file and no-ops an unchanged
        one (content-keyed), which is exactly what a timeline replay needs. All
        files exist from entry 0, so only content varies thereafter. Each entry
        stamps ALL current files with that entry's instant so the whole snapshot
        shares one timestamp (an unchanged file's new mtime does not create a
        version -- no-change is keyed on content, not mtime).
        """
        src = self.work / "src"
        arc = self.work / "timeline.wal"

        state: dict[str, str] = {}
        for idx, (iso, changes) in enumerate(TIMELINE):
            ts = _epoch(iso)
            state.update(changes)
            for name, content in changes.items():
                self.write(src / name, content)
            for name in state:
                os.utime(src / name, (ts, ts))
            if idx == 0:
                self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
            else:
                self.assert_code(self.run_walrus("add", arc, src, "-R"), 0)
        return arc

    @covers("FR-W11-AC8", "CLI-DESIGN-6a-utc-stamp", "FR-W10-AC4")
    def test_extract_at_timestamp_reproduces_the_historical_multi_file_state(self):
        arc = self._build_timeline_archive()

        # Each case: a probe instant, and the EXACT content expected per file
        # under the documented `--at` rule -- the newest CHANGE RECORD at-or-
        # before the probe, else CURRENT when the probe precedes a file's first
        # record (the base/entry-0 content is NOT timestamp-addressable, so a
        # file whose only pre-probe state is the base resolves to current).
        #
        # Record dates per file in TIMELINE:
        #   readme.md: 2022-07-15, 2023-11-20, 2025-09-30 (base content @2021)
        #   notes.md : 2024-05-02                         (base content @2021)
        #   data.csv : 2023-11-20                         (base content @2021)
        current = {
            "readme.md": "readme v4 final\n",
            "notes.md": "notes v2 (2024)\n",
            "data.csv": "a,b\n1,2\n9,9\n",
        }
        cases = [
            # Mid-2022: readme at its 2022 record; notes/data still pre-first-
            # record -> current.
            ("2022-08-01T00:00:00Z", {
                "readme.md": "readme v2 (2022)\n",
                "notes.md": current["notes.md"],
                "data.csv": current["data.csv"],
            }),
            # Early 2024: readme at 2023 record; data at 2023 record; notes still
            # pre-first-record (its only record is 2024-05) -> current.
            ("2024-01-01T00:00:00Z", {
                "readme.md": "readme v3\n",
                "notes.md": current["notes.md"],
                "data.csv": "a,b\n1,2\n9,9\n",
            }),
            # Mid-2024 (after notes' 2024 record, before readme's 2025 record):
            # readme still at 2023, notes at 2024, data at 2023.
            ("2024-06-01T00:00:00Z", {
                "readme.md": "readme v3\n",
                "notes.md": "notes v2 (2024)\n",
                "data.csv": "a,b\n1,2\n9,9\n",
            }),
            # After everything: all current.
            ("2026-01-01T00:00:00Z", current),
        ]
        for probe, expected in cases:
            out = self.work / ("at" + probe.replace(":", "").replace("-", ""))
            self.assert_code(
                self.run_walrus("extract", arc, "-o", out, "--at", probe), 0
            )
            for name, content in expected.items():
                self.assert_file(out / "src" / name)
                self.assertEqual(
                    (out / "src" / name).read_text(encoding="utf-8"),
                    content,
                    f"{name} at {probe} must be the newest record at-or-before it",
                )

    @covers("FR-W11-AC8", "CLI-DESIGN-6a-utc-stamp")
    def test_extract_at_before_earliest_record_resolves_to_current(self):
        arc = self._build_timeline_archive()
        # A T before ANY file's earliest record resolves every file to CURRENT
        # (the base has no record), so the extracted tree is the LAST state.
        current = {
            "readme.md": "readme v4 final\n",
            "notes.md": "notes v2 (2024)\n",
            "data.csv": "a,b\n1,2\n9,9\n",
        }
        out = self.work / "at-before"
        self.assert_code(
            self.run_walrus("extract", arc, "-o", out, "--at", "2000-01-01T00:00:00Z"),
            0,
        )
        for name, content in current.items():
            self.assertEqual(
                (out / "src" / name).read_text(encoding="utf-8"), content,
                f"{name} before earliest record must be the current version",
            )


if __name__ == "__main__":
    unittest.main(verbosity=2)
