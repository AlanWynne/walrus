"""Acceptance tests for `-` / `--name` stdin ingestion (CLI-DESIGN 5.4e, AC9).

A single `-` operand reads content from stdin and requires `--name`. The misuse
rules are all RC 2. The `--name` value is stored verbatim and is validated
through the shared naming safety (an escaping or unrepresentable name is RC 2).
Binary stdin is rejected and leaves the archive unchanged.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class StdinTests(WalrusCase):

    @covers("FR-W11-AC9", "FR-W11-AC9a", "CLI-DESIGN-5.4e-stdin")
    def test_add_from_stdin_round_trips_exact_bytes(self):
        arc = self.work / "a.wal"
        content = b"piped content\nsecond line"      # exact bytes, no shell NL
        r = self.run_walrus("add", arc, "-", "--name", "piped.txt", "--wal",
                            stdin=content)
        self.assert_code(r, 0)
        r2 = self.run_walrus("cat", arc, "piped.txt")
        self.assert_code(r2, 0)
        self.assert_stdout_bytes(r2, content)

    @covers("FR-W11-AC9a", "CLI-DESIGN-5.4e-stdin")
    def test_add_from_stdin_with_nested_name(self):
        arc = self.work / "a.wal"
        content = b"nested stdin body"
        r = self.run_walrus("add", arc, "-", "--name", "a/b/c.txt", "--wal",
                            stdin=content)
        self.assert_code(r, 0)
        r2 = self.run_walrus("cat", arc, "a/b/c.txt")
        self.assert_stdout_bytes(r2, content)

    @covers("FR-W11-AC9", "FR-W11-AC9e")
    def test_update_from_stdin_on_tracked_name_adds_version(self):
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, "-", "--name", "p.txt",
                                         "--wal", stdin=b"v1"), 0)
        self.assert_code(self.run_walrus("update", arc, "-", "--name", "p.txt",
                                         stdin=b"v2 longer"), 0)
        r = self.run_walrus("query", arc)
        self.assertEqual(r.out_text.strip(), "p.txt")
        self.assertEqual(self.query_versions(arc, "p.txt"), 1)

    @covers("FR-W11-AC9e", "FR-W11-AC3", "CLI-DESIGN-6-rc8")
    def test_update_from_stdin_untracked_name_is_rc8(self):
        arc, _src, _h = self.seed_archive()
        r = self.run_walrus("update", arc, "-", "--name", "never-seen.txt",
                            stdin=b"body")
        self.assert_code(r, 8)

    @covers("FR-W11-AC9b", "CLI-DESIGN-6-rc2")
    def test_dash_without_name_is_rc2(self):
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, "-", "--wal", stdin=b"x"), 2)

    @covers("FR-W11-AC9b")
    def test_name_without_dash_is_rc2(self):
        arc = self.work / "a.wal"
        f = self.write(self.work / "f.txt", "f")
        self.assert_code(self.run_walrus("add", arc, f, "--name", "x.txt"), 2)

    @covers("FR-W11-AC9c")
    def test_dash_mixed_with_file_is_rc2(self):
        arc = self.work / "a.wal"
        f = self.write(self.work / "f.txt", "f")
        self.assert_code(self.run_walrus("add", arc, "-", f, "--name", "x.txt",
                                         "--wal", stdin=b"x"), 2)

    @covers("FR-W11-AC9c")
    def test_dash_twice_is_rc2(self):
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, "-", "-", "--name", "x.txt",
                                         "--wal", stdin=b"x"), 2)

    @covers("FR-W11-AC9c")
    def test_recurse_with_dash_is_rc2(self):
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, "-", "-R", "--name", "x.txt",
                                         "--wal", stdin=b"x"), 2)

    @covers("FR-W11-AC9d")
    def test_binary_stdin_rejected_and_archive_unchanged(self):
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--name", "b.bin", "--wal",
                            stdin=bytes([0, 1, 2, 0, 255]))
        self.assert_nonzero_no_panic(r)
        self.assert_missing(arc)

    @covers("FR-W11-AC9f")
    def test_name_is_stored_verbatim(self):
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, "-", "--name", "exact/kept.txt",
                                         "--wal", stdin=b"x"), 0)
        r = self.run_walrus("query", arc)
        self.assertEqual(r.out_text.strip(), "exact/kept.txt")

    @covers("FR-W11-AC9f", "CLI-DESIGN-5.4d-zipslip")
    def test_escaping_name_is_rc2(self):
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, "-", "--name", "../x.txt",
                                         "--wal", stdin=b"x"), 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
