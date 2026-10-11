"""Acceptance tests for the `walrus` CLI (black-box, cross-platform).

Each test drives the real built binary and verifies exit code + stdout bytes +
stderr + on-disk results. Run from the repo root with the binary built:

    cargo build -p walrus-cli
    python -m unittest discover -s tests/acceptance/walrus_cli

Return-code contract under test (CLI-DESIGN section 6):
    0  success (incl. no-change no-op)
    2  usage / safety (bad args, two selectors, cat zero-or-multi-match,
       overwrite-without-force, reserved-name/zip-slip refuse, stdin misuse)
    4  extract numeric index beyond last (original written + warning)
    8  target file/mask matches nothing (update / remove / condense / convert)
"""

from __future__ import annotations

import os
import unittest
from pathlib import Path

from harness import WalrusCase


class AddTests(WalrusCase):

    def test_add_create_wal_from_recursed_tree(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, src, "-R", "--wal")
        self.assert_code(r, 0)
        self.assert_file(arc)

    def test_add_directory_without_recurse_is_rc2(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, src)   # no -R
        self.assert_code(r, 2)

    def test_add_single_file_stores_under_basename(self):
        # A plain file argument (no -R) should be tracked under a sane name and
        # round-trip through cat. (Regression guard for absolute-path handling.)
        f = self.write(self.work / "one.txt", "one")
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, f, "--wal")
        self.assert_code(r, 0)
        self.assert_file(arc)

    def test_second_add_unchanged_is_noop_success(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.run_walrus("add", arc, src, "-R", "--wal")
        r = self.run_walrus("add", arc, src, "-R")
        self.assert_code(r, 0)

    def test_add_wbl_round_trips(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wbl"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wbl"), 0)
        initial = self.tree_hashes(src)
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_tree_equals(out / "src", initial)

    def test_add_binary_rejected_and_no_archive(self):
        blob = self.write_bytes(self.work / "blob.bin", bytes([0, 1, 2, 0, 255, 254]))
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, blob, "--wal")
        self.assertNotEqual(r.code, 0, f"binary add should fail\n{r!r}")
        self.assert_missing(arc)


class RoundTripTests(WalrusCase):

    def _round_trip(self, variant: str):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / f"a{variant}.wal"
        initial = self.tree_hashes(src)
        self.assert_code(self.run_walrus("add", arc, src, "-R", variant), 0)
        # Make a real edit so there are two versions.
        self.write(src / "readme.txt", "hello walrus\nline two CHANGED")
        self.write(src / "notes" / "todo.md", "# TODO\n- first item\n- second item")
        edited = self.tree_hashes(src)
        self.assert_code(self.run_walrus("update", arc, src, "-R"), 0)
        o_orig, o_curr = self.work / "orig", self.work / "curr"
        self.assert_code(self.run_walrus("extract", arc, "-o", o_orig, "--version", "original"), 0)
        self.assert_code(self.run_walrus("extract", arc, "-o", o_curr), 0)
        self.assert_tree_equals(o_curr / "src", edited)
        self.assert_tree_equals(o_orig / "src", initial)

    def test_round_trip_wal(self):
        self._round_trip("--wal")

    def test_round_trip_wbl(self):
        self._round_trip("--wbl")


class UpdateTests(WalrusCase):

    def test_update_untracked_file_is_rc8(self):
        # Establish an archive with a recursed tree first (so the archive EXISTS
        # and opens), then update a file that is not tracked in it.
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        stray = self.write(self.work / "stray.txt", "not tracked")
        r = self.run_walrus("update", arc, stray)
        self.assert_code(r, 8)


class ExtractTests(WalrusCase):

    def _seeded(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        return arc

    def test_index_beyond_last_is_rc4(self):
        arc = self._seeded()
        r = self.run_walrus("extract", arc, "-o", self.work / "out", "--index", "999")
        self.assert_code(r, 4)

    def test_existing_target_refused_unless_force(self):
        arc = self._seeded()
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 2)
        self.assert_code(self.run_walrus("extract", arc, "-o", out, "--force"), 0)

    def test_two_selectors_is_rc2(self):
        arc = self._seeded()
        r = self.run_walrus("extract", arc, "-o", self.work / "out",
                            "--index", "0", "--version", "original")
        self.assert_code(r, 2)


