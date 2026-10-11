"""Acceptance tests for `walrus update` (CLI-DESIGN 5.4).

update records a new version of an ALREADY-tracked file. A file absent from the
archive is RC 8. The batch is transactional: a mid-batch RC 8 leaves the
archive byte-for-byte unchanged. A no-change save is RC 0 with a note.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class UpdateTests(WalrusCase):

    @covers("FR-W11-AC6")
    def test_update_existing_file_adds_a_version(self):
        arc, src, _h = self.seed_archive()
        self.write(src / "readme.txt", "hello walrus\nline two UPDATED")
        self.assert_code(self.run_walrus("update", arc, src, "-R"), 0)
        r = self.run_walrus("query", arc)
        self.assert_stdout_contains(r, "src/readme.txt\t1")

    @covers("FR-W11-AC3", "CLI-DESIGN-5.4-update-refuse", "CLI-DESIGN-6-rc8")
    def test_update_untracked_file_is_rc8(self):
        arc, _src, _h = self.seed_archive()
        stray = self.write(self.work / "stray.txt", "not tracked")
        self.assert_code(self.run_walrus("update", arc, stray), 8)

    @covers("FR-W11-AC7e", "CLI-DESIGN-5.4a-transactional")
    def test_recurse_update_with_untracked_file_is_rc8_and_unchanged(self):
        arc, src, _h = self.seed_archive()
        before = self.read_archive_magic(arc)
        before_bytes = arc.read_bytes()
        # Add a new untracked file into the tree, then update -R: the untracked
        # member makes the batch RC 8 and the archive must be unchanged.
        self.write(src / "newcomer.txt", "surprise")
        self.write(src / "readme.txt", "hello walrus\nline two edited")
        r = self.run_walrus("update", arc, src, "-R")
        self.assert_code(r, 8)
        self.assertEqual(arc.read_bytes(), before_bytes,
                         "a failed -R update must leave the archive unchanged")
        self.assertEqual(self.read_archive_magic(arc), before)

    @covers("CLI-DESIGN-5.4-nochange", "CLI-DESIGN-6-rc0")
    def test_no_change_update_is_noop_with_note(self):
        arc, src, _h = self.seed_archive()
        r = self.run_walrus("update", arc, src, "-R")        # nothing edited
        self.assert_code(r, 0)
        self.assert_stderr_contains(r, "no change")
        q = self.run_walrus("query", arc)
        self.assert_stdout_contains(q, "src/readme.txt\t0")

    @covers("FR-W11-AC8", "CLI-DESIGN-6a-utc-stamp")
    def test_updates_are_stamped_so_at_addressing_resolves(self):
        # AC8: add/update stamp each recorded change with a UTC instant, which
        # is what makes --at addressing answerable. Build a two-version archive
        # through the CLI, then prove a valid --at query resolves without error
        # and returns a known version (a far-future instant selects current).
        arc, src, _h = self.seed_archive()
        edited = self.make_multi_version(arc, src)
        out = self.work / "at-current"
        r = self.run_walrus("extract", arc, "-o", out, "--at", "2999-01-01T00:00:00Z")
        self.assert_code(r, 0)
        # The newest-at-or-before a far-future instant is the current version;
        # a resolvable answer proves the change records carried a timestamp.
        self.assert_tree_equals(out / "src", edited)

    @covers("CLI-DESIGN-5.3-generations")
    def test_generations_on_update_trims(self):
        arc, src, _h = self.seed_archive()
        for body in ("e1", "e2 bb", "e3 ccc", "e4 dddd"):
            self.write(src / "readme.txt", body)
            self.assert_code(self.run_walrus("update", arc, src, "-R",
                                             "--generations", "2"), 0)
        r = self.run_walrus("query", arc)
        row = next(l for l in r.out_text.splitlines() if l.startswith("src/readme.txt\t"))
        self.assertLessEqual(int(row.split("\t")[1]), 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
