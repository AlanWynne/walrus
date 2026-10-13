"""Scale round-trip for `walrus add -R` (CLI-DESIGN 5.3).

A single deep tree with many small files at mixed directory depths must add
with `-R` and extract back byte-identical, for both the forward (`.wal`) and
reverse (`.wbl`) variants. This exercises the recurse walk, stored-name
mapping, and reconstruct engine at a scale a single hand-written fixture does
not reach, while staying fully deterministic (names and content derive from a
loop index -- no randomness, no clock, no set/dict iteration order).
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


# The relative subdirectory for each file index, chosen so files land at the
# root, 2-3 dirs deep, and 5+ dirs deep. Deterministic function of the index.
def _rel_dir_for(index: int) -> str:
    bucket = index % 3
    if bucket == 0:
        # At the root.
        return ""
    if bucket == 1:
        # 2-3 dirs deep.
        return f"mid/level{index % 2}/group{index % 4}"
    # 5+ dirs deep.
    return f"deep/a/b/c/d/e{index % 3}/f{index % 2}"


class ScaleRoundTripTests(WalrusCase):

    def _build_scale_tree(self):
        """Build ~50 deterministic small text files at mixed depths.

        Returns ``(root, initial_hashes, file_count)``.
        """
        root = self.work / "scale-src"
        file_count = 50
        for i in range(file_count):
            rel_dir = _rel_dir_for(i)
            name = f"file_{i:02}.txt"
            rel = f"{rel_dir}/{name}" if rel_dir else name
            self.write(root / rel, f"file {i}\nline two")
        return root, self.tree_hashes(root), file_count

    def _assert_scale_round_trip(self, variant: str, ext: str) -> None:
        root, initial, file_count = self._build_scale_tree()
        # Sanity: the builder really produced the expected number of files.
        self.assertEqual(len(initial), file_count)

        arc = self.work / f"scale.{ext}"
        self.assert_code(self.run_walrus("add", arc, root, "-R", variant), 0)
        self.assert_file(arc)

        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)

        # Extract reproduces the tree under <out>/<root-basename>.
        extracted = out / root.name
        self.assert_tree_equals(extracted, initial)

        # No file dropped or added: the extracted count equals the source.
        extracted_hashes = self.tree_hashes(extracted)
        self.assertEqual(
            len(extracted_hashes), len(initial),
            "extracted file count must equal the source file count",
        )

    @covers("CLI-DESIGN-5.3-add-scale", "FR-W11-AC7", "FR-W11-AC7b")
    def test_many_file_mixed_depth_tree_wal_round_trips_byte_identical(self):
        self._assert_scale_round_trip("--wal", "wal")

    @covers("CLI-DESIGN-5.3-add-scale", "FR-W11-AC7", "FR-W11-AC7b")
    def test_many_file_mixed_depth_tree_wbl_round_trips_byte_identical(self):
        self._assert_scale_round_trip("--wbl", "wbl")


if __name__ == "__main__":
    unittest.main(verbosity=2)
