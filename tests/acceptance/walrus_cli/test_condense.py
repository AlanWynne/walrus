"""Acceptance tests for `walrus condense` (CLI-DESIGN 5.6).

condense re-bases a selected file, discarding older history. `--keep 0`
collapses to the current version only (the current bytes stay byte-identical);
`--keep n` retains n older versions. No match is RC 8. The version count (query
index size) drops.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class CondenseTests(WalrusCase):

    def _multi(self):
        """Seed an archive and give readme.txt three change-log records."""
        arc, src, _h = self.seed_archive()
        for body in ("hello walrus\nedit one", "hello walrus\nedit two",
                     "hello walrus\nedit three"):
            self.write(src / "readme.txt", body)
            self.assert_code(self.run_walrus("update", arc, src, "-R"), 0)
        return arc, src

    def _index_size(self, arc, name):
        r = self.run_walrus("query", arc)
        row = next(l for l in r.out_text.splitlines() if l.startswith(name + "\t"))
        return int(row.split("\t")[1])

    @covers("CLI-DESIGN-5.6-condense-keep0")
    def test_keep_zero_collapses_to_current_byte_identical(self):
        arc, src = self._multi()
        before = self.run_walrus("cat", arc, "src/readme.txt").out
        self.assert_code(
            self.run_walrus("condense", arc, "src/readme.txt", "--keep", "0"), 0
        )
        self.assertEqual(self._index_size(arc, "src/readme.txt"), 0,
                         "keep 0 drops the index size to zero")
        after = self.run_walrus("cat", arc, "src/readme.txt").out
        self.assertEqual(before, after, "current bytes stay byte-identical")

    @covers("CLI-DESIGN-5.6-condense-keepn")
    def test_keep_n_retains_n_versions(self):
        arc, src = self._multi()
        start = self._index_size(arc, "src/readme.txt")
        self.assertGreaterEqual(start, 3)
        self.assert_code(
            self.run_walrus("condense", arc, "src/readme.txt", "--keep", "1"), 0
        )
        self.assertEqual(self._index_size(arc, "src/readme.txt"), 1)
        # The retained older version still reconstructs.
        self.assert_code(self.run_walrus("cat", arc, "src/readme.txt", "--index", "1"), 0)

    @covers("CLI-DESIGN-5.6-condense-keepn")
    def test_condense_reduces_version_count(self):
        arc, src = self._multi()
        before = self._index_size(arc, "src/readme.txt")
        self.assert_code(
            self.run_walrus("condense", arc, "src/readme.txt", "--keep", "0"), 0
        )
        after = self._index_size(arc, "src/readme.txt")
        self.assertLess(after, before, "condense must reduce the version count")

    @covers("CLI-DESIGN-5.6-condense-nomatch", "FR-W11-AC3", "CLI-DESIGN-6-rc8")
    def test_no_match_is_rc8(self):
        arc, _src, _h = self.seed_archive()
        self.assert_code(self.run_walrus("condense", arc, "nope.txt", "--keep", "0"), 8)


if __name__ == "__main__":
    unittest.main(verbosity=2)
