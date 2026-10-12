"""Acceptance tests for `walrus query` (and the bare-archive default).

Terse output is ls-style: one bare relative name per line, sorted, nothing else.
`-l` is an aligned table with a header row and an ASCII `-`-run separator row,
columns Versions | LastWriteTime | Bytes | Name. LastWriteTime is the source
file's mtime (the current version on a multi-version file). A mask filters with
segment-aware glob semantics. A missing or non-archive file fails cleanly,
non-zero, no panic.
"""

from __future__ import annotations

import datetime
import os
import unittest

from harness import WalrusCase, covers


def _display_stamp(epoch: int) -> str:
    """The `YYYY-MM-DD HH:MM:SS` display form of a UTC epoch second."""
    dt = datetime.datetime.fromtimestamp(epoch, tz=datetime.timezone.utc)
    return dt.strftime("%Y-%m-%d %H:%M:%S")


class QueryTests(WalrusCase):

    @covers("FR-W11-AC1", "CLI-DESIGN-5.1-query-terse")
    def test_terse_listing_names_every_file_with_an_index_size(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("query", arc)
        self.assert_code(r, 0)
        # ls-style: one bare name per line, sorted, no index/count.
        lines = [line for line in r.out_text.splitlines() if line]
        self.assertEqual(
            lines,
            ["src/data/nums.csv", "src/notes/todo.md", "src/readme.txt"],
        )

    @covers("FR-W11-AC1", "CLI-DESIGN-5.1-query-long")
    def test_index_size_counts_recorded_versions_after_an_update(self):
        arc, src, _h = self.seed_archive()
        self.make_multi_version(arc, src)
        # The count now lives in the -l Versions column, not terse output.
        self.assertEqual(self.query_versions(arc, "src/readme.txt"), 1)
        self.assertEqual(self.query_versions(arc, "src/data/nums.csv"), 0)

    @covers("FR-W11-AC1", "FR-W11-AC6")
    def test_bare_archive_defaults_to_query(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus(arc)
        self.assert_code(r, 0)
        self.assert_stdout_contains(r, "src/readme.txt")

    @covers("FR-W11-AC1", "CLI-DESIGN-5.1-query-long")
    def test_long_listing_is_an_aligned_table_with_versions_lastwrite_bytes_name(self):
        arc, src, _h = self.seed_archive()
        self.make_multi_version(arc, src)
        r = self.run_walrus("query", arc, "-l")
        self.assert_code(r, 0)
        lines = r.out_text.splitlines()
        header = lines[0]
        for label in ("Versions", "LastWriteTime", "Bytes", "Name"):
            self.assertIn(label, header, f"header has {label}: {header!r}")
        # The second row is runs of '-' (the separator).
        separator = lines[1]
        self.assertTrue(
            all(all(c == "-" for c in field) for field in separator.split()),
            f"separator row is runs of '-': {separator!r}",
        )
        # The readme row parses (whitespace runs) to Versions == 1, a numeric
        # Bytes, and Name src/readme.txt.
        row = next(l for l in lines if l.split() and l.split()[-1] == "src/readme.txt")
        fields = row.split()
        self.assertEqual(int(fields[0]), 1, f"Versions column is 1: {row!r}")
        self.assertEqual(fields[-1], "src/readme.txt")
        # Bytes is the field immediately before Name.
        self.assertTrue(fields[-2].isdigit(), f"Bytes column numeric: {row!r}")

    @covers("FR-W11-AC8", "CLI-DESIGN-5.1-query-lastwrite")
    def test_long_lastwrite_is_the_source_file_mtime(self):
        src = self.make_fixture_tree(self.work / "src")
        epoch = 1_791_735_524  # 2026-10-11T16:18:44Z
        for rel in ("readme.txt", "notes/todo.md", "data/nums.csv"):
            os.utime(src / rel, (epoch, epoch))
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        r = self.run_walrus("query", arc, "-l")
        self.assert_code(r, 0)
        row = next(
            l for l in r.out_text.splitlines() if l.split() and l.split()[-1] == "src/readme.txt"
        )
        fields = row.split()
        # LastWriteTime is the date + time fields (two whitespace tokens), which
        # must equal the fixed source mtime, NOT "now".
        last = f"{fields[1]} {fields[2]}"
        self.assertEqual(last, _display_stamp(epoch))

    @covers("CLI-DESIGN-5.1-query-lastwrite", "FR-W11-AC8")
    def test_long_lastwrite_tracks_the_current_version_on_a_multi_version_wal(self):
        # HIGH-1 guard: add with a KNOWN-early mtime, update with a KNOWN-later
        # mtime (Versions == 2), then -l LastWriteTime must be the LATER time.
        src = self.make_fixture_tree(self.work / "src")
        early = 1_600_000_000   # 2020-09-13T12:26:40Z
        later = 1_791_735_524   # 2026-10-11T16:18:44Z
        for rel in ("readme.txt", "notes/todo.md", "data/nums.csv"):
            os.utime(src / rel, (early, early))
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        # Change readme content and stamp it later; update via -R so the name
        # matches the recursed `src/readme.txt` the add stored.
        self.write(src / "readme.txt", "hello walrus\nline two CHANGED")
        os.utime(src / "readme.txt", (later, later))
        os.utime(src / "notes/todo.md", (early, early))
        os.utime(src / "data/nums.csv", (early, early))
        self.assert_code(self.run_walrus("update", arc, src, "-R"), 0)
        self.assertEqual(self.query_versions(arc, "src/readme.txt"), 1)
        r = self.run_walrus("query", arc, "-l")
        self.assert_code(r, 0)
        row = next(
            l for l in r.out_text.splitlines() if l.split() and l.split()[-1] == "src/readme.txt"
        )
        fields = row.split()
        last = f"{fields[1]} {fields[2]}"
        self.assertEqual(last, _display_stamp(later), "LastWriteTime is the later save")

    @covers("CLI-DESIGN-5.1-query-mask", "CLI-DESIGN-5.4c-mask-segment")
    def test_single_star_mask_matches_within_one_segment_only(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("query", arc, "src/*")
        self.assert_code(r, 0)
        # Terse lines are bare names (no TAB); the whole line IS the name.
        names = sorted(line for line in r.out_text.splitlines() if line)
        self.assertEqual(names, ["src/readme.txt"])

    @covers("CLI-DESIGN-5.1-query-mask", "CLI-DESIGN-5.4c-mask-segment")
    def test_double_star_mask_spans_slash_boundaries(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("query", arc, "src/**")
        self.assert_code(r, 0)
        names = sorted(line for line in r.out_text.splitlines() if line)
        self.assertEqual(
            names,
            ["src/data/nums.csv", "src/notes/todo.md", "src/readme.txt"],
        )

    @covers("FR-W11-AC1")
    def test_empty_archive_lists_nothing_and_is_rc0(self):
        arc, _src, _h = self.seed_archive()
        self.assert_code(self.run_walrus("remove", arc, "src/**", "--force"), 0)
        r = self.run_walrus("query", arc)
        self.assert_code(r, 0)
        self.assertEqual(r.out.strip(), b"", f"empty archive should list nothing\n{r!r}")

    @covers("FR-W11-AC6")
    def test_missing_archive_fails_cleanly(self):
        r = self.run_walrus("query", self.work / "nope.wal")
        self.assert_nonzero_no_panic(r)

    @covers("CLI-DESIGN-1.4-codec-garbage")
    def test_garbage_file_fails_cleanly(self):
        junk = self.write_bytes(self.work / "junk.wal", bytes(range(0, 32)))
        r = self.run_walrus("query", junk)
        self.assert_nonzero_no_panic(r)


if __name__ == "__main__":
    unittest.main(verbosity=2)
