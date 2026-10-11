"""Acceptance tests for global/usage behaviour and the return-code contract.

`--help`/`-h`/`help` print the usage banner and exit 0. Empty argv is usage +
RC 2. An unknown option is RC 2. Two operations is RC 2. A garbage archive
fails cleanly, non-zero, no panic. The suite's own RC-coverage self-check
asserts that each documented return code (0/2/4/8) is produced by at least one
representative invocation.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class UsageGlobalTests(WalrusCase):

    @covers("CLI-DESIGN-4-help", "CLI-DESIGN-6-rc0")
    def test_help_long_exits_zero_with_usage(self):
        r = self.run_walrus("--help")
        self.assert_code(r, 0)
        self.assert_output_contains(r, "USAGE")

    @covers("CLI-DESIGN-4-help")
    def test_help_short_exits_zero_with_usage(self):
        r = self.run_walrus("-h")
        self.assert_code(r, 0)
        self.assert_output_contains(r, "USAGE")

    @covers("CLI-DESIGN-4-help")
    def test_help_word_exits_zero_with_usage(self):
        r = self.run_walrus("help")
        self.assert_code(r, 0)
        self.assert_output_contains(r, "USAGE")

    @covers("CLI-DESIGN-4-help", "CLI-DESIGN-6-rc2")
    def test_empty_argv_is_usage_rc2(self):
        r = self.run_walrus()
        self.assert_code(r, 2)
        self.assert_output_contains(r, "USAGE")

    @covers("CLI-DESIGN-4-unknown-option", "CLI-DESIGN-6-rc2")
    def test_unknown_option_is_rc2(self):
        arc, _src, _h = self.seed_archive()
        self.assert_code(self.run_walrus("query", arc, "--bogus-flag"), 2)

    @covers("FR-W11-AC2", "CLI-DESIGN-6-rc2")
    def test_two_operations_is_rc2(self):
        arc, _src, _h = self.seed_archive()
        # `query` (canonical first token) plus the `-x` extract flag alias.
        self.assert_code(self.run_walrus("query", arc, "-x"), 2)

    @covers("FR-W11-AC6")
    def test_garbage_archive_fails_cleanly(self):
        junk = self.write_bytes(self.work / "garbage.wal", bytes([1, 2, 3, 4, 5, 6]))
        self.assert_nonzero_no_panic(self.run_walrus("extract", junk,
                                                     "-o", self.work / "o"))

    @covers("CLI-DESIGN-6-rc0", "CLI-DESIGN-6-rc2", "CLI-DESIGN-6-rc4",
            "CLI-DESIGN-6-rc8")
    def test_every_documented_return_code_is_producible(self):
        # One representative invocation per documented code, asserted together
        # so the suite proves its own RC coverage (CLI-DESIGN section 6).
        arc, src, _h = self.seed_archive()
        self.make_multi_version(arc, src)
        # RC 0: a plain query.
        self.assert_code(self.run_walrus("query", arc), 0)
        # RC 2: two selectors on extract.
        self.assert_code(
            self.run_walrus("extract", arc, "-o", self.work / "o2",
                            "--index", "0", "--version", "original"),
            2,
        )
        # RC 4: numeric index beyond last.
        self.assert_code(
            self.run_walrus("extract", arc, "-o", self.work / "o4", "--index", "999"),
            4,
        )
        # RC 8: remove a non-existent file.
        self.assert_code(self.run_walrus("remove", arc, "no-such.txt"), 8)


if __name__ == "__main__":
    unittest.main(verbosity=2)
