# Design Review: ustar short-id fix, query rework, source-mtime stamping

Scope reviewed: `design.md` (all three changes plus both review-response passes)
and the applied `FORMAT.md` edits (sections 2.1 and 3). The WHAT is fixed by the
user; this review judges only the HOW -- ambiguity, gaps, unverified
assumptions, and spec correctness. All load-bearing API claims were checked
against the live worktree source.

Verdict basis: findings below. HIGH + MEDIUM count determines the gate verdict.

---

## Findings

### MEDIUM-1 -- FORMAT.md section 3 manifest example still shows an old-style id, contradicting the new f<N> narrative

Where: `FORMAT.md` section 3, the `manifest.json` example block.

The whole point of Change 1's spec edit is that the id is a SHORT, sequential
`f<N>` token decoupled from the name. Section 2.1 and the section-3 field notes
now say exactly that ("The reference writer mints sequential ids `f0`, `f1`,
`f2`, ... in `files`-array order"). But the JSON example immediately above those
field notes still reads:

```json
"id": "a1b2c3",
```

`a1b2c3` is neither an `f<N>` token nor obviously decoupled from a name -- it
reads like a hash, which the design explicitly rejected in favour of `f<N>`
"for readability and determinism". A reader hitting the example first will
reasonably infer a hash scheme, directly contradicting the normative text and
the reference writer this task ships. FORMAT.md is the language-agnostic source
of truth (INV-1); an example that contradicts the normative prose is a spec
defect, not a cosmetic one.

Concrete fix: change the example id to the reference scheme and make the member
paths in the surrounding prose consistent, e.g.

```json
"id": "f0",
```

and verify no other FORMAT.md example (section 2.1 member-path table uses the
placeholder `<id>`, which is fine) hard-codes a non-`f<N>` concrete id. The
design's "Files touched" section lists the section 2.1 and section 3 edits but
does not call out fixing this example; add it to the Change-1 FORMAT.md edit
list.

### MEDIUM-2 -- The exact call site and ordering for `set_attributes_mtime` on create is unspecified

Where: `design.md` Change 3, "Why count-0 files can carry an mtime" and the
`run` wiring in `cmd_add.rs`.

The design correctly establishes that `set_attributes_mtime` is MANDATORY (no
public mutable manifest accessor on `Archive`) and specifies the setter's
signature, module, error, and body. What it does NOT pin is WHERE and WHEN the
CLI calls it, and that gap hides a real ordering question:

- `cmd_add.rs::run` loops over sources calling `add_at(...)`. On a brand-new
  file, `add_at` delegates to `add`, which records nothing (count 0). The design
  says the CLI "calls it right after a create with `clock::file_mtime_utc(path)`
  (file source) or `clock::now_utc()` (stdin source)" -- but the loop as written
  does not currently distinguish a create from a re-add, and it carries the
  per-source `ts` string, not the original `path`. The `Source`/`Collected`
  structs the design extends carry `ts` (a pre-formatted string), NOT the path,
  so by the time the loop runs the path needed for `file_mtime_utc` is gone.
- Resolution is simple but must be stated: call `set_attributes_mtime(&source.
  name, &source.ts)` using the SAME pre-formatted `source.ts` the design already
  carries (it is exactly the source-file mtime for a file source, or the
  wall-clock fallback for stdin), so no path is needed at call time. Gate the
  call on "this source was a create" using the `was_tracked` flag the loop
  ALREADY computes (`let was_tracked = archive.file(&source.name).is_some();`
  before the `add_at`). Set the attribute only when `!was_tracked` (a genuine
  create); on a re-add the record `ts` is the authoritative LastWriteTime and the
  create-time attribute must not be clobbered.

Concrete fix: in the `run` loop, after the `add_at(...)?` call, add:

```rust
if !was_tracked {
    archive
        .set_attributes_mtime(&source.name, &source.ts)
        .with_context(|| format!("stamping mtime for '{}'", source.name))?;
}
```

and state in the design that the create-time attribute reuses `source.ts`
(not a fresh path read) and is written only on the create branch. Do the same in
`cmd_update.rs` ONLY if update can create (it cannot -- `update` errors
`NotTracked` on an absent name), so for update there is no create branch and no
`set_attributes_mtime` call is needed; say so explicitly to avoid an implementer
adding a redundant call.

### NIT-1 -- `current_record_ts` and `last_write_time` fallback order can mask a corrupt/missing record ts behind a stale attribute, but the design's choice is defensible

Where: `design.md` Change 2, `last_write_time` resolution order.

The resolution order is: current version's record `ts` -> `attributes.mtime` ->
`-`. For a multi-version file the record `ts` always wins, which is correct
(HIGH-1). The only edge the design does not spell out: a multi-version file
whose current record has `ts == None` (possible -- `ChangeRecord.ts` is
`Option`, and `update_with_splices` records a splice with NO ts) falls back to
`attributes.mtime`, which is the CREATE-time mtime -- i.e. an OLDER time than the
current version. That is a slightly misleading LastWriteTime, but it is strictly
better than `-` and the fallback chain is the documented best-effort behaviour.
No change required; this is noted so the implementer does not mistake it for a
bug. If the owner wants the column to read `-` rather than a stale create-time
value for a stamp-less current record, that is a one-line change (drop the
`attributes.mtime` fallback when the file has >= 1 record), but the current
choice is acceptable.

---

## Verified Assumptions

Each checked against the live worktree source; all are correct as the design
states them.

1. `mint_id` currently slugifies the full name (`slugify(name)`) and is the
   root cause of the ustar overflow. CONFIRMED: `mutate_add.rs::mint_id(&self,
   name: &str)` calls `slugify(name)`; the single caller is `let id =
   self.mint_id(name);` in `Archive::add`. The design's rewrite (drop the `name`
   param, mint smallest-free `f<N>`) and the caller update are accurate.
