"""Regression guard for the ustar long-path crash (CLI-DESIGN 2.1).

The original bug: a deep recursed relative path slugged into a tracked-file id
that grew with path depth and overflowed the ustar 100-byte member-name field,
so `add -R` failed with "provided value is too long when setting path for
files/...". The short `f<N>` id scheme (Change 1) plus the bounded `.wchg`
member name keep every derived member path ustar-legal, so a deep tree now adds
cleanly and round-trips byte-identical.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class LongPathTests(WalrusCase):

    @covers("CLI-DESIGN-2.1-shortid-longpath", "FR-W11-AC7", "FR-W11-AC7b")
    def test_deep_long_path_add_recurse_round_trips_without_ustar_overflow(self):
        # A relative path whose OLD slug id would exceed 100 bytes: nested dirs
        # modelled on the user's reported crash, totalling well over 120 chars,
        # ending in a .rs-like leaf file.
        root = self.work / "Walrus-TestData-Base"
        deep_rel = (
            "crates/walker/gui/target/debug/build/"
            "glutin_wgl_sys-31fa4596c8d30f15/out/"
            "generated/bindings/platform/windows/wgl_bindings.rs"
        )
        self.write(root / deep_rel, "// generated bindings\npub const X: u32 = 1;")
        self.assertGreater(
            len(f"Walrus-TestData-Base/{deep_rel}"),
            120,
            "fixture path must be deep enough to trip the old bug",
        )
        arc = self.work / "a.wal"
        initial = self.tree_hashes(root)

        r = self.run_walrus("add", arc, root, "-R", "--wal")
        self.assert_code(r, 0)
        # The exact ustar error from the report must NOT appear.
        self.assertNotIn("too long", r.err_text, f"ustar overflow regressed\n{r!r}")
        self.assert_file(arc)

        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_tree_equals(out / "Walrus-TestData-Base", initial)


if __name__ == "__main__":
    unittest.main(verbosity=2)