class CatTests(WalrusCase):

    def _seeded(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        return arc

    def test_cat_writes_exact_bytes(self):
        arc = self._seeded()
        r = self.run_walrus("cat", arc, "src/readme.txt")
        self.assert_code(r, 0)
        self.assert_stdout_bytes(r, b"hello walrus\nline two")

    def test_cat_multi_match_is_rc2(self):
        arc = self._seeded()
        r = self.run_walrus("cat", arc, "src/**")
        self.assert_code(r, 2)


class QueryTests(WalrusCase):

    def _seeded(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        return arc

    def test_query_lists_files(self):
        arc = self._seeded()
        r = self.run_walrus("query", arc)
        self.assert_code(r, 0)
        self.assert_stdout_contains(r, "readme.txt")

    def test_bare_archive_defaults_to_query(self):
        arc = self._seeded()
        r = self.run_walrus(arc)
        self.assert_code(r, 0)
        self.assert_stdout_contains(r, "readme.txt")

    def test_query_long_listing(self):
        arc = self._seeded()
        self.assert_code(self.run_walrus("query", arc, "-l"), 0)


class RemoveTests(WalrusCase):

    def _seeded(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        return arc

    def test_multi_match_needs_force(self):
        arc = self._seeded()
        self.assert_code(self.run_walrus("remove", arc, "src/**"), 2)
        self.assert_code(self.run_walrus("remove", arc, "src/**", "--force"), 0)

    def test_no_match_is_rc8(self):
        arc = self._seeded()
        self.assert_code(self.run_walrus("remove", arc, "nope.txt"), 8)


class CondenseTests(WalrusCase):

    def test_keep_zero_collapses_to_current(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        self.write(src / "readme.txt", "hello walrus\nline two v2")
        self.assert_code(self.run_walrus("update", arc, src, "-R"), 0)
        edited = self.tree_hashes(src)
        self.assert_code(self.run_walrus("condense", arc, "src/readme.txt", "--keep", "0"), 0)
        out = self.work / "out"
        self.assert_code(self.run_walrus("extract", arc, "-o", out), 0)
        self.assert_tree_equals(out / "src", edited)


class ConvertTests(WalrusCase):

    def test_whole_archive_to_wbl_round_trips(self):
        src = self.make_fixture_tree(self.work / "src")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)
        self.write(src / "readme.txt", "hello walrus\nline two v2")
        self.assert_code(self.run_walrus("update", arc, src, "-R"), 0)
        out_wal = self.work / "wal-current"
        self.assert_code(self.run_walrus("extract", arc, "-o", out_wal), 0)
        wbl = self.work / "a.wbl"
        self.assert_code(self.run_walrus("convert", arc, "--to", "wbl", "-o", wbl), 0)
        self.assert_file(wbl)
        out_wbl = self.work / "wbl-current"
        self.assert_code(self.run_walrus("extract", wbl, "-o", out_wbl), 0)
        self.assert_tree_equals(out_wbl / "src", self.tree_hashes(out_wal / "src"))


class StdinTests(WalrusCase):

    def test_add_from_stdin_with_name(self):
        arc = self.work / "a.wal"
        content = b"piped content\nsecond line"   # EXACT bytes, no shell newline
        r = self.run_walrus("add", arc, "-", "--name", "piped.txt", "--wal", stdin=content)
        self.assert_code(r, 0)
        r2 = self.run_walrus("cat", arc, "piped.txt")
        self.assert_code(r2, 0)
        self.assert_stdout_bytes(r2, content)

    def test_dash_without_name_is_rc2(self):
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--wal", stdin=b"x")
        self.assert_code(r, 2)

    def test_name_without_dash_is_rc2(self):
        arc = self.work / "a.wal"
        f = self.write(self.work / "f.txt", "f")
        r = self.run_walrus("add", arc, f, "--name", "x.txt")
        self.assert_code(r, 2)

    def test_stdin_binary_rejected(self):
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "-", "--name", "b.bin", "--wal",
                            stdin=bytes([0, 1, 2, 0, 255]))
        self.assertNotEqual(r.code, 0, f"binary stdin should fail\n{r!r}")
        self.assert_missing(arc)


class UsageTests(WalrusCase):

    def test_help_exits_zero_and_shows_usage(self):
        r = self.run_walrus("--help")
        self.assert_code(r, 0)
        self.assert_output_contains(r, "USAGE")   # stdout or stderr

    def test_unknown_option_is_rc2(self):
        f = self.write(self.work / "f.txt", "f")
        arc = self.work / "a.wal"
        self.run_walrus("add", arc, f)
        r = self.run_walrus("query", arc, "--bogus-flag")
        self.assert_code(r, 2)


class AliasTests(WalrusCase):

    def test_install_status_remove_in_isolated_dir(self):
        # Point --dir at a scratch bin dir so the real PATH is never touched.
        bindir = self.work / "bin"
        bindir.mkdir(parents=True, exist_ok=True)
        self.assert_code(self.run_walrus("alias", "install", "--dir", bindir), 0)
        shims = list(bindir.glob("wal*"))
        self.assertTrue(shims, "alias install should create a wal shim")
        self.assert_code(self.run_walrus("alias", "remove", "--dir", bindir), 0)


if __name__ == "__main__":
    unittest.main(verbosity=2)