2. `wchg_member_name` exists in `archive.rs` and currently does separator->`_`
   with no length bound. CONFIRMED. The design's extension (bound the tail to
   `WCHG_NAME_MAX_BYTES`) is additive to the real function.
3. `write.rs` builds the member path from the already-rendered `wchg_name`, so
   bounding in `wchg_member_name`/`to_bytes` is sufficient. CONFIRMED via
   `archive.rs::to_bytes` building `FileParts.wchg_name` from `wchg_member_name`.
4. The library stamp seam `add_at`/`update_at` takes `ts: &str`, validates its
   shape via `timestamp::parse_ts_epoch`, and threads it through `store_save`.
   CONFIRMED in `mutate_stamp.rs`. Reusing it for Change 3 needs no signature
   change.
5. `store_save` APPENDS a forward record (`file.records.push(net)`) and INSERTS a
   reverse record at index 0 (`file.records.insert(0, rec)`). CONFIRMED in
   `mutate_add.rs::store_save`. This is the basis of HIGH-1 (resolved in the
   design): forward current = last splice, reverse current = first splice. The
   design's `current_record_ts` direction selection (`iter().rev().find` for
   Forward, `iter().find` for Reverse) is correct against this.
6. Public API used by the query helper all exists and is public: `Archive::
   header(sel) -> Option<&WchgHeader>`, `Archive::records(sel) ->
   Option<&[ChangeRecord]>`, `Archive::reconstruct(&FileId, Version)`,
   `Archive::file(sel) -> Option<&TrackedFile>`, `Archive::files()`,
   `ChangeRecord::is_splice()`, `ChangeRecord.ts`, `WchgHeader.direction`
   (public field), and the re-exports `Direction`, `FileId`, `Version`,
   `WchgHeader`, `FileAttributes`, `TrackedFile`, `ChangeRecord`, `Op` from
   `walrus_fmt` (lib.rs `pub use`). CONFIRMED.
7. `clock.rs` owns `now_utc()` and a private `format_epoch_utc(i64)`. CONFIRMED.
   The design's `file_mtime_utc` lives in the SAME module, so it can call the
   private `format_epoch_utc` directly -- the plan is sound.
8. `Archive::manifest_mut`/`files_mut`/`directories_mut` are all `pub(crate)`;
   the single-document `WalrusFile::manifest_mut` is the only PUBLIC
   `manifest_mut`. CONFIRMED. The design's conclusion that a new additive
   `Archive` seam is REQUIRED (not optional) for create-time `attributes.mtime`
   is correct.
9. `id_for_selector` is `pub(crate)` in `mutate_add.rs` and reusable from the
   sibling `mutate_stamp` module; sibling name-based seams error
   `NotTracked { name }`. CONFIRMED. The design's `set_attributes_mtime` reuse of
   `id_for_selector` and its `NotTracked { name }` error match the siblings.
