"""Acceptance tests for `walrus add` (CLI-DESIGN 5.3).

add creates a tracked file or records a new version. Sources are file
arguments, a `-R` directory walk, or a single `-` stdin stream. A directory
without `-R` is RC 2. The create-time direction follows the seed order
(explicit flag -> brand-new-archive extension -> default forward) and is
ignored for an already-tracked file. A binary input aborts the whole batch
transactionally.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class AddTests(WalrusCase):

    @covers("CLI-DESIGN-5.3-add-create", "FR-W11-AC6", "CLI-DESIGN-6-rc0")
    def test_create_wal_from_recursed_tree(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, src, "-R", "--wal")
        self.assert_code(r, 0)
        self.assert_file(arc)
        self.assertEqual(self.read_archive_magic(arc), b"\x28\xb5\x2f\xfd")

    @covers("CLI-DESIGN-5.3-add-create", "FR-W11-AC5")
    def test_create_wbl_round_trips(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wbl"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wbl"), 0)
        initial = self.tree_hashes(src)
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_tree_equals(out / "src", initial)

    @covers("CLI-DESIGN-5.3-add-basename", "FR-W11-AC6")
    def test_single_file_arg_is_stored_under_its_basename(self):
        # Regression guard: a plain (possibly absolute) file argument is stored
        # under its basename, never its full path, and round-trips via cat.
        f = self.write(self.work / "nested" / "one.txt", "one body")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, f, "--wal"), 0)
        r = self.run_walrus("query", arc)
        self.assert_code(r, 0)
        self.assertEqual(r.out_text.strip(), "one.txt\t0")
        r2 = self.run_walrus("cat", arc, "one.txt")
        self.assert_code(r2, 0)
        self.assert_stdout_bytes(r2, b"one body")

    @covers("CLI-DESIGN-5.3-add-create")
    def test_multiple_file_args_in_one_add(self):
        a = self.write(self.work / "a.txt", "aa")
        b = self.write(self.work / "b.txt", "bb")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, a, b, "--wal"), 0)
        r = self.run_walrus("query", arc)
        names = sorted(line.split("\t")[0] for line in r.out_text.splitlines() if line)
        self.assertEqual(names, ["a.txt", "b.txt"])

    @covers("FR-W11-AC6")
    def test_second_add_changed_content_adds_a_version(self):
        f = self.write(self.work / "f.txt", "v1")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, f, "--wal"), 0)
        self.write(f, "v2 different")
        self.assert_code(self.run_walrus("add", arc, f), 0)
        r = self.run_walrus("query", arc)
        self.assertEqual(r.out_text.strip(), "f.txt\t1")

    @covers("CLI-DESIGN-5.4-nochange", "CLI-DESIGN-6-rc0")
    def test_second_add_unchanged_is_noop_with_note(self):
        f = self.write(self.work / "f.txt", "same")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, f, "--wal"), 0)
        r = self.run_walrus("add", arc, f)
        self.assert_code(r, 0)
        self.assert_stderr_contains(r, "no change")
        q = self.run_walrus("query", arc)
        self.assertEqual(q.out_text.strip(), "f.txt\t0")

    @covers("FR-W11-AC7a", "CLI-DESIGN-6-rc2")
    def test_directory_without_recurse_is_rc2(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src), 2)

    @covers("FR-W11-AC7", "FR-W11-AC7b", "CLI-DESIGN-5.4b-recurse")
    def test_recurse_stores_relative_tree_and_round_trips(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        initial = self.tree_hashes(src)
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_tree_equals(out / "src", initial)

    @covers("FR-W11-AC7c")
    def test_recurse_ingests_in_deterministic_sorted_order(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        r = self.run_walrus("query", arc)
        names = [line.split("\t")[0] for line in r.out_text.splitlines() if line]
        self.assertEqual(names, sorted(names), "recursion walk must be sorted")

    @covers("FR-W11-AC7d", "CLI-DESIGN-5.4a-transactional")
    def test_recurse_binary_file_aborts_batch_and_no_archive(self):
        src = self.work / "src"
        self.write(src / "readme.txt", "text ok")
        self.write_bytes(src / "blob.bin", bytes([0, 1, 2, 0, 255, 254]))
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, src, "-R", "--wal")
        self.assert_nonzero_no_panic(r)
        self.assert_missing(arc)

    @covers("CLI-DESIGN-5.3-add-create")
    def test_binary_single_file_rejected_and_no_archive(self):
        blob = self.write_bytes(self.work / "blob.bin", bytes([0, 1, 2, 0, 255]))
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, blob, "--wal")
        self.assert_nonzero_no_panic(r)
        self.assert_missing(arc)

    @covers("CLI-DESIGN-5.3-direction", "FR-W11-AC5")
    def test_wbl_extension_seeds_a_working_reverse_archive(self):
        # No explicit flag: the `.wbl` extension seeds the reverse variant; the
        # archive still round-trips (direction took effect without error).
        f = self.write(self.work / "f.txt", "body")
        arc = self.work / "a.wbl"
        self.assert_code(self.run_walrus("add", arc, f), 0)
        r = self.run_walrus("cat", arc, "f.txt")
        self.assert_stdout_bytes(r, b"body")

    @covers("CLI-DESIGN-5.3-direction", "FR-W11-AC5")
    def test_bare_name_defaults_to_forward(self):
        f = self.write(self.work / "f.txt", "body")
        arc = self.work / "archive"       # no extension -> default forward
        self.assert_code(self.run_walrus("add", arc, f), 0)
        # A no-op `convert --to wal` on a forward archive succeeds.
        self.assert_code(self.run_walrus("convert", arc, "--to", "wal",
                                         "-o", self.work / "c"), 0)

    @covers("FR-W11-AC5")
    def test_direction_flag_ignored_for_already_tracked_file(self):
        # A file first created forward keeps forward even if a later add names
        # --wbl; the add still succeeds and the content round-trips.
        f = self.write(self.work / "f.txt", "v1")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, f, "--wal"), 0)
        self.write(f, "v2 changed")
        self.assert_code(self.run_walrus("add", arc, f, "--wbl"), 0)
        r = self.run_walrus("cat", arc, "f.txt")
        self.assert_stdout_bytes(r, b"v2 changed")

    @covers("CLI-DESIGN-5.3-generations")
    def test_generations_caps_recorded_versions(self):
        f = self.write(self.work / "f.txt", "v1")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, f, "--wal", "--generations", "2"), 0)
        for body in ("v2 aaa", "v3 bbbb", "v4 ccccc"):
            self.write(f, body)
            self.assert_code(self.run_walrus("add", arc, f, "--generations", "2"), 0)
        r = self.run_walrus("query", arc)
        count = int(r.out_text.strip().split("\t")[1])
        self.assertLessEqual(count, 2, "generations cap must bound the index size")

    @covers("CLI-DESIGN-5.3-direction", "CLI-DESIGN-6-rc2")
    def test_conflicting_direction_flags_is_rc2(self):
        f = self.write(self.work / "f.txt", "body")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, f, "--wal", "--wbl"), 2)


if __name__ == "__main__":
    unittest.main(verbosity=2)
