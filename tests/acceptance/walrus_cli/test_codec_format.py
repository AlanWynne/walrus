"""Acceptance tests for the container codec/format boundary (CLI-DESIGN 1.4).

A freshly written archive begins with the zstd frame magic. A file carrying a
different magic (gzip, a deferred codec) or random bytes must error cleanly
(non-zero, no panic) when passed to a read command -- the reader sniffs the
codec from the magic bytes, not the extension.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers, gzip_magic, zstd_magic


class CodecFormatTests(WalrusCase):

    @covers("CLI-DESIGN-1.4-codec-zstd")
    def test_written_archive_begins_with_zstd_magic(self):
        arc, _src, _h = self.seed_archive()
        self.assertEqual(self.read_archive_magic(arc), zstd_magic())

    @covers("CLI-DESIGN-1.4-codec-gzip-defer")
    def test_gzip_magic_file_errors_cleanly(self):
        fake = self.write_bytes(self.work / "g.wal", gzip_magic() + bytes([0] * 32))
        self.assert_nonzero_no_panic(self.run_walrus("query", fake))

    @covers("CLI-DESIGN-1.4-codec-garbage")
    def test_random_magic_file_errors_cleanly(self):
        fake = self.write_bytes(self.work / "r.wal", bytes([0x7A, 0x9C, 0x00, 0xFF, 0x11]))
        self.assert_nonzero_no_panic(self.run_walrus("query", fake))


if __name__ == "__main__":
    unittest.main(verbosity=2)