10. `FileAttributes.mtime` is `Option<String>` and `TrackedFile.attributes` is a
    public field. CONFIRMED in `manifest.rs`. Populating it is a CLI-side set of
    an existing field; no FORMAT.md change needed for Change 3.
11. `names::log_bytes` has exactly one production caller (`cmd_query.rs::
    long_line`) plus its own `#[cfg(test)]` test and a `tests/query.rs` use.
    CONFIRMED. Deleting it with the long-line rewrite is safe.
12. The four `tests/query.rs` tests exist exactly as the design's MEDIUM-1
    enumerates them, and their assertions match: `query_subcommand_lists_
    tracked_files_with_index_size_rc0` asserts `line.contains('2')` /
    `nested.contains('0')` (BREAKS under ls-style terse, as the design says);
    `query_long_listing_shows_attributes_and_log_bytes` asserts `columns.len()
    >= 4`; the bare and mask tests assert name containment only (UNCHANGED).
    CONFIRMED.
13. Acceptance tests named in the impact list exist: `test_query.py::
    test_terse_listing_names_every_file_with_an_index_size`,
    `test_long_listing_adds_log_bytes_and_generation_columns`, and
    `test_condense.py::_index_size` (parses `l.startswith(name + "\t")` and
    `row.split("\t")[1]`). CONFIRMED. The re-point to `query -l` Versions is
    necessary and correctly scoped.
14. `cmd_add.rs` carries one batch `let ts = clock::now_utc();` and the loop
    calls `add_at(..., &ts)`; `Source { name, bytes }` and `recurse::Collected
    { name, bytes }` are the structs to extend; `stdin_src::resolve` is the stdin
    path. CONFIRMED. The design's per-source `ts` wiring is accurate.
15. Byte-identical reconstruction (FORMAT.md section 5) is over the splice
    `at`/`delete_len`/`insert` applied to the base; `ts` is never consulted in
    `Archive::reconstruct`. CONFIRMED by reading the reconstruct path in
    `archive.rs`. Source-mtime `ts` cannot perturb reconstructed content; INV-3
    is intact as the design claims.
16. FORMAT.md contains no prohibited non-ASCII characters (ran the
    documentation-rule check over the file: no matches outside the allowed
    box-drawing range). CONFIRMED. The section 2.1/3 edits keep ASCII `->`,
    `<=`, `--` conventions.

## Unverified / Wrong Assumptions

1. WRONG (minor, spec hygiene): the design's implicit assumption that the
   FORMAT.md edits for Change 1 are COMPLETE. The section-3 manifest JSON example
   id was not updated to the `f<N>` scheme (MEDIUM-1), so the applied spec edit
   is internally inconsistent. Not a logic error in the code plan, but a spec
   defect that must be fixed since FORMAT.md is the source of truth.
2. UNDER-SPECIFIED (not wrong): the create-time `set_attributes_mtime` call site
   and its create-only gating (MEDIUM-2). The setter itself is fully specified;
   the caller wiring is not, and the obvious implementation needs the
   `was_tracked`-gated call and reuse of `source.ts` rather than a second path
   read.
3. Not independently verifiable here (no behavioural claim to confirm): the
   acceptance-harness `os.utime` determinism claim for the new source-mtime
   tests. The approach (set a fixed mtime, assert the derived
   `YYYY-MM-DD HH:MM:SS`) is sound and deterministic in principle; the exact
   epoch->display math will be exercised by the test itself. No design change
   needed, flagged only as a thing the implementer verifies when the test is
   written (watch it fail first, per the TDD rule).

---

## Summary

The design is strong: the three changes are cleanly separated, the dependency
boundary (INV-2) and byte-identical guarantee (INV-3) are preserved and
explicitly argued, HIGH-1 (direction-correct current-record selection) was
caught and fixed with a verified-against-source rationale, the panic-safety of
`render_last_write` is total, and the acceptance-suite impact is concrete and
accurate down to the named functions. Two MEDIUM items remain: a stale FORMAT.md
example that contradicts the new id scheme (spec correctness), and an
under-specified create-time `set_attributes_mtime` call site that hides an
ordering/argument question. Both are small and local; resolving them makes the
plan ready to implement.

Verdict: CHANGES_REQUESTED (2 MEDIUM, 1 NIT).
