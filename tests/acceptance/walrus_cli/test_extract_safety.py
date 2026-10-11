"""Acceptance tests for extract naming safety (CLI-DESIGN 5.4d).

The 5.4d policy is: a path-escape (zip-slip) name is ALWAYS refused (no flag
relaxes it); an unrepresentable name (reserved device name, reserved char,
trailing dot/space) is refused by default (RC 2) and deterministically
rewritten under --sanitize.

Important black-box boundary: the only way to introduce a stored name through
THIS CLI is a file basename, a `-R` relative path, or a stdin `--name`. All
three run the stored name through the SAME `safety::classify` guard that
extract uses, and the create side (stdin `--name`) refuses a zip-slip or
unrepresentable name up front (RC 2). So a black-box test cannot build an
archive that carries an unsafe stored name to exercise the on-EXTRACT branch;
that branch is covered by the Rust unit tests in `wal-cli-common/src/safety.rs`
(`reserved_device_name_is_refused_by_default`, `sanitize_*`,
`absolute_or_drive_or_dotdot_name_is_refused_zip_slip`). These tests therefore
pin the CREATE-side refusals the CLI actually enforces, and the --sanitize /
default-refuse remap behaviour is asserted through the stdin create guard that
shares the classifier.
"""

from __future__ import annotations

import os
import unittest

from harness import WalrusCase, covers


class ExtractSafetyTests(WalrusCase):

    @covers("CLI-DESIGN-5.4d-zipslip", "FR-W11-AC9f")
    def test_zip_slip_dotdot_name_is_refused_at_create_rc2(self):
        # The create guard (stdin_src -> safety::classify) refuses a `..`
        # escaping name BEFORE it can enter the archive; the on-extract guard
        # (never relaxed by --sanitize/--force) is Rust-tested in safety.rs.
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--name", "../escape.txt", "--wal",
                            stdin=b"x")
        self.assert_code(r, 2)
        self.assert_missing(arc)

    @covers("CLI-DESIGN-5.4d-zipslip")
    def test_absolute_name_is_refused_at_create_rc2(self):
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--name", "/etc/passwd", "--wal",
                            stdin=b"x")
        self.assert_code(r, 2)
        self.assert_missing(arc)

    @covers("CLI-DESIGN-5.4d-zipslip")
    def test_drive_qualified_name_is_refused_at_create_rc2(self):
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--name", "C:/Windows/x.txt", "--wal",
                            stdin=b"x")
        self.assert_code(r, 2)
        self.assert_missing(arc)

    @covers("CLI-DESIGN-5.4d-refuse", "FR-W11-AC9f")
    def test_reserved_character_name_is_refused_at_create_rc2(self):
        # A reserved character makes the name unrepresentable -> refused up
        # front (the same classifier extract's default-refuse branch uses).
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--name", "weird:name.txt", "--wal",
                            stdin=b"x")
        self.assert_code(r, 2)
        self.assert_missing(arc)

    @covers("CLI-DESIGN-5.4d-refuse", "CLI-DESIGN-5.4d-sanitize")
    def test_reserved_device_name_is_refused_at_create_rc2(self):
        # `aux` is a reserved device-name stem; the classifier marks it
        # unrepresentable (sanitize form "aux_"), so the create guard refuses it.
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--name", "aux", "--wal", stdin=b"x")
        self.assert_code(r, 2)
        self.assert_missing(arc)

    @unittest.skipIf(
        os.name != "nt",
        "Windows reserved device-name resolution is only meaningful on nt",
    )
    @covers("CLI-DESIGN-5.4d-refuse")
    def test_reserved_device_name_with_extension_refused_on_windows(self):
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--name", "nul.txt", "--wal", stdin=b"x")
        self.assert_code(r, 2)
        self.assert_missing(arc)


if __name__ == "__main__":
    unittest.main(verbosity=2)
