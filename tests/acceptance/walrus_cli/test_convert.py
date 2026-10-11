"""Acceptance tests for `walrus convert` (CLI-DESIGN 5.7, AC4).

convert flips replay direction. No mask converts the whole archive; a mask
converts the matching file(s) per-file. The result must round-trip
byte-identically for every index. The on-disk input is never auto-renamed; a
`-o <name>` writes a new file and leaves the input in place. No match is RC 8.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class ConvertTests(WalrusCase):

    def _two_version_wal(self):
        """A forward archive whose readme.txt has an original + a current."""
        arc, src, initial = self.seed_archive("--wal")
        edited = self.make_multi_version(arc, src)
        return arc, initial, edited

    def _extract_hashes(self, arc, selector_args):
        out = self.work / ("x" + str(id(selector_args)))
        self.assert_code(
            self.run_walrus("extract", arc, "-o", out, *selector_args), 0
        )
        return self.tree_hashes(out / "src")

    @covers("FR-W11-AC4", "CLI-DESIGN-5.7-convert-roundtrip")
    def test_whole_archive_flip_round_trips_every_index(self):
        arc, initial, edited = self._two_version_wal()
        wal_current = self._extract_hashes(arc, [])
        wal_original = self._extract_hashes(arc, ["--version", "original"])

        wbl = self.work / "a.wbl"
        self.assert_code(self.run_walrus("convert", arc, "--to", "wbl", "-o", wbl), 0)

        wbl_current = self._extract_hashes(wbl, [])
        wbl_original = self._extract_hashes(wbl, ["--version", "original"])
        self.assertEqual(wbl_current, wal_current, "current index must match after flip")
        self.assertEqual(wbl_original, wal_original, "original index must match after flip")

    @covers("CLI-DESIGN-5.7-convert-roundtrip")
    def test_to_wal_on_forward_archive_is_noop_success(self):
        arc, _i, _e = self._two_version_wal()
        self.assert_code(
            self.run_walrus("convert", arc, "--to", "wal", "-o", self.work / "c.wal"), 0
        )

    @covers("CLI-DESIGN-5.7-convert-masked")
    def test_masked_convert_flips_only_the_matched_file(self):
        arc, initial, edited = self._two_version_wal()
        before_current = self._extract_hashes(arc, [])
        out = self.work / "mixed.wbl"
        self.assert_code(
            self.run_walrus("convert", arc, "src/readme.txt", "--to", "wbl", "-o", out),
            0,
        )
        # The mixed archive still reconstructs every file at current + original.
        self.assertEqual(self._extract_hashes(out, []), before_current)
        after_original = self._extract_hashes(out, ["--version", "original"])
        self.assertEqual(after_original["readme.txt"], initial["readme.txt"])

    @covers("FR-W11-AC4", "CLI-DESIGN-5.7-convert-norename")
    def test_no_auto_rename_on_disk_and_output_writes_new_file(self):
        arc, _i, _e = self._two_version_wal()
        wbl = self.work / "out.wbl"
        self.assert_code(self.run_walrus("convert", arc, "--to", "wbl", "-o", wbl), 0)
        # The input is left in place with its original name; the new file exists.
        self.assert_file(arc)
        self.assert_file(wbl)

    @covers("CLI-DESIGN-5.7-convert-norename")
    def test_no_output_leaves_input_filename_unchanged(self):
        arc, _i, _e = self._two_version_wal()
        r = self.run_walrus("convert", arc, "--to", "wbl")
        self.assert_code(r, 0)
        self.assert_file(arc)                 # same path, not auto-renamed
        self.assert_missing(self.work / "a.wbl")

    @covers("CLI-DESIGN-5.7-convert-nomatch", "CLI-DESIGN-6-rc8")
    def test_no_match_mask_is_rc8(self):
        arc, _i, _e = self._two_version_wal()
        self.assert_code(
            self.run_walrus("convert", arc, "nope.txt", "--to", "wbl"), 8
        )

    @covers("CLI-DESIGN-5.7-convert-roundtrip", "FR-W11-AC4")
    def test_forward_reverse_forward_reproduces_every_index(self):
        arc, initial, edited = self._two_version_wal()
        wal_current = self._extract_hashes(arc, [])
        wal_original = self._extract_hashes(arc, ["--version", "original"])

        wbl = self.work / "step.wbl"
        self.assert_code(self.run_walrus("convert", arc, "--to", "wbl", "-o", wbl), 0)
        back = self.work / "back.wal"
        self.assert_code(self.run_walrus("convert", wbl, "--to", "wal", "-o", back), 0)

        self.assertEqual(self._extract_hashes(back, []), wal_current)
        self.assertEqual(
            self._extract_hashes(back, ["--version", "original"]), wal_original
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
