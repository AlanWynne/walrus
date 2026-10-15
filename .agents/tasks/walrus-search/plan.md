# Implementation Plan: Walrus search feature

Search a `.wal` / `.wbl` archive for a string and report where it was found
(file name/path, version index, byte offset, 1-based line, 1-based column), as
a Markdown report (default) or plain text, from both `walrus` and `walx`.

All paths below are rooted at `c:\workspace\Walrus\.worktrees\walrus-core`.
TDD per `.kiro/steering/testing.md`: write the failing test FIRST, watch it
fail, then implement. Keep every non-test `.rs` file under the 400-line cap and
strict ASCII (`.kiro/steering/rust-standards.md`, `documentation.md`).

---

## Current state found during exploration (the API search builds on)

The multi-file format re-work that `ROADMAP.md` lists as "not started" is in
fact substantially IMPLEMENTED in this worktree; the code is the authority, not
the roadmap prose. The relevant surface:

- `walrus_fmt::Archive` (crates/walrus/src/archive.rs) is the read model.
  - `Archive::open(path)` / `Archive::read(&[u8])` -> `Result<Archive>`.
  - `archive.files() -> impl Iterator<Item = &TrackedFile>` (manifest order).
  - `archive.file(sel) -> Option<&TrackedFile>` (matches `id` OR `name`).
  - `archive.header(sel) -> Option<&WchgHeader>` (carries `direction`).
  - `archive.records(sel) -> Option<&[ChangeRecord]>`.
  - `archive.reconstruct(&FileId, Version) -> Result<Vec<u8>>` -- the
    per-file, direction-aware engine. Index 0 = current, `count` = original,
    for BOTH directions (FORMAT.md section 5). This is the ONLY reconstruct
    surface search needs; it already exists and is tested. No limitation to
    work around for single-version search.
- `walrus_fmt::Version` (crates/walrus/src/version.rs): `Index(usize)`,
  `Current`, `Original`, `Relative(i64)`, `Label(String)`, `Timestamp(i64)`.
  Resolution clamps and raises `WalrusError::VersionRange` on a too-large
  numeric `Index`.
- `walrus_fmt::FileId` (crates/walrus/src/archive.rs): `FileId::from(&str)`,
  `.as_str()`. A tracked file's `id` is reached via `archive.file(name).id`.
- `TrackedFile` (crates/walrus/src/manifest.rs) exposes `id`, `name`,
  `versions.count`, `attributes.mtime`, `checkpoints`.
- `ChangeRecord` / `Op` (crates/walrus/src/record.rs): `op`, `ts:
  Option<String>`, `note`. `is_splice()` filter; `Op::Splice { at, delete_len,
  insert }`.
- Version-change TIMESTAMP for a given index: there is NO public helper that
  returns "the timestamp of version k". `cmd_query.rs::current_record_ts` shows
  the pattern for index 0 only (forward -> last splice `ts`; reverse -> first
  splice `ts`). The report header needs the timestamp of the SEARCHED index, so
  the matcher result type will carry an optional `version_ts: Option<String>`
  that the CLI fills from the record whose application produces that index
  (derivable from direction + index + records, same reasoning as
  `current_record_ts`). This keeps the library matcher free of timestamp policy
  and keeps the field load-bearing for the all-versions seam.

CLI layer:

- Shared code lives in `crates/wal-cli-common` (`publish = false`), already
  holding `mask.rs` (segment-aware `/` glob via `mask::matches`), `selector.rs`
  (the `Selector` enum + `to_version()` + `effective_selector` + `set_selector`
  one-selector rule + `parse_at_utc`), `names.rs` (`matching(&archive, mask)`),
  `extract.rs` (incl. `reconstruct_with_fallback` -- the RC-4 beyond-last
  numeric-index clamp), `cat.rs`, `error.rs` (`CliError`, `code: u8`, `usage()`
  = RC 2). Dependencies: `walrus-fmt`, `anyhow`, `serde_json` only.
