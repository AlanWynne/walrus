"""Acceptance tests for `walrus extract` (CLI-DESIGN 5.2.1).

Extract restores tracked files to disk under their stored names at a chosen
version. It is per-file (not transactional) and the whole-command return code
is the most severe per-file outcome. These tests build a mixed-count archive
so per-file version resolution is genuinely exercised.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class ExtractTests(WalrusCase):

    @covers("FR-W10-AC1", "FR-W11-AC6")
    def test_default_extract_reproduces_current_tree(self):
        arc, src, initial = self.seed_archive()
        edited = self.make_multi_version(arc, src)
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_tree_equals(out / "src", edited)

    @covers("FR-W10-AC4")
    def test_version_current_equals_default(self):
        arc, src, _h = self.seed_archive()
        edited = self.make_multi_version(arc, src)
        out = self.work / "out"
        self.assert_code(
            self.run_walrus("extract", arc, "-o", out, "--version", "current"), 0
        )
        self.assert_tree_equals(out / "src", edited)

    @covers("FR-W10-AC4")
    def test_version_original_restores_oldest_without_warning(self):
        arc, src, initial = self.seed_archive()
        self.make_multi_version(arc, src)
        out = self.work / "orig"
        r = self.run_walrus("extract", arc, "-o", out, "--version", "original")
        self.assert_code(r, 0)
        self.assert_tree_equals(out / "src", initial)
        self.assertNotIn("warn", r.err_text.lower(), f"original must not warn\n{r!r}")

    @covers("FR-W10-AC4")
    def test_index_zero_reproduces_current(self):
        arc, src, _h = self.seed_archive()
        edited = self.make_multi_version(arc, src)
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out, "--index", "0"), 0)
        self.assert_tree_equals(out / "src", edited)

    @covers("FR-W10-AC2", "CLI-DESIGN-6-rc4")
    def test_index_beyond_last_writes_original_and_warns_rc4(self):
        arc, src, initial = self.seed_archive()
        self.make_multi_version(arc, src)
        out = self.work / "out"
        r = self.run_walrus("extract", arc, "-o", out, "--index", "999")
        self.assert_code(r, 4)
        self.assert_stderr_contains(r, "beyond last")
        # The original version was written for the file that overshot.
        got = self.tree_hashes(out / "src")
        self.assertEqual(got["readme.txt"], initial["readme.txt"])

    @covers("FR-W10-AC4")
    def test_relative_plus_one_steps_back_one_version(self):
        arc, src, initial = self.seed_archive()
        self.make_multi_version(arc, src)
        out = self.work / "out"
        # Relative addressing: index 0 is current and a positive offset steps
        # toward older versions, so `+1` reaches readme's original content; a
        # negative offset clamps at current.
        self.assert_code(
            self.run_walrus("extract", arc, "src/readme.txt", "-o", out,
                            "--version", "+1"),
            0,
        )
        got = self.tree_hashes(out / "src")
        self.assertEqual(got["readme.txt"], initial["readme.txt"])

    @covers("FR-W10-AC4")
    def test_relative_negative_offset_clamps_at_current(self):
        arc, src, _h = self.seed_archive()
        edited = self.make_multi_version(arc, src)
        out = self.work / "out-neg"
        self.assert_code(
            self.run_walrus("extract", arc, "src/readme.txt", "-o", out,
                            "--version", "-5"),
            0,
        )
        got = self.tree_hashes(out / "src")
        self.assertEqual(got["readme.txt"], edited["readme.txt"])

    @covers("FR-W10-AC4", "CLI-DESIGN-6a-utc-stamp")
    def test_at_before_earliest_resolves_to_current(self):
        arc, src, _h = self.seed_archive()
        edited = self.make_multi_version(arc, src)
        out = self.work / "out"
        # A stamp far before any CLI save resolves to the current version.
        r = self.run_walrus("extract", arc, "-o", out, "--at", "2000-01-01T00:00:00Z")
        self.assert_code(r, 0)
        self.assert_tree_equals(out / "src", edited)

    @covers("FR-W10-AC4")
    def test_malformed_at_is_usage_error_rc2(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("extract", arc, "-o", self.work / "out",
                            "--at", "2026-10-08 12:00:00")
        self.assert_code(r, 2)

    @covers("FR-W10-AC4", "CLI-DESIGN-6-rc2")
    def test_two_selectors_is_rc2(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("extract", arc, "-o", self.work / "out",
                            "--index", "0", "--version", "original")
        self.assert_code(r, 2)

    @covers("CLI-DESIGN-5.2.1-extract-output", "FR-W11-AC7b")
    def test_output_recreates_the_nested_relative_tree(self):
        arc, _src, initial = self.seed_archive()
        out = self.work / "deep" / "out"      # parent does not yet exist
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_tree_equals(out / "src", initial)

    @covers("CLI-DESIGN-5.2.1-extract-overwrite", "CLI-DESIGN-6-rc2")
    def test_existing_target_refused_unless_force(self):
        arc, _src, _h = self.seed_archive()
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 2)
        self.assert_code(self.run_walrus("extract", arc, "-o", out, "--force"), 0)

    @covers("CLI-DESIGN-5.1-query-mask")
    def test_mask_extracts_only_the_subset(self):
        arc, _src, _h = self.seed_archive()
        out = self.work / "out"
        self.assert_code(
            self.run_walrus("extract", arc, "src/readme.txt", "-o", out), 0
        )
        got = self.tree_hashes(out / "src")
        self.assertEqual(sorted(got), ["readme.txt"])

    @covers("CLI-DESIGN-5.5-remove-nomatch", "CLI-DESIGN-6-rc8")
    def test_mask_matching_nothing_is_rc8(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("extract", arc, "no/such/file", "-o", self.work / "out")
        self.assert_code(r, 8)


if __name__ == "__main__":
    unittest.main(verbosity=2)
