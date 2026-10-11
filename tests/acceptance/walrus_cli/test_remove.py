"""Acceptance tests for `walrus remove` (CLI-DESIGN 5.5).

A single-match mask removes without `--force`. A multi-match mask without
`--force` is RC 2 and lists the matches; with `--force` it removes all. No
match is RC 8. After a remove, query and extract omit the removed file.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class RemoveTests(WalrusCase):

    @covers("CLI-DESIGN-5.5-remove-single")
    def test_single_match_removes_without_force(self):
        arc, _src, _h = self.seed_archive()
        self.assert_code(self.run_walrus("remove", arc, "src/readme.txt"), 0)
        r = self.run_walrus("query", arc)
        self.assertNotIn("readme.txt", r.out_text)

    @covers("CLI-DESIGN-5.5-remove-multi", "CLI-DESIGN-6-rc2")
    def test_multi_match_without_force_is_rc2_and_lists_matches(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("remove", arc, "src/**")
        self.assert_code(r, 2)
        self.assert_stderr_contains(r, "src/readme.txt")

    @covers("CLI-DESIGN-5.5-remove-multi")
    def test_multi_match_with_force_removes_all(self):
        arc, _src, _h = self.seed_archive()
        self.assert_code(self.run_walrus("remove", arc, "src/**", "--force"), 0)
        r = self.run_walrus("query", arc)
        self.assertEqual(r.out.strip(), b"")

    @covers("CLI-DESIGN-5.5-remove-nomatch", "FR-W11-AC3", "CLI-DESIGN-6-rc8")
    def test_no_match_is_rc8(self):
        arc, _src, _h = self.seed_archive()
        self.assert_code(self.run_walrus("remove", arc, "nope.txt"), 8)

    @covers("CLI-DESIGN-5.5-remove-single")
    def test_extract_omits_a_removed_file(self):
        arc, _src, _h = self.seed_archive()
        self.assert_code(self.run_walrus("remove", arc, "src/readme.txt"), 0)
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        got = self.tree_hashes(out / "src")
        self.assertNotIn("readme.txt", got)


if __name__ == "__main__":
    unittest.main(verbosity=2)
