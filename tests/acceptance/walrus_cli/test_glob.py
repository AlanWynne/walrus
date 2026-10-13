"""Acceptance tests for walrus-side wildcard FILE-INPUT expansion (CLI-DESIGN 5.4f).

`add` and `update` expand a file-input operand that contains wildcard
metacharacters themselves, using a portable glob, so `walrus add base.wal *.md`
behaves IDENTICALLY on Windows and POSIX. Because `harness.run_walrus` passes an
argv list via subprocess with NO shell expansion, a literal `"*.md"` argument
faithfully simulates the Windows-cmd case on every platform -- the headline fix.

Resolution order per operand (section 5.4f):
  1. an EXISTING literal path wins as-is (no globbing);
  2. else a metacharacter operand (`*`, `?`, `[`) is globbed relative to CWD,
     sorted deterministically, and a no-match is RC 2 with a clear diagnostic;
  3. else a literal, non-existent, no-metacharacter operand is unchanged (the
     downstream file-not-found read error).

A plain `*` stays within one directory level; only `**` recurses. A binary file
among the matches aborts the batch transactionally (no archive).
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class GlobTests(WalrusCase):

    @covers("CLI-DESIGN-5.4f-add-glob", "CLI-DESIGN-6-rc0")
    def test_add_star_md_expands_to_all_matching_md_files(self):
        # The headline Windows fix: a literal "*.md" operand (as cmd/PowerShell
        # hand walrus) expands to every .md file in the directory, not .txt.
        self.write(self.work / "b.md", "bee")
        self.write(self.work / "a.md", "aay")
        self.write(self.work / "c.txt", "see")
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "*.md", "--wal", cwd=self.work)
        self.assert_code(r, 0)
        q = self.run_walrus("query", arc)
        names = sorted(line for line in q.out_text.splitlines() if line)
        self.assertEqual(names, ["a.md", "b.md"])

    @covers("CLI-DESIGN-5.4f-add-glob")
    def test_subdir_qualified_pattern_matches_in_that_subdir(self):
        self.write(self.work / "sub" / "x.txt", "ex")
        self.write(self.work / "sub" / "y.txt", "why")
        self.write(self.work / "top.txt", "top")
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "sub/*.txt", "--wal", cwd=self.work)
        self.assert_code(r, 0)
        q = self.run_walrus("query", arc)
        names = sorted(line for line in q.out_text.splitlines() if line)
        # Stored under basename for a plain file arg (CLI-DESIGN 5.4b).
        self.assertEqual(names, ["x.txt", "y.txt"])

    @covers("CLI-DESIGN-5.4f-no-recurse")
    def test_single_star_does_not_cross_slash_but_double_star_recurses(self):
        self.write(self.work / "top.md", "top")
        self.write(self.work / "sub" / "deep.md", "deep")
        # Plain * stays in one directory level: only top.md.
        arc1 = self.work / "one.wal"
        r1 = self.run_walrus("add", arc1, "*.md", "--wal", cwd=self.work)
        self.assert_code(r1, 0)
        names1 = sorted(
            line for line in self.run_walrus("query", arc1).out_text.splitlines() if line
        )
        self.assertEqual(names1, ["top.md"])
        # ** recurses across segments: both files (both stored under basename).
        arc2 = self.work / "two.wal"
        r2 = self.run_walrus("add", arc2, "**/*.md", "--wal", cwd=self.work)
        self.assert_code(r2, 0)
        names2 = sorted(
            line for line in self.run_walrus("query", arc2).out_text.splitlines() if line
        )
        self.assertEqual(names2, ["deep.md", "top.md"])

    @covers("CLI-DESIGN-5.4f-literal-wins", "CLI-DESIGN-5.3-add-basename")
    def test_literal_existing_path_added_as_is_no_globbing_side_effects(self):
        # A shell-pre-expanded (POSIX) command line hands walrus literal paths;
        # an existing literal is used verbatim and never re-globbed, so only the
        # named file is tracked even though another .md sits beside it.
        a = self.write(self.work / "a.md", "aay body")
        self.write(self.work / "b.md", "bee body")
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, a, "--wal", cwd=self.work)
        self.assert_code(r, 0)
        q = self.run_walrus("query", arc)
        self.assertEqual(q.out_text.strip(), "a.md")
        cat = self.run_walrus("cat", arc, "a.md")
        self.assert_stdout_bytes(cat, b"aay body")

    @covers("CLI-DESIGN-5.4f-nomatch-rc2", "CLI-DESIGN-6-rc2")
    def test_pattern_matching_nothing_is_rc2_and_creates_no_archive(self):
        self.write(self.work / "a.md", "aay")
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "*.zzz", "--wal", cwd=self.work)
        self.assert_code(r, 2)
        self.assert_stderr_contains(r, "no files match")
        self.assert_missing(arc)

    @covers("CLI-DESIGN-5.4f-add-glob", "CLI-DESIGN-5.4a-transactional")
    def test_binary_file_among_glob_matches_aborts_transactionally(self):
        self.write(self.work / "ok.md", "text ok")
        self.write_bytes(self.work / "blob.md", bytes([0, 1, 2, 0, 255, 254]))
        arc = self.work / "a.wal"
        r = self.run_walrus("add", arc, "*.md", "--wal", cwd=self.work)
        self.assert_nonzero_no_panic(r)
        self.assert_missing(arc)

    @covers("CLI-DESIGN-5.4f-update-glob", "CLI-DESIGN-6-rc0")
    def test_update_star_md_reexpands_and_records_new_versions(self):
        self.write(self.work / "a.md", "a v1")
        self.write(self.work / "b.md", "b v1")
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, "*.md", "--wal", cwd=self.work), 0)
        # Change one file, then update via the SAME pattern (re-expanded).
        self.write(self.work / "a.md", "a v2 changed")
        r = self.run_walrus("update", arc, "*.md", cwd=self.work)
        self.assert_code(r, 0)
        self.assertEqual(self.query_versions(arc, "a.md"), 1)
        cat = self.run_walrus("cat", arc, "a.md")
        self.assert_stdout_bytes(cat, b"a v2 changed")

    @covers("CLI-DESIGN-5.4f-add-glob")
    def test_absolute_qualified_pattern_matches_same_as_bare(self):
        self.write(self.work / "a.md", "aay")
        self.write(self.work / "b.md", "bee")
        self.write(self.work / "c.txt", "see")
        arc = self.work / "a.wal"
        pattern = str(self.work / "*.md")
        r = self.run_walrus("add", arc, pattern, "--wal", cwd=self.work)
        self.assert_code(r, 0)
        names = sorted(
            line for line in self.run_walrus("query", arc).out_text.splitlines() if line
        )
        self.assertEqual(names, ["a.md", "b.md"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