- `walx` (crates/walx/src/main.rs): hand-rolled arg scan, dispatches
  `extract` / `cat`. `print_usage()` banner. Tests in crates/walx/tests/ with
  `common/mod.rs` (`Fixture::build()`, `run_walx(cwd, args)`, `code_of`).
- `walrus` (crates/walrus-cli): `dispatch.rs` has an `Op` enum + `scan_operation`
  + `canonical_op`/`flag_op` + `print_usage`; each `cmd_*.rs` parses then calls a
  shared engine (e.g. `cmd_cat.rs` -> `wal_cli_common::cat::run`). `common.rs`
  has `split_option` / `take_value`.

RC table (CLI-DESIGN section 6): 0 success, 2 usage/safety, 4 extract index
beyond last (original + warning), 8 target not in archive. There is currently NO
"no matches" code; see the search RC decision below.

---

## Design decisions (made here; no design doc provided)

1. Matcher lives in `walrus-fmt` as pure logic, operating on reconstructed
   bytes. It takes bytes + a known version index; it does NOT open archives or
   touch the CLI. This honours dependency-boundary invariant 2 and keeps the
   matcher reusable by the future all-versions loop and by Walker.

2. REGEX DECISION: ship literal + case-insensitive in the DEFAULT build; put
   regex behind a new `search-regex` cargo feature on `walrus-fmt` that pulls
   the `regex` crate. Rationale from what was read: `regex` is pure Rust with no
   app/GUI/`ff-*` dependency, so it does not breach invariant 2 (same posture as
   the already-optional `similar`/`encoding_rs`/`export-*` deps). But a
   playback-only embedder must stay lean and `cargo tree` must stay auditable,
   so regex is OPT-IN, exactly mirroring the existing `diff` / `i18n` feature
   pattern in crates/walrus/Cargo.toml. The default `walrus-fmt` build links
   neither `regex` nor the search-regex code. The CLI binaries enable
   `walrus-fmt/search-regex` so `--regex` works on the command line; without the
   feature a `--regex` request is a typed, documented refusal.

3. MATCH MODE type is a plain enum (`Literal`, caseless flag, optional
   `Regex`), compiled once per `search_version` call. The regex arm is behind
   `#[cfg(feature = "search-regex")]`; the enum variant for regex is also
   feature-gated so the default build cannot name it.

4. LINE/COLUMN RULE (documented in a doc comment on the result type and the
   deriver): byte offset is the authoritative locator. Line is 1-based = 1 + the
   count of LF (`0x0A`) bytes strictly before the match's start offset. Column
   is 1-based and measured in UNICODE SCALAR VALUES (chars), not bytes: it is
   1 + the number of `char`s in the slice from the byte after the last LF
   at-or-before the offset up to the match start. CR (`0x0D`) is NOT a line
   terminator and counts as an ordinary character on its line (consistent with
   FORMAT.md 4.1 which makes LF the sole `.wchg` terminator; a CRLF document
   shows the CR as the last column char of the line). A match offset that is not
   on a UTF-8 char boundary (only reachable for a regex/needle over invalid
   UTF-8 bytes) counts bytes for the column and the doc comment states this
   fallback. The deriver is a single pure function tested across multi-line,
   CRLF, and multi-byte UTF-8 inputs.

5. VERSION-AWARE RESULT: index is a FIELD on each `Match`, not a report-header
   concept, so an all-versions result set is just a longer `Vec<Match>` with
   mixed `version_index` values. `search_version(bytes, version_index, needle,
   opts) -> Vec<Match>` is the single-version primitive; a thin
   `search_versions(...)` range helper is added as a documented stub that loops
   the primitive (NOT wired to the CLI yet) so the all-versions path composes
   without a redesign.

6. EMPTY NEEDLE: defined as "no matches" (returns an empty `Vec`), for both
   literal and case-insensitive, and documented. (An empty regex is rejected at
   compile with a typed error.) This avoids an infinite stream of zero-width
   hits and matches `grep ''`-style surprise avoidance; the choice is stated in
   the doc comment and asserted by a test.

