"""Acceptance tests for `walrus cat` (CLI-DESIGN 5.2.2).

cat reconstructs exactly ONE tracked file and writes its raw bytes to stdout.
It never touches disk; zero or more than one match is RC 2; the beyond-last
numeric index fallback (original + warning + RC 4) matches extract.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class CatTests(WalrusCase):

    @covers("FR-W10-AC3", "CLI-DESIGN-5.2.2-cat-stdout")
    def test_cat_writes_exact_current_bytes(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("cat", arc, "src/readme.txt")
        self.assert_code(r, 0)
        self.assert_stdout_bytes(r, b"hello walrus\nline two")

    @covers("CLI-DESIGN-5.2.2-cat-stdout")
    def test_cat_adds_no_trailing_newline(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("cat", arc, "src/data/nums.csv")
        self.assert_code(r, 0)
        # The stored content had no trailing newline; cat must not add one.
        self.assert_stdout_bytes(r, b"a,b,c\n1,2,3")

    @covers("FR-W10-AC4")
    def test_cat_older_version_by_index(self):
        arc, src, _h = self.seed_archive()
        self.make_multi_version(arc, src)
        # Index 1 is the pre-edit (original) content of readme.txt.
        r = self.run_walrus("cat", arc, "src/readme.txt", "--index", "1")
        self.assert_code(r, 0)
        self.assert_stdout_bytes(r, b"hello walrus\nline two")

    @covers("FR-W10-AC4")
    def test_cat_version_original_bytes(self):
        arc, src, _h = self.seed_archive()
        self.make_multi_version(arc, src)
        r = self.run_walrus("cat", arc, "src/readme.txt", "--version", "original")
        self.assert_code(r, 0)
        self.assert_stdout_bytes(r, b"hello walrus\nline two")

    @covers("FR-W10-AC4", "CLI-DESIGN-6a-utc-stamp")
    def test_cat_at_before_earliest_gives_current(self):
        arc, src, _h = self.seed_archive()
        self.make_multi_version(arc, src)
        r = self.run_walrus("cat", arc, "src/readme.txt", "--at", "2000-01-01T00:00:00Z")
        self.assert_code(r, 0)
        self.assert_stdout_bytes(r, b"hello walrus\nline two CHANGED")

    @covers("FR-W10-AC3", "CLI-DESIGN-6-rc2")
    def test_cat_zero_match_is_rc2(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("cat", arc, "no-such.txt")
        self.assert_code(r, 2)

    @covers("FR-W10-AC3", "CLI-DESIGN-6-rc2")
    def test_cat_multi_match_is_rc2(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("cat", arc, "src/**")
        self.assert_code(r, 2)

    @covers("FR-W10-AC3")
    def test_cat_never_writes_disk(self):
        arc, src, _h = self.seed_archive()
        before = self.tree_hashes(self.work)
        self.assert_code(self.run_walrus("cat", arc, "src/readme.txt"), 0)
        after = self.tree_hashes(self.work)
        self.assertEqual(before, after, "cat must not create or alter any file")


if __name__ == "__main__":
    unittest.main(verbosity=2)
