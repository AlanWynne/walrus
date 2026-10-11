"""Acceptance tests for `walrus query` (and the bare-archive default).

Terse output is one `name<TAB>index_size` line per tracked file, where the
index size is the count of change-log records beyond the base (0 for a single
version). `-l` adds the long columns. A mask filters with segment-aware glob
semantics. A missing or non-archive file fails cleanly, non-zero, no panic.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class QueryTests(WalrusCase):

    @covers("FR-W11-AC1", "CLI-DESIGN-5.1-query-terse")
    def test_terse_listing_names_every_file_with_an_index_size(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("query", arc)
        self.assert_code(r, 0)
        # One line per fixture file, each `name<TAB>count`.
        lines = sorted(line for line in r.out_text.splitlines() if line)
        self.assertEqual(
            lines,
            ["src/data/nums.csv\t0", "src/notes/todo.md\t0", "src/readme.txt\t0"],
        )

    @covers("FR-W11-AC1", "CLI-DESIGN-5.1-query-terse")
    def test_index_size_counts_recorded_versions_after_an_update(self):
        arc, src, _h = self.seed_archive()
        self.make_multi_version(arc, src)
        r = self.run_walrus("query", arc)
        self.assert_code(r, 0)
        # readme.txt gained one change-log record; nums.csv stayed at the base.
        self.assert_stdout_contains(r, "src/readme.txt\t1")
        self.assert_stdout_contains(r, "src/data/nums.csv\t0")

    @covers("FR-W11-AC1", "FR-W11-AC6")
    def test_bare_archive_defaults_to_query(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus(arc)
        self.assert_code(r, 0)
        self.assert_stdout_contains(r, "src/readme.txt")

    @covers("FR-W11-AC1", "CLI-DESIGN-5.1-query-long")
    def test_long_listing_adds_log_bytes_and_generation_columns(self):
        arc, src, _h = self.seed_archive()
        self.make_multi_version(arc, src)
        r = self.run_walrus("query", arc, "-l")
        self.assert_code(r, 0)
        # Find the readme row and assert it has the long column shape:
        # name, index_size, log_bytes, generations, size, mode, mtime.
        row = next(
            line for line in r.out_text.splitlines() if line.startswith("src/readme.txt\t")
        )
        cols = row.split("\t")
        self.assertEqual(len(cols), 7, f"long row should have 7 columns: {row!r}")
        self.assertEqual(cols[1], "1")          # index size (one record)
        self.assertTrue(cols[2].isdigit(), f"log_bytes column numeric: {row!r}")
        self.assertGreater(int(cols[2]), 0, "a versioned file has non-zero log bytes")

    @covers("CLI-DESIGN-5.1-query-mask", "CLI-DESIGN-5.4c-mask-segment")
    def test_single_star_mask_matches_within_one_segment_only(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("query", arc, "src/*")
        self.assert_code(r, 0)
        names = sorted(line.split("\t")[0] for line in r.out_text.splitlines() if line)
        # `src/*` matches `src/readme.txt` but not the deeper nested files.
        self.assertEqual(names, ["src/readme.txt"])

    @covers("CLI-DESIGN-5.1-query-mask", "CLI-DESIGN-5.4c-mask-segment")
    def test_double_star_mask_spans_slash_boundaries(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("query", arc, "src/**")
        self.assert_code(r, 0)
        names = sorted(line.split("\t")[0] for line in r.out_text.splitlines() if line)
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
