"""Acceptance tests for `walrus search` (CLI-DESIGN 5.6).

Search finds a string in one version of each selected tracked file and reports
WHERE it was found (file, version index, byte offset, 1-based line, 1-based
column) as a Markdown report (default) or plain text. The engine is shared with
`walx search`, so both binaries behave identically; these tests drive the built
`walrus` binary end to end.

Command shape:
  walrus search <archive> <string> [file-mask] [selector]
                [-i|--ignore-case] [--regex] [--format markdown|plain]

Return codes (CLI-DESIGN section 6):
  0 found, 1 no match (grep convention; search-specific), 2 usage,
  4 numeric index beyond last (clamp + warn).

These assert on return codes and the PRESENCE of report header fields / table
columns, not exact whitespace -- mirroring crates/walrus-cli/tests/search.rs so
the acceptance suite and the Rust integration tests agree on the contract.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


class SearchTests(WalrusCase):

    def seed_search_archive(self):
        """Build a small multi-file .wal with known, deterministic content so
        search needles resolve predictably. Returns the archive path.

        Files (each added as a lone file so it is stored under its BASENAME --
        a plain file argument is stored basename-only per CLI-DESIGN 5.4b, which
        keeps the mask test independent of the -R recursion-root name prefix):
          notes.txt -> contains 'cats' (mixed-case 'Cats' too) for the
                       case-sensitivity and literal-match cases.
          main.rs   -> contains 'main' only here, for the file-mask case.
        """
        notes = self.write(self.work / "notes.txt", "cats and dogs\nCats are great\n")
        main = self.write(self.work / "main.rs", "fn main() {\n    // entry\n}\n")
        arc = self.work / "fixture.wal"
        # Two single-file adds into one archive: each stored under its basename.
        self.assert_code(self.run_walrus("add", arc, notes, "--wal"), 0)
        self.assert_code(self.run_walrus("add", arc, main), 0)
        return arc

    @covers("CLI-DESIGN-5.6-search-found", "CLI-DESIGN-6-rc0")
    def test_literal_found_prints_markdown_report_and_exits_zero(self):
        arc = self.seed_search_archive()
        r = self.run_walrus("search", arc, "cats")
        self.assert_code(r, 0)
        # The Markdown report header carries the archive, the searched version
        # index, and the version timestamp, then a located-match table.
        self.assert_stdout_contains(r, "Search report")
        self.assert_stdout_contains(r, "fixture.wal")
        self.assert_stdout_contains(r, "Version index: 0")
        self.assert_stdout_contains(r, "Version timestamp:")
        self.assert_stdout_contains(r, "| Line | Col | Byte offset | Match |")

    @covers("CLI-DESIGN-5.6-search-nomatch", "CLI-DESIGN-6-rc1")
    def test_no_match_exits_one(self):
        arc = self.seed_search_archive()
        r = self.run_walrus("search", arc, "penguin")
        self.assert_code(r, 1)

    @covers("CLI-DESIGN-5.6-search-ignore-case")
    def test_case_insensitive_flag_matches_mixed_case(self):
        arc = self.seed_search_archive()
        # Case-sensitive: the needle 'CATS' is not present verbatim.
        strict = self.run_walrus("search", arc, "CATS")
        self.assert_code(strict, 1)
        # -i folds case and finds 'cats' / 'Cats'.
        caseless = self.run_walrus("search", arc, "CATS", "-i")
        self.assert_code(caseless, 0)

    @covers("CLI-DESIGN-5.6-search-plain")
    def test_plain_format_omits_markdown_table(self):
        arc = self.seed_search_archive()
        r = self.run_walrus("search", arc, "cats", "--format", "plain")
        self.assert_code(r, 0)
        self.assertNotIn(
            "| Line |", r.out_text,
            f"plain output must omit the Markdown table header\n{r!r}",
        )
        # Plain output lists the file that matched.
        self.assert_stdout_contains(r, "notes.txt")

    @covers("CLI-DESIGN-5.6-search-mask")
    def test_file_mask_restricts_to_matching_files(self):
        arc = self.seed_search_archive()
        # 'main' lives only in main.rs; masking to notes.txt finds nothing.
        miss = self.run_walrus("search", arc, "main", "notes.txt")
        self.assert_code(miss, 1)
        # Masking to the file that holds it finds the match.
        hit = self.run_walrus("search", arc, "main", "main.rs")
        self.assert_code(hit, 0)

    @covers("CLI-DESIGN-5.6-search-selector", "CLI-DESIGN-6-rc2")
    def test_two_selectors_is_usage_error(self):
        arc = self.seed_search_archive()
        r = self.run_walrus(
            "search", arc, "cats", "--index", "0", "--version", "original"
        )
        self.assert_code(r, 2)

    @covers("CLI-DESIGN-5.6-search-beyond-last", "CLI-DESIGN-6-rc4")
    def test_index_beyond_last_clamps_and_exits_rc4(self):
        arc = self.seed_search_archive()
        # Single-version files: index 9 is well beyond the last -> clamp + RC 4.
        r = self.run_walrus("search", arc, "cats", "--index", "9")
        self.assert_code(r, 4)

    @covers("CLI-DESIGN-5.6-search-regex")
    def test_regex_flag_matches_pattern(self):
        arc = self.seed_search_archive()
        # 'ca.s' matches 'cats'; --regex is available because the CLI binaries
        # build walrus-fmt with the search-regex feature.
        r = self.run_walrus("search", arc, "ca.s", "--regex")
        self.assert_code(r, 0)

    @covers("CLI-DESIGN-5.6-search-found")
    def test_search_matches_walx_behaviour_is_read_only(self):
        # Search must not mutate the archive: its bytes are identical before and
        # after a search (read-only guarantee).
        arc = self.seed_search_archive()
        before = arc.read_bytes()
        self.assert_code(self.run_walrus("search", arc, "cats"), 0)
        self.assertEqual(
            arc.read_bytes(), before, "search must not modify the archive"
        )


if __name__ == "__main__":
    unittest.main(verbosity=2)