7. CLI RETURN CODES for search (documented in CLI-DESIGN and ROADMAP):
   - matches found anywhere -> RC 0 (grep convention).
   - NO matches found (valid search, zero hits) -> RC 1. This is the grep
     "no match" convention. RC 1 is otherwise the anyhow general-failure code,
     so search's `run` returns 1 deliberately via `Ok(1)` (never an `Err`), and
     the new row is documented in the section 6 table as search-specific so the
     contract stays explicit. (RC 8 is reserved for "target not in archive" on
     mutate ops and is NOT reused: a search mask matching no FILES is still a
     successful search with zero matches -> RC 1, matching `grep` over a glob
     that matches no files semantics closely enough and keeping one "nothing
     found" code for search.)
   - usage error (two selectors, unknown option, `--regex` without the feature,
     bad `--at`) -> RC 2.
   - beyond-last NUMERIC `--index` -> clamp to original, warn on stderr, search
     that version, and the command exits RC 4 (reusing
     `reconstruct_with_fallback`'s contract). If that clamped search finds
     matches the code is still 4 (the clamp warning dominates the 0); if it
     finds none, 4 also dominates 1. Document this precedence.

8. REPORT RENDERER data model has an all-versions seam: the renderer consumes a
   `SearchReport { archive_display, groups: Vec<FileGroup> }` where each
   `FileGroup { file_name, matches: Vec<&Match> }` and each `Match` carries its
   own `version_index` + `version_ts`. The single-version renderer groups by
   file; a `// SEAM:` comment marks where an all-versions renderer will instead
   group/collapse by version-index RANGE (e.g. "versions 55-36") or expand
   per-version with each version's timestamp. The data model does NOT hard-code
   one version: it never stores "the index" as a single report-level scalar.

---

## Ordered implementation items

- [ ] 1. Add the `search-regex` cargo feature to `walrus-fmt` (no code yet).
      Add `regex = { version = "1", optional = true, default-features = false,
      features = ["std", "unicode"] }` to `[dependencies]` and
      `search-regex = ["dep:regex"]` to `[features]` in
      crates/walrus/Cargo.toml, with a comment mirroring the `diff`/`i18n`
      rationale (opt-in, keeps a playback-only embedder lean, invariant 2).
      Files: crates/walrus/Cargo.toml
      Verify: `cargo build -p walrus-fmt` and
      `cargo build -p walrus-fmt --features search-regex` both compile. Then
      `cargo tree -p walrus-fmt` shows NO `regex` in the default build (feature
      off), and `cargo tree -p walrus-fmt --features search-regex` shows it.

- [ ] 2. (TDD red) Create the matcher module skeleton and its failing tests.
      Add `mod search;` to crates/walrus/src/lib.rs and
      `pub use search::{Match, SearchOptions, MatchMode, search_version};`
      (plus `search_versions` once item 6 lands). Create
      crates/walrus/src/search.rs with the public types declared but
      `search_version` returning `Vec::new()` (or `todo!()`), and a
      `#[cfg(test)] mod tests` holding the scenario-named failing tests listed
      in item 9 that cover literal find/not-found, multiple matches, and
      index-carrying.
      Files: crates/walrus/src/lib.rs, crates/walrus/src/search.rs
      Verify: `cargo test -p walrus-fmt search_` compiles and the new tests
      FAIL (red) -- confirm the failure before implementing.

- [ ] 3. Define the `Match` result type and the line/column deriver, make the
      line/col + literal tests pass. `Match { file_name: String, version_index:
      usize, version_ts: Option<String>, byte_offset: usize, line: usize, col:
      usize, matched_text: String }`, derive `Debug, Clone, PartialEq, Eq`. Add
      a private `fn line_col(bytes: &[u8], offset: usize) -> (usize, usize)`
      implementing decision 4 with the full doc comment (LF rule, CR handling,
      char-based column, non-boundary byte fallback). Note `version_ts` and
      `file_name` are set by the caller/primitive signature; `line_col` is pure.
      Files: crates/walrus/src/search.rs
      Verify: `cargo test -p walrus-fmt search_` -- the line/column tests from
      item 9 pass (green).

- [ ] 4. Implement `search_version` for literal + case-insensitive and make
      those tests pass. Signature: `pub fn search_version(file_name: &str,
      version_index: usize, version_ts: Option<&str>, bytes: &[u8], needle:
      &str, opts: &SearchOptions) -> Result<Vec<Match>>`. `SearchOptions {
      mode: MatchMode, case_insensitive: bool }` with `MatchMode::Literal` the
      default; literal search over bytes (case-insensitive compares an
      ASCII/Unicode-lowercased copy -- document the folding rule chosen and keep
      it deterministic). Empty needle returns `Ok(vec![])` (decision 6). Each
      hit builds a `Match` via `line_col`. Add a `// doc:` note stating the
      single-version primitive is the composition point for all-versions search.
      Files: crates/walrus/src/search.rs
      Verify: `cargo test -p walrus-fmt search_` -- literal, multi-match,
      case-insensitive, empty-needle, and index-carrying tests all pass.

- [ ] 5. Add the regex arm behind `#[cfg(feature = "search-regex")]` and its
      feature-gated tests. `MatchMode::Regex` variant and the compile+iterate
      path gated on the feature; an empty-pattern or invalid-pattern compile
      returns a typed error (add a `WalrusError` variant, e.g.
      `SearchPattern(String)`, in crates/walrus/src/error.rs with a
      `#[non_exhaustive]`-safe addition and a `///` doc). Tests gated with
      `#[cfg(feature = "search-regex")]`.
      Files: crates/walrus/src/search.rs, crates/walrus/src/error.rs
      Verify: `cargo test -p walrus-fmt --features search-regex search_` passes
      the regex tests; `cargo test -p walrus-fmt search_` (no feature) still
      passes and does not reference the regex variant.

- [ ] 6. Add the `search_versions` range-loop helper as a documented, tested
      stub. `pub fn search_versions(...) -> Result<Vec<Match>>` that loops an
      index range calling `search_version` per index and concatenates, with a
      `///` comment stating it is the all-versions composition point and is not
      yet CLI-wired. One unit test proving a 2-index range yields matches tagged
      with the correct differing `version_index` values (reconstruct the two
      versions in the test via hand-built bytes, not via `Archive`, to keep the
      matcher independent of the reconstruct engine).
      Files: crates/walrus/src/search.rs
      Verify: `cargo test -p walrus-fmt search_` passes the range test; confirm
      crates/walrus/src/search.rs non-test lines are under 400 (split into
      `search/mod.rs` + `search/line_col.rs` if approaching the cap).

- [ ] 7. (TDD red) Add the shared CLI search engine skeleton + failing tests.
      Create crates/wal-cli-common/src/search.rs with `SearchArgs { archive:
      PathBuf, needle: String, mask: Option<String>, selector: Selector,
      case_insensitive: bool, regex: bool, format: OutputFormat }`,
      `enum OutputFormat { Markdown, Plain }` (default Markdown), and `pub fn
      run(args: &SearchArgs) -> anyhow::Result<u8>` returning `Ok(0)` as a stub.
      Register `pub mod search;` in crates/wal-cli-common/src/lib.rs. Add the
      report-rendering data model (`SearchReport`/`FileGroup`) with the
      `// SEAM:` all-versions comment (decision 8). Add a `#[cfg(test)] mod
      tests` with failing unit tests for the renderer (Markdown header carries
      archive + index + timestamp; per-file table columns Line/Col/Byte
      offset/Match; trailing match/file-count summary; plain format differs).
      Files: crates/wal-cli-common/src/search.rs,
      crates/wal-cli-common/src/lib.rs
      Verify: `cargo test -p wal-cli-common search` compiles and the renderer
      tests FAIL (red).

- [ ] 8. Implement the shared search engine and renderer; make item 7's tests
      pass. `run` opens the archive (`Archive::open`), expands `mask` via
      `names::matching` (omitted mask = all files, like extract), resolves the
      selector once, and for each matched file: looks up its `FileId`, calls
      `reconstruct_with_fallback` (reuse from extract.rs for the RC-4 beyond-last
      numeric-index behaviour), derives the searched index's `version_ts` from
      direction+index+records (mirror `cmd_query.rs::current_record_ts`, but for
      the resolved index not just 0), calls
      `walrus_fmt::search_version(...)` once per file, and collects a
      `SearchReport`. If `args.regex` is set, map to `MatchMode::Regex`
      (available because the CLI enables `walrus-fmt/search-regex`, item 11);
      guard with a typed RC-2 usage error only if the feature is somehow off.
      Render Markdown (default) or plain to stdout; diagnostics/warnings to
      stderr. Return RC per decision 7 (0 found / 1 none / 4 clamp dominates).
      Keep the file under 400 non-test lines (split renderer into
      `search_render.rs` if needed).
      Files: crates/wal-cli-common/src/search.rs (+ optional
      crates/wal-cli-common/src/search_render.rs)
      Verify: `cargo test -p wal-cli-common search` passes (green).

- [ ] 9. (TDD list) The library matcher unit tests (scenario-sentence names) in
      crates/walrus/src/search.rs `#[cfg(test)] mod tests`, written FIRST per
      items 2-6:
      - `search_finds_literal_substring_reports_line_and_byte_offset`
      - `search_literal_absent_needle_returns_no_matches`
      - `search_reports_every_occurrence_in_one_version`
      - `search_line_and_column_are_one_based_across_multiple_lines`
      - `search_column_counts_unicode_scalars_not_bytes`
      - `search_crlf_document_treats_cr_as_ordinary_character`
      - `search_case_insensitive_matches_mixed_case`
      - `search_empty_needle_returns_no_matches`
      - `match_carries_the_version_index_it_was_found_in`
      - `search_versions_range_tags_matches_with_their_own_index`
      - (feature-gated) `search_regex_matches_pattern_and_reports_offset`
      - (feature-gated) `search_regex_invalid_pattern_is_a_typed_error`
      Files: crates/walrus/src/search.rs
      Verify: `cargo test -p walrus-fmt search_` and
      `cargo test -p walrus-fmt --features search-regex search_` all pass.

- [ ] 10. Wire the `search` subcommand into `walx`. Add a `"search"` arm to the
      dispatch match in crates/walx/src/main.rs, a `parse_search` building
      `SearchArgs` (reuse the existing `scan`/`split_option`/`take_value`
      helpers; add `-i`/`--ignore-case`, `--regex`, `--format <markdown|plain>`;
      positionals are `<archive> <string> [file-mask]`; selector via the shared
      selector grammar), and a usage line in `print_usage()`. Enable the feature
      in crates/walx/Cargo.toml: `walrus-fmt = { workspace = true, features =
      ["search-regex"] }` (or add the feature to the existing dep line).
      Files: crates/walx/src/main.rs, crates/walx/Cargo.toml
      Verify: `cargo build -p walx`; then item 12's `walx` integration tests.

- [ ] 11. Wire the `search` subcommand into `walrus`. Add `Search` to the `Op`
      enum and `"search"` to `canonical_op` in
      crates/walrus-cli/src/dispatch.rs (no flag-alias needed), a `cmd_search.rs`
      that parses tokens into `SearchArgs` and calls
      `wal_cli_common::search::run` (mirror cmd_cat.rs), register `mod
      cmd_search;` in crates/walrus-cli/src/main.rs, and add the usage line to
      dispatch's `print_usage`. Enable `walrus-fmt/search-regex` in
      crates/walrus-cli/Cargo.toml.
      Files: crates/walrus-cli/src/dispatch.rs,
      crates/walrus-cli/src/cmd_search.rs, crates/walrus-cli/src/main.rs,
      crates/walrus-cli/Cargo.toml
      Verify: `cargo build -p walrus-cli`; then item 12's `walrus` tests.

- [ ] 12. Add CLI integration tests over fixture archives for BOTH binaries.
      In crates/walx/tests/search.rs (reuse `common::{Fixture, run_walx,
      code_of}`) and crates/walrus-cli/tests/search.rs (reuse that crate's test
      common helper), cover with scenario-sentence names:
      - `search_literal_found_prints_markdown_and_exits_zero`
      - `search_no_match_exits_one`
      - `search_case_insensitive_flag_matches_mixed_case`
      - `search_plain_format_omits_markdown_table_syntax`
      - `search_file_mask_restricts_to_matching_files`
      - `search_two_selectors_is_usage_error_rc2`
      - `search_index_beyond_last_warns_and_exits_rc4`
      - `search_regex_flag_matches_pattern` (feature is on for the binaries)
      Assert on return codes and on stdout containing the expected header fields
      (archive name, index, timestamp) and table columns, not on exact
      whitespace.
      Files: crates/walx/tests/search.rs, crates/walrus-cli/tests/search.rs
      Verify: `cargo test -p walx search` and `cargo test -p walrus-cli search`
      pass.

- [ ] 13. Update CLI-DESIGN.md: add a `search` subsection under section 5 (its
      own 5.x), documenting the shape `walrus search <archive> <string>
      [file-mask] [selector] [-i|--ignore-case] [--regex] [--format
      markdown|plain]` and the identical `walx search`, the reused version
      selectors, the Markdown report shape (header with archive name/path +
      index + change timestamp; per-file table Line/Col/Byte offset/Match;
      match/file-count summary), the plain format, and the return codes (0
      found, 1 no matches, 2 usage, 4 beyond-last clamp). Add the search RC rows
      to the section 6 table (RC 1 = no matches found, search-only). Note
      all-versions search (index RANGE, per-version timestamps) as a designed-for
      but DEFERRED follow-up, pointing at the `search_versions` seam. ASCII only
      (`->`/`<=` etc.).
      Files: CLI-DESIGN.md
      Verify: `cargo fmt -- --check` is unaffected; manual read-through that the
      new 5.x and section 6 rows are present and ASCII-clean
      (`rg "[^\x00-\x7F\u2500-\u257F]" CLI-DESIGN.md` yields nothing new).

- [ ] 14. Update ROADMAP.md: record the search feature's shipped state --
      single-version search DONE in `walrus-fmt` (matcher) + both CLIs
      (subcommand), regex behind the `search-regex` feature; all-versions search
      DESIGNED-FOR and DEFERRED (the `search_versions` range primitive exists but
      is not CLI-wired, and the renderer has the version-range seam). ASCII only.
      Files: ROADMAP.md
      Verify: manual read-through; `rg "[^\x00-\x7F\u2500-\u257F]" ROADMAP.md`
      yields nothing new.

- [ ] 15. Final scoped verification across the touched crates.
      Verify (scoped, the preferred inner loop per shell-and-gate steering):
      - `cargo test -p walrus-fmt` and
        `cargo test -p walrus-fmt --features search-regex`
      - `cargo test -p wal-cli-common`
      - `cargo test -p walx` and `cargo test -p walrus-cli`
      - `cargo clippy -p walrus-fmt -- -D warnings`,
        `cargo clippy -p wal-cli-common -- -D warnings`,
        `cargo clippy -p walx -- -D warnings`,
        `cargo clippy -p walrus-cli -- -D warnings`
      - `cargo fmt -- --check`
      - `cargo tree -p walrus-fmt` shows NO `regex` by default (invariant 2
        boundary intact).
      Then hand off to the owner for the full `cargo gate --build` per the
      shell-and-gate two-stage completion rule.

---

## Notes / assumptions

- The matcher takes already-reconstructed bytes and a known index, so it is
  independent of the reconstruct engine's current shape; no limitation needs a
  workaround for single-version search. If a later refactor changes the
  `Archive::reconstruct` signature, only the CLI engine (item 8) is affected,
  not the matcher.
- `--label` stays deferred (as in extract/cat); search accepts the same selector
  set minus `--label`.
- `version_ts` is derived in the CLI layer (not the library) to keep timestamp
  policy out of the pure matcher; the field exists on `Match` so the data model
  is version-aware end-to-end for the all-versions follow-up.
- Keep each new/edited `.rs` file under 400 non-test lines; `search.rs` in both
  the library and wal-cli-common are the likeliest to need a by-concern split
  (matcher vs line/col; engine vs renderer).
