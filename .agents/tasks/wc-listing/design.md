# Design: ustar long-path fix, query rework, source-mtime stamping

## Overview

Three already-decided changes to the walrus-core stream, authored spec-first.
FORMAT.md (the language-agnostic source of truth) has ALREADY been edited in
this step for Change 1 (section 2.1 short-id scheme, section 2.1 `<name>.wchg`
ustar-legal note, section 3 `id` field note); Change 2 is CLI-only (no FORMAT.md
change); Change 3 reuses the EXISTING record `ts` field (FORMAT.md section 4.4)
and so needs no FORMAT.md change either -- it changes only the SOURCE of the
value the CLI feeds through the existing stamp seam.

The guiding constraint is the dependency boundary (INV-2): `walrus-fmt` stays
clock-free AND filesystem-free. Reading the clock already lives in the CLI
(`crates/walrus-cli/src/clock.rs`); reading a source file's mtime is the same
kind of CLI-side read, passed into the library as a pre-formatted string through
the existing `add_at`/`update_at` seam. No `std::fs` and no clock enters
`walrus-fmt`.

The three changes are independent and can be implemented and tested in any
order, but Change 1 (the id scheme) is the only one that touches the on-disk
layout, so it lands first behind its FORMAT.md edit.

---

## Change 1 -- ustar long-path crash fix (short `f<N>` ids)

### Root cause

`crates/walrus/src/mutate_add.rs::mint_id` currently slugifies the full
tracked-file name into the id via `slugify(name)`: for a deep recursed path like
`Walrus-TestData-Base/.../build/.../wgl_bindings.rs` the slug becomes
`walrus_testdata_base_..._wgl_bindings_rs`, growing without bound with path
depth. The id is then embedded in BOTH member paths
(`files/<id>/base`, `files/<id>/<name>.wchg`). `write::append_member` calls
`tar::Header::set_path(path)`, and the ustar `name` field is capped at 100
bytes; a long id overflows it, which is exactly the observed error:
`provided value is too long when setting path for files/walrus_testdata_base_..._rs/`.

### The fix (decided: Option B, sequential `f<N>`)

The id becomes a SHORT archive-unique token whose length does not grow with path
depth. The scheme is the sequential `f<N>` family -- `f0`, `f1`, `f2`, ...
assigned in manifest (`files`-array) order -- chosen over a hash for readability
and determinism (per the FORMAT.md section 2.1 wording this step added). The
manifest `TrackedFile.name` stays the authoritative human path; the id is
decoupled from it.

### `mint_id` rewrite (`crates/walrus/src/mutate_add.rs`)

Replace the slugify-based `mint_id(&self, name: &str) -> String` with a
name-independent minting of the smallest free `f<N>`:

```rust
/// Mint a SHORT, archive-unique, filesystem-safe id DECOUPLED from `name`
/// (FORMAT.md section 2.1): the smallest free sequential `f<N>` in manifest
/// order. Length does not grow with the file's name or path depth, so every
/// derived member path stays ustar-legal (<= 100 bytes).
fn mint_id(&self) -> String {
    let taken = |candidate: &str| self.manifest().files.iter().any(|f| f.id == candidate);
    let mut n = 0usize;
    loop {
        let candidate = format!("f{n}");
        if !taken(&candidate) {
            return candidate;
        }
        n += 1;
    }
}
```

- `mint_id` no longer takes `name`; the single caller in `Archive::add`
  (`let id = self.mint_id(name);`) becomes `let id = self.mint_id();`.
- Smallest-free (not `files.len()`) keeps ids archive-unique and deterministic
  even after `remove` leaves a gap: `remove` of `f1` then a new add re-fills
  `f1`. This matches the existing collision-handling intent and never collides
  with a surviving id. (Reusing a freed id is acceptable because an id is only
  required to be unique WITHIN the current archive at any instant; it is not a
  cross-time key. If a reviewer prefers monotonic ids that never reuse a freed
  slot, that is a one-line change to track a high-water mark in the manifest, but
  smallest-free is simpler and stays deterministic for the add/recurse paths the
  tests exercise.)
- `slugify` becomes dead code. Delete it (and its unit coverage) per the
  rust-standards "delete dead code" rule. The existing test
  `add_mints_deterministic_slug_id` in `mutate_add.rs` is rewritten (see Tests).

### `<name>.wchg` member path stays ustar-legal (`crates/walrus/src/archive.rs`, `write.rs`)

A short id fixes `files/<id>/base`, but `files/<id>/<name>.wchg` still embeds the
readable `<name>` rendering, which can itself exceed 100 bytes for a deep path.
Per the FORMAT.md section 2.1 note this step added, the writer MUST keep the
WHOLE member path within the ustar limit by shortening the advisory `<name>`
when needed. The rendering lives in `archive.rs::wchg_member_name`; extend it to
bound the result:

```rust
/// Max bytes for the whole `files/<id>/<name>.wchg` member path. ustar caps the
/// `name` field at 100 bytes; reserve headroom for `files/`, the id, the slash,
/// and the `.wchg` suffix so even the longest short id leaves the name a safe
/// budget. 100 - len("files/") - MAX_ID - len("/") - len(".wchg").
const WCHG_NAME_MAX_BYTES: usize = 72;

fn wchg_member_name(name: &str) -> String {
    // Separators to `_` as today, then bound the length: keep a trailing slice
    // (the basename end is the most recognisable) so the whole member path fits
    // ustar. The authoritative name is the manifest's; this is advisory only.
    let slug: String = name
        .chars()
        .map(|c| match c { '/' | '\\' => '_', other => other })
        .collect();
    bound_tail_bytes(&slug, WCHG_NAME_MAX_BYTES)
}
```

- `bound_tail_bytes` has the signature
  `fn bound_tail_bytes(s: &str, max: usize) -> String`. It returns the last
  <= `max` bytes of `s` on a UTF-8 char boundary: if `s.len() <= max` return it
  unchanged; otherwise take a candidate start of `s.len() - max` and walk the
  index FORWARD to the next char boundary (`str::is_char_boundary`) before
  slicing, so the result is valid UTF-8 and never exceeds `max` bytes. A trailing
  slice is chosen over a leading one because the file's basename and extension --
  the recognisable part when a `.wchg` is extracted standalone -- are at the end
  of the path.
- Named unit tests (walrus crate, `#[cfg(test)]` in `archive.rs`), per the
  write-the-failing-test-first rule:
  - `wchg_member_name_bounds_a_deep_path_to_ustar_legal_length`: a pathological
    deep name renders to a member path `files/f0/<name>.wchg` whose total byte
    length is <= 100.
  - `bound_tail_bytes_keeps_a_utf8_boundary`: a multi-byte-char string truncated
    at a boundary that would split a char returns valid UTF-8 and <= `max` bytes.
- `WCHG_NAME_MAX_BYTES = 72` is derived conservatively: `files/` (6) + a short id
  budget (reserve 16 bytes for the id) + `/` (1) + `.wchg` (5) = 28;
  `100 - 28 = 72`. The REAL id is `f<N>`, which is 2 bytes (`f0`..`f9`) and stays
  <= ~8 bytes even for tens of millions of files (`f99999999`), so the 16-byte
  reservation is pure safety margin, not a constraint the id scheme ever
  approaches. The margin exists only so a future id scheme change cannot silently
  push the member path over 100 bytes.
- This is the ONLY place the name length matters for the writer: `write.rs`
  builds `format!("files/{}/{}.wchg", file.id, file.wchg_name)` where
  `wchg_name` is already the bounded rendering, so no change is needed in
  `write.rs` beyond relying on the bounded helper. `append_member` keeps its
  fixed `set_mode(0o644)` / `set_mtime(0)` so output stays byte-identical for
  identical inputs (determinism, FORMAT.md section 5 / INV-3).

### Determinism / byte-identical output

- Ids are a pure function of add order (`f0`, `f1`, ... smallest-free), so the
  same files added in the same order always yield the same ids and therefore the
  same member paths.
- `append_member` already pins `mode = 0o644` and `mtime = 0`; nothing in this
  change touches those, so a write of identical manifest + bases + logs produces
  byte-identical container bytes. The source-mtime stamp from Change 3 lives in
  record `ts` / manifest metadata, NOT in reconstructed content, so it does not
  perturb the byte-identical reconstruction guarantee (see Change 3's boundary
  note).

### Edge cases (Change 1)

- Empty archive / zero files: `mint_id` returns `f0` for the first add.
- `remove` then re-add: smallest-free re-uses the freed id; no collision with a
  surviving id (the `taken` closure scans live manifest entries).
- A name that renders to an empty slug (all separators): `bound_tail_bytes` of an
  empty string is empty; the member path becomes `files/<id>/.wchg`, which is
  valid and still located by directory + extension. (This cannot normally happen
  because `add` rejects such names earlier, but the rendering is total.)
- Backwards read compatibility: a reader never infers name from id and matches
  `.wchg` by directory + extension (`find_wchg_member`), so archives written with
  the OLD slug ids still open unchanged. This is a WRITER change; no reader
  migration is needed.

---

## Change 2 -- query output rework (CLI-only, no FORMAT.md change)

All rendering changes live in `crates/walrus-cli/src/cmd_query.rs`. `walx` has
NO query subcommand (its `main.rs` dispatches only `extract` / `cat`), so there
is nothing to mirror there. The mask argument and `-l` flag semantics are
unchanged; only the rendered lines change.

### Terse (`walrus query <archive>`, no `-l`) -- ls-style

One tracked file's relative name per line, nothing else: no index, no columns,
no header. Order is deterministic: the already-sorted manifest order is NOT
guaranteed sorted, so sort the selected names lexicographically before printing
(ls lists sorted). Replace `terse_line`:

```rust
/// ls-style terse listing: just the name, one per line.
fn terse_line(name: &str) -> String {
    name.to_string()
}
```

and in `run`, after building `names`, sort them so output is deterministic
regardless of manifest insertion order or mask:

```rust
let mut names: Vec<String> = match &args.mask {
    Some(mask) => names::matching(&archive, mask),  // mask branch
    None => archive.files().map(|f| f.name.clone()).collect(),  // no-mask branch
};
names.sort();   // byte-wise lexicographic (Vec<String>::sort)
```

SORT CONTRACT (applies to BOTH terse and long): `names.sort()` is
`Vec<String>::sort`, which is byte-wise lexicographic (Rust's `Ord for String`
compares by UTF-8 bytes, not locale). It runs on the FINAL `names` vector AFTER
the match, so it covers the mask branch AND the no-mask branch identically.
Terse and long therefore emit the same deterministic byte-sorted order
regardless of manifest insertion order or whether a mask is present. The
acceptance tests' expected arrays (e.g.
`["src/data/nums.csv", "src/notes/todo.md", "src/readme.txt"]`, and the
mask-matching tests) are stated in byte order to match this.

### Long (`-l`) -- aligned human table

A fixed-column, right-aligned-numeric table with a header row and an ASCII
separator row of `-` runs. Columns, in order:

| Column        | Align | Source                                                    |
|---------------|-------|-----------------------------------------------------------|
| Versions      | right | recorded version count = `TrackedFile.versions.count`     |
| LastWriteTime | right | stored last-modified UTC, rendered `YYYY-MM-DD HH:MM:SS`  |
| Bytes         | right | size of the CURRENT reconstructed version (index 0)       |
| Name          | left  | `TrackedFile.name` (authoritative relative name)          |

Exact widths and layout (single space gutter between columns):

- `Versions` column width = `8`.
- `LastWriteTime` column width = `19` (exact width of `YYYY-MM-DD HH:MM:SS`).
- `Bytes` column width = `12`.
- `Name` column is left-aligned and NOT padded on the right (it is last).
- The header separator row is a run of `-` matching each column's width,
  single-space-gutter-joined, so it lines up with the header labels. The `Name`
  separator is FIXED at 4 dashes (`-".repeat(4)`, the header label `Name` is 4
  chars) -- NOT the longest name -- so the separator row is deterministic and
  independent of the data.

AUTHORITATIVE LAYOUT: the Rust format-specifier block below is the single source
of truth for header, separator, and row layout. There is deliberately NO
hand-drawn "rendered shape" example, because a hand-spaced example drifts from
the format strings and leaves an implementer with two disagreeing sources. To see
the exact output, apply the format strings; the acceptance test asserts by
splitting on runs of whitespace, not by matching a fixed picture.

Formatting with Rust format specifiers:

```rust
// Header + separator (printed once, before the rows).
fn long_header() -> String {
    format!(
        "{:>8} {:>19} {:>12} {}\n{} {} {} {}",
        "Versions", "LastWriteTime", "Bytes", "Name",
        "-".repeat(8), "-".repeat(19), "-".repeat(12), "-".repeat(4),
    )
}

fn long_line(archive: &Archive, name: &str) -> String {
    let versions = index_size(archive, name);           // usize
    let last = last_write_time(archive, name);          // String: render_last_write output ("YYYY-MM-DD HH:MM:SS" or "-")
    let bytes = current_bytes_len(archive, name);       // u64
    format!("{versions:>8} {last:>19} {bytes:>12} {name}")
}
```

where `last_write_time` picks the stamp SOURCE (the CURRENT version's record
`ts`, else `attributes.mtime`, else none) and passes it through the total
`render_last_write` helper defined below, so the returned string is always either
a 19-char timestamp or `"-"`:

```rust
/// The `ts` of the CURRENT version's splice record, selected by direction.
///
/// CRITICAL (HIGH-1): record order is OLDEST-APPLIED-FIRST along the log
/// (`version.rs::resolve_timestamp`: "oldest-applied first is record order"),
/// and index 0 is ALWAYS the current version. For a FORWARD (`.wal`) file,
/// `store_save` APPENDS the net splice (`file.records.push`), so the current
/// version's record is `records.last()`; `records.first()` is the OLDEST step.
/// For a REVERSE (`.wbl`) file, `store_save` inserts at index 0
/// (`file.records.insert(0, rec)`), so the current version's record is
/// `records.first()`. Picking `.first()` unconditionally would show a multi-
/// version `.wal` file's FIRST-edit time as LastWriteTime -- wrong, and
/// `--wal` is the default and the user's reported scenario. Filtering
/// `is_splice()` also skips a leading/trailing checkpoint record that a raw
/// `.first()`/`.last()` would wrongly pick up.
fn current_record_ts(archive: &Archive, name: &str) -> Option<String> {
    let direction = archive.header(name)?.direction;
    let records = archive.records(name)?;
    let rec = match direction {
        Direction::Forward => records.iter().rev().find(|r| r.is_splice()),
        Direction::Reverse => records.iter().find(|r| r.is_splice()),
    }?;
    rec.ts.clone()
}

fn last_write_time(archive: &Archive, name: &str) -> String {
    let stamp = current_record_ts(archive, name).or_else(|| {
        archive
            .file(name)
            .and_then(|f| f.attributes.mtime.clone())
    });
    match stamp.as_deref() {
        Some(s) => render_last_write(s),
        None => "-".to_string(),
    }
}
```

`Direction` is re-exported from `walrus_fmt` (`pub use wchg::{Direction, ...}`),
`Archive::header(name) -> Option<&WchgHeader>` exposes the per-file
`direction` field, `Archive::records(name) -> Option<&[ChangeRecord]>` returns
the log in record order, and `ChangeRecord::is_splice()` is public -- all
confirmed against the worktree, so the helper uses only existing public API.

- `Versions`: `index_size` already exists (`versions.count`).
- `Bytes` -- size of the CURRENT reconstructed version: reconstruct index 0 and
  take its length. The library exposes `Archive::reconstruct(&FileId, Version)`.
  The CLI resolves the file's id from its name via `archive.file(name)` and calls
  `archive.reconstruct(&FileId::from(id), Version::Index(0))`, taking `.len()`.
  On a reconstruct error (should not happen for a valid archive at index 0) the
  Bytes column renders `-` rather than aborting the whole listing -- a listing
  is read-only and best-effort per column. (`FileId`, `Version` are re-exported
  from `walrus_fmt`.)
- `LastWriteTime` -- see Change 3 for the SOURCE of the value; the rendering is
  defined here. The stored stamp is the record `ts` of the CURRENT version,
  selected by direction via `current_record_ts` (forward: last splice record;
  reverse: first splice record -- see HIGH-1 above), nominally in the fixed
  library shape `YYYY-MM-DDThh:mm:ssZ`. The display shape is `YYYY-MM-DD
  HH:MM:SS` (replace the `T` with a space, drop the trailing `Z`).

  CRITICAL: the render MUST be TOTAL and must never panic. `attributes.mtime` is
  a free `Option<String>` in the manifest (FORMAT.md section 3 does not fix its
  format), and an externally-written archive could carry a `ts`/`mtime` string
  that is NOT the 20-char shape; a naive `stamp[11..19]` slice would panic on a
  shorter value and abort the whole listing, violating the best-effort-per-column
  rule. Route BOTH the record-`ts` path and the `attributes.mtime` fallback
  through ONE total helper that validates the fixed shape before slicing:

  ```rust
  /// Render a stored UTC stamp to the display shape `YYYY-MM-DD HH:MM:SS`.
  /// TOTAL: returns `"-"` for any value that is not the fixed 20-char
  /// `YYYY-MM-DDThh:mm:ssZ` shape, so a malformed stamp never panics the listing.
  fn render_last_write(stamp: &str) -> String {
      let b = stamp.as_bytes();
      // NIT-2: explicit char-boundary checks on every slice edge so a 20-BYTE
      // string carrying a multi-byte char at an interior position (which could
      // still satisfy the three ASCII-byte checks below) can NEVER slice
      // mid-char. Cheap, and removes all doubt for externally-written archives.
      if b.len() == 20
          && b[10] == b'T'
          && b[19] == b'Z'
          && stamp.is_char_boundary(10)
          && stamp.is_char_boundary(11)
          && stamp.is_char_boundary(19)
      {
          format!("{} {}", &stamp[0..10], &stamp[11..19])
      } else {
          "-".to_string()
      }
  }
  ```

  Guard conditions: `len == 20`, byte 10 == `b'T'`, byte 19 == `b'Z'`, plus
  `is_char_boundary` at offsets 10, 11, 19 (the slice edges). The three byte
  checks are ASCII checks on a UTF-8 string; the boundary checks defend the one
  residual case NIT-2 flagged -- a 20-byte string with a multi-byte char in an
  interior position that happens to pass the byte checks -- so the
  `[0..10]`/`[11..19]` slices are always on valid char boundaries when the guard
  passes (positions 0..=19 are all ASCII in a conforming stamp anyway). A file with NO recorded version (count 0, no records)
  has no per-version stamp; fall back to `TrackedFile.attributes.mtime` through
  the SAME `render_last_write`, and if that is absent (or non-conforming) the
  helper yields `-`. The resulting string (a 19-char timestamp or `"-"`) is
  right-justified into the 19-width field by `long_line`'s `{last:>19}`. (See
  Change 3 for why count-0 files may still carry an `attributes.mtime`.)

### `run` wiring

```rust
let mut out = String::new();
if args.long {
    out.push_str(&long_header());
    out.push('\n');
}
for name in &names {
    out.push_str(if args.long { &long_line(&archive, name) } else { &terse_line(name) });
    out.push('\n');
}
print!("{out}");
```

NIT-1: the existing `run` calls `terse_line(&archive, name)` (two args). This
new wiring drops the now-unused `&Archive` argument so the call is
`terse_line(name)`, matching the one-arg `terse_line(name: &str)` signature
above -- the only change needed at the call site, no `&Archive` parameter on
`terse_line`.

- DECISION (fixed, so the Rust test asserts one concrete behaviour): an empty
  archive under `-l` prints the header + separator row and NO data rows. A terse
  empty archive (no `-l`) prints nothing. The header is therefore printed
  unconditionally whenever `args.long` is set, independent of `names.len()` --
  the `if args.long { push header }` above is NOT guarded by `!names.is_empty()`.
  This matches `test_empty_archive_lists_nothing_and_is_rc0` (terse) and gives
  the long-empty case a discoverable column header.

### Removed helpers

The old long form surfaced `log_bytes`, `generations`, `size`, `mode`,
`mtime` as TAB columns. The new table drops `log_bytes`, `generations`, `size`,
`mode` from the display. `wal_cli_common::names::log_bytes` has exactly ONE
caller: `cmd_query.rs::long_line` (verified against the worktree). Removing the
old long line therefore makes it dead; DELETE `names::log_bytes` and its unit
test with this change (rust-standards "delete dead code"). `names::matching`
stays (mask expansion is unchanged).

### Edge cases (Change 2)

- Deterministic order: sort selected names before printing (terse and long).
- A mask selecting nothing: zero rows (terse prints nothing; long prints just
  the header+separator), RC 0 -- unchanged from today's behaviour.
- Reconstruct failure on a column (Bytes) or a missing stamp (LastWriteTime)
  degrades to `-` in that one cell; the listing never panics or aborts.
- Numeric alignment: `Versions`/`Bytes` right-aligned so columns line up; a
  value wider than its column simply pushes that row's gutter (acceptable; the
  widths are generous -- 12 digits of Bytes covers ~1 TB).

---

## Change 3 -- store SOURCE-FILE mtime as the version timestamp

### Does the format already persist a per-version UTC stamp? YES.

FORMAT.md section 4.4 defines the change record's optional `ts` field
(`"ts": "2026-10-08T12:01:02Z"`), and the library already threads it through the
additive stamp seam `Archive::add_at` / `Archive::update_at`
(`crates/walrus/src/mutate_stamp.rs`), stamping each recorded splice (and the
inverted reverse record) with the caller-supplied `YYYY-MM-DDThh:mm:ssZ` string.
The CLI already calls `add_at(..., &ts)` with `ts = clock::now_utc()` in
`cmd_add.rs` and `cmd_update.rs`.

Therefore Change 3 needs NO FORMAT.md change and NO new field: the `ts` field is
the per-version stamp query `-l`'s LastWriteTime reads. ONLY the SOURCE of the
value changes -- from the wall clock to the source file's filesystem
modified-time -- and only for file-sourced adds/updates. The seam signature
(`add_at`/`update_at` taking `ts: &str`) is UNCHANGED; no additive library change
is required.

### Where the mtime read goes (CLI only)

The library stays filesystem-free; the CLI reads the source mtime. Two source
paths feed `add`/`update`:

1. A plain file argument or a `-R` recursed file: there IS a source file on disk.
2. A `-` stdin stream (`--name`): there is NO source file.

The CLI reads source bytes in two places:
- `cmd_add.rs::collect_sources` (plain file: `std::fs::read(path)`; recursed:
  `recurse::collect_dir` -> `std::fs::read(&disk_path)`).
- the update equivalent (`cmd_update.rs` / its recurse path).

Carry the source mtime alongside the bytes so the save loop can stamp with it.
Extend the `Source` struct (and `recurse::Collected`) with an optional
pre-formatted UTC stamp:

```rust
// cmd_add.rs
struct Source {
    name: String,
    bytes: Vec<u8>,
    ts: String,           // source-file mtime UTC, or the wall-clock fallback
}

// recurse.rs
pub struct Collected {
    pub name: String,
    pub bytes: Vec<u8>,
    pub ts: String,
}
```

Add a small CLI helper (in `clock.rs`, which already owns the epoch->UTC
formatting) that reads a file's mtime and formats it with the SAME
`format_epoch_utc` the clock already uses -- so the stamp shape is byte-identical
to AC8's `now_utc` output and the library's `validate_ts` accepts it:

```rust
// clock.rs -- reads the FILESYSTEM mtime; still CLI-only, library stays FS-free.
/// The source file's last-modified instant formatted as `YYYY-MM-DDThh:mm:ssZ`.
/// Falls back to `now_utc()` when the mtime is unavailable (platform/FS quirk).
pub fn file_mtime_utc(path: &std::path::Path) -> String {
    match std::fs::metadata(path).and_then(|m| m.modified()) {
        Ok(mtime) => match mtime.duration_since(std::time::UNIX_EPOCH) {
            Ok(d) => format_epoch_utc(d.as_secs() as i64),
            Err(_) => now_utc(),   // pre-epoch mtime: fall back
        },
        Err(_) => now_utc(),       // mtime unreadable: fall back
    }
}
```

Wiring:
- Plain file in `collect_sources`: `ts: clock::file_mtime_utc(path)` using the
  path it just `std::fs::read`-s.
- Recursed file in `recurse::collect_dir`: set `ts: clock_mtime(&disk_path)` --
  but `recurse` is in `walrus-cli`, so it can call `crate::clock::file_mtime_utc`
  directly (keep the mtime read next to the existing `std::fs::read(&disk_path)`).
- stdin (`stdin_src::resolve`): there is no file; set `ts: clock::now_utc()`
  (the current wall-clock fallback the CLI already supplies for AC8).
- The save loop passes `&source.ts` to `add_at` / `update_at` INSTEAD of a single
  batch `let ts = clock::now_utc();`. Remove the batch-level `ts` and stamp
  per-source so each file carries its OWN source mtime. (`cmd_add.rs` currently
  computes one `ts` for the whole batch; replace that with per-source `ts`.)

### Boundary note (critical)

- The library NEVER gains `std::fs` or a clock: it still only receives a
  validated `&str` through `add_at`/`update_at`. `std::fs::metadata().modified()`
  lives in the CLI's `clock.rs` only. INV-2 (the dependency boundary) is intact.
- Source mtime makes the stamp FILESYSTEM-DEPENDENT. That is acceptable for this
  metadata/UX field (LastWriteTime in a listing) but it MUST NOT feed
  byte-identical reconstruction. It does not: `ts` lives in the record metadata
  and the manifest, never in reconstructed content. FORMAT.md section 5's
  byte-identical guarantee is over the SPLICE `at`/`delete_len`/`insert` applied
  to the base; `ts` is not consulted during reconstruction. INV-3 stays intact.
- Determinism of CONTAINER BYTES: the container is not required to be
  byte-identical across different machines/clocks (it never was -- AC8 already
  wrote a wall-clock `ts`). The byte-identical guarantee is specifically about
  RECONSTRUCTED VERSION bytes for a given index, which do not include `ts`.

### Why count-0 files can carry an mtime for LastWriteTime

A fresh `add` of a file records NO splice (count 0), so there is no record `ts`
to show. To still surface a LastWriteTime for a single-version file, the CLI
SHOULD record the source mtime into `TrackedFile.attributes.mtime` at create
time (the manifest field already exists, FORMAT.md section 3). This is a CLI-side
population of an existing optional field, not a format change. On create,
`cmd_add.rs` would set the created file's `attributes.mtime` to the source mtime
string.

The library's `Archive::add` create path builds the `TrackedFile` with default
(empty) attributes today, and the multi-file `Archive` exposes NO public mutable
manifest accessor: `Archive::manifest_mut`/`files_mut`/`directories_mut` are all
`pub(crate)`, and the only public `manifest_mut` is on the single-document
`WalrusFile` shim, not on `Archive`. So the CLI CANNOT populate
`attributes.mtime` through any existing public seam. A new additive library
method is therefore REQUIRED (not optional); it is the single load-bearing
source of a count-0 file's LastWriteTime.

Chosen design: expose a tiny additive metadata setter on `Archive`, in
`crates/walrus/src/mutate_stamp.rs` (the module that already owns the
caller-supplied-string stamp seam, so the setter sits beside `add_at`/`update_at`
and can reuse `id_for_selector`):

```rust
/// Record a tracked file's source-file last-modified stamp into its manifest
/// `attributes.mtime` (FORMAT.md section 3). Metadata only: the library stays
/// clock-free and filesystem-free -- `mtime` is a pre-formatted UTC string the
/// CLL supplies, exactly like the `ts` seam. Sets ONLY `attributes.mtime`; no
/// other attribute, record, base, or version is touched, so it never perturbs
/// reconstruction (FORMAT.md section 5 / INV-3).
///
/// # Errors
/// Returns [`WalrusError::NotTracked`] when `name` matches no tracked file.
pub fn set_attributes_mtime(&mut self, name: &str, mtime: &str) -> Result<()> {
    let id = self
        .id_for_selector(name)
        .ok_or_else(|| WalrusError::NotTracked { name: name.to_string() })?;
    let tracked = self
        .manifest_mut()
        .files
        .iter_mut()
        .find(|f| f.id == id)
        .ok_or_else(|| WalrusError::NotTracked { name: name.to_string() })?;
    tracked.attributes.mtime = Some(mtime.to_string());
    Ok(())
}
```

- Error variant is `NotTracked { name }` (not `UnknownFile { id }`): the CLI
  calls this BY NAME right after a create, matching the sibling name-based mutate
  seams (`update_with_splices`, `set_generations`, `add_at`) which all resolve
  via `id_for_selector` and error `NotTracked { name }`. `id_for_selector` is the
  existing `pub(crate)` resolver in `mutate_add.rs`, reusable from the sibling
  `mutate_stamp` module.
- It mutates `attributes.mtime = Some(mtime.to_string())` on the selected
  `TrackedFile` ONLY. It does not merge or replace the whole `FileAttributes`
  struct, so `size`/`mode` (if later populated) are untouched. It is clock-free
  and FS-free: the string is produced in the CLI's `clock.rs`.
- The CLI calls it right after a create with `clock::file_mtime_utc(path)` (file
  source) or `clock::now_utc()` (stdin source).
- query `-l`'s `last_write_time` resolution order becomes: the CURRENT version's
  record `ts` (via `current_record_ts`, direction-selected -- see HIGH-1) -> else
  `attributes.mtime` (the create-time source mtime) -> else `-`.

#### Create-time call site and gating (MEDIUM-2, pinned)

In `cmd_add.rs`'s per-source run loop, each source already computes a
`was_tracked` boolean (true when the name already existed in the archive before
this add) and a pre-formatted `source.ts` UTC string (the value handed to
`add_at`). Wire the mtime setter as follows, immediately AFTER the existing
`archive.add_at(&source.name, ..., &source.ts)` call for that source:

```rust
// A fresh create (count 0) records no splice, so attributes.mtime is the only
// LastWriteTime a single-version file can surface. Reuse the SAME source.ts
// (no second mtime read) and set it ONLY on first create, never on re-add of
// an already-tracked name (an update path owns its own record ts).
if !was_tracked {
    archive.set_attributes_mtime(&source.name, &source.ts)?;
}
```

- Gate on `!was_tracked` exactly: re-adding an already-tracked name goes through
  the normal splice/`add_at` path whose record `ts` is the authoritative
  current-version stamp, so overwriting `attributes.mtime` there would be
  redundant and could mask the current record. First create is the only case
  that needs it.
- Reuse `source.ts` (already formatted for `add_at`); do NOT re-read the file
  mtime a second time -- one `clock::file_mtime_utc(path)` read per source, its
  result feeding both `add_at` and this setter.
- `cmd_update.rs` needs NO such call: `update` cannot create a tracked file (it
  errors `NotTracked` on an absent name), so every update already has a record
  `ts` that `current_record_ts` surfaces; `attributes.mtime` is a create-time
  artefact only.

### Edge cases (Change 3)

- Unreadable or pre-epoch mtime: `file_mtime_utc` falls back to `now_utc()`; the
  stamp is still a valid fixed-shape UTC string, so `validate_ts` accepts it and
  the save succeeds.
- stdin source: no file, wall-clock fallback (unchanged AC8 behaviour).
- A no-change save records nothing (the existing `store_save` returns early for
  `old == new`), so no stamp is applied -- the previous version's stamp stays the
  LastWriteTime. Correct: nothing was written.
- Reverse files: `store_save` already stamps the inverted reverse record(s) with
  the same `ts`, so a reverse file's current-version stamp is the source mtime
  too -- symmetric with forward (INV-3 symmetry preserved).

---

## Acceptance-suite impact

Python harness: `c:\workspace\Walrus\.worktrees\wc-listing\tests\acceptance\walrus_cli\`.
The old query SHAPE is asserted in several places and MUST change. Rust
integration tests (`crates/walrus-cli/tests/query.rs`,
`crates/walrus/src/mutate_add.rs` unit tests) also change.

### Must change (old shape asserted)

- `test_query.py`
  - `test_terse_listing_names_every_file_with_an_index_size`
    (`FR-W11-AC1`, `CLI-DESIGN-5.1-query-terse`): expected lines change from
    `name\tcount` to ls-style bare names, sorted. New expected:
    `["src/data/nums.csv", "src/notes/todo.md", "src/readme.txt"]`.
  - `test_index_size_counts_recorded_versions_after_an_update`
    (`FR-W11-AC1`, `-query-terse`): index size is NO LONGER in terse output.
    RE-SCOPE this to assert the count via `query -l`'s Versions column instead
    (or move the count assertion to the long-table test). Rename to reflect it
    now reads the Versions column under `-l`.
  - `test_long_listing_adds_log_bytes_and_generation_columns`
    (`FR-W11-AC1`, `-query-long`): the whole assertion changes. Rename to
    `test_long_listing_is_an_aligned_table_with_versions_lastwrite_bytes_name`.
    Assert: a header row containing `Versions`, `LastWriteTime`, `Bytes`, `Name`;
    an ASCII separator row of `-` runs; the readme row parses to Versions == 1,
    a `YYYY-MM-DD HH:MM:SS` LastWriteTime, a numeric Bytes, and Name
    `src/readme.txt`. Columns are whitespace-aligned (split on runs of spaces,
    not TAB).
  - `test_single_star_mask_matches_within_one_segment_only` and
    `test_double_star_mask_spans_slash_boundaries`
    (`CLI-DESIGN-5.1-query-mask`): these parse `line.split("\t")[0]`. Terse now
    has no TAB; change to take the whole line as the name (the terse line IS the
    name). Semantics (which names match) are unchanged.
  - `test_empty_archive_lists_nothing_and_is_rc0` (`FR-W11-AC1`): unchanged for
    terse (still empty stdout). (If the long header is printed for an empty
    archive, this test stays terse-only, so no change.)

- `test_add.py` -- several assertions parse terse `name\tcount`:
  - `test_single_file_arg_is_stored_under_its_basename`: `"one.txt\t0"` ->
    `"one.txt"`.
  - `test_multiple_file_args_in_one_add`: `line.split("\t")[0]` -> whole line.
  - `test_second_add_changed_content_adds_a_version`: `"f.txt\t1"` -> assert the
    count via `query -l` Versions column (change to read `-l`), since terse no
    longer carries the count.
  - `test_second_add_unchanged_is_noop_with_note`: `"f.txt\t0"` -> `"f.txt"`
    (count via `-l` if the count still matters here; the no-change note on stderr
    is the primary assertion, so terse name-only suffices).
  - `test_recurse_ingests_in_deterministic_sorted_order`: `line.split("\t")[0]`
    -> whole line; order assertion unchanged (now also guaranteed sorted by
    query itself).
  - `test_generations_caps_recorded_versions`: reads
    `r.out_text.strip().split("\t")[1]` for the count -> switch to `query -l` and
    parse the Versions column.

- `test_stdin.py`:
  - `test_..._p.txt\t1` and `exact/kept.txt\t0`: terse `name\tcount` ->
    name-only, count via `-l` where the count is the point.

- `test_update.py`:
  - `test_...` asserting `"src/readme.txt\t1"` / `"src/readme.txt\t0"`
    (`FR-W11-AC8`, others): terse name-only; count via `-l` Versions column.

- `test_condense.py`:
  - `_index_size` helper parses `row.split("\t")[1]` from terse output. Re-point
    it at `query -l` and parse the Versions column (split on whitespace runs).

- Rust `crates/walrus-cli/tests/query.rs` has FOUR concrete tests; each is named
  with its exact change (MEDIUM-1):
  - `query_subcommand_lists_tracked_files_with_index_size_rc0` -- BREAKS. It
    asserts the terse `notes.txt` line `contains('2')` and the `src/main.rs`
    line `contains('0')`. ls-style terse is the BARE NAME only, so `notes.txt`
    no longer carries any count digit and the `contains('2')` assertion fails.
    RE-POINT it at the `-l` Versions column: run `query -l`, find the
    `notes.txt` row, parse the first whitespace-split field (Versions) == 2 and
    the `src/main.rs` row's Versions == 0. Rename to
    `query_long_versions_column_counts_recorded_versions_rc0`.
  - `query_long_listing_shows_attributes_and_log_bytes` -- RENAME + REWRITE. It
    is named for `log_bytes` (now deleted) and only asserts `columns.len() >= 4`
    on a `split_whitespace` of the long line (which happens to still pass because
    the new `YYYY-MM-DD HH:MM:SS` value itself splits into two fields, so a data
    row yields 5 fields -- a weak, misleading assertion). Rewrite as
    `query_long_listing_is_an_aligned_versions_lastwrite_bytes_name_table`:
    assert the header line contains `Versions`, `LastWriteTime`, `Bytes`, `Name`;
    assert a separator row of `-` runs follows; find the `notes.txt` data row and
    assert its whitespace-split fields parse to Versions == 2, a `YYYY-MM-DD
    HH:MM:SS` LastWriteTime (date + time = two fields), a numeric Bytes, and
    Name `notes.txt`.
  - `bare_archive_defaults_to_query_listing_rc0` -- UNCHANGED (decision). It only
    checks `stdout.contains("notes.txt")` / `contains("src/main.rs")`; bare
    terse names still contain those substrings, so it passes as-is.
  - `query_mask_filters_to_matching_files` -- UNCHANGED (decision). It checks
    name containment only (`contains("notes.txt")`, `!contains("src/main.rs")`);
    the mask semantics and the names in the output are unchanged, so it passes
    as-is.

- Rust `crates/walrus/src/mutate_add.rs` unit tests:
  - `add_mints_deterministic_slug_id` -> rewrite as
    `add_mints_sequential_f_ids_decoupled_from_name`: assert the first add gets
    id `f0`, a second add gets `f1`, and the id is INDEPENDENT of the (long,
    deep) name. Assert a name that previously produced a long slug now produces a
    short `f<N>` id.
  - `@covers` / traceability: these are Rust tests (no `@covers`); no mapping
    change, but keep names descriptive (rust-standards test naming).

### New coverage needed

- Long-path round-trip (the ORIGINAL bug): a `tests/acceptance/walrus_cli`
  test (`test_add.py` or a new `test_longpath.py`) that builds a fixture tree
  with a DEEP, long relative path (deep enough that the OLD slug id would exceed
  100 bytes -- e.g. nested dirs totalling > 120 chars), `add -R`s it, asserts
  RC 0 (no ustar "value is too long" error), and round-trips via `extract` to
  byte-identical content. `@covers` a new id, e.g.
  `CLI-DESIGN-2.1-shortid-longpath` (add to `requirements.py`) plus the existing
  `FR-W11-AC7`/`FR-W11-AC5` round-trip ids. This is the regression guard for the
  reported crash.
- ls-style terse: covered by the rewritten
  `test_terse_listing_names_every_file...` (bare names, sorted). Keep
  `CLI-DESIGN-5.1-query-terse` as its `@covers` and update that id's description
  in `requirements.py` to "terse ls-style: one bare name per line, sorted".
- Aligned long table: the rewritten long-listing test above
  (`CLI-DESIGN-5.1-query-long`); update that id's description in
  `requirements.py` to "`-l` aligned table: Versions / LastWriteTime / Bytes /
  Name".
- Source-mtime surfacing in `query -l`: a new test (`test_query.py`) that writes
  a source file, sets a KNOWN mtime on it (via `os.utime` with a fixed epoch in
  the Python harness -- deterministic, no wall-clock dependence), `add`s it,
  then `query -l` and asserts the LastWriteTime column equals the expected
  `YYYY-MM-DD HH:MM:SS` derived from that fixed mtime (not "now"). `@covers`
  `FR-W11-AC8` plus a new `CLI-DESIGN-5.1-query-lastwrite` id (add to
  `requirements.py`: "query -l LastWriteTime is the source-file mtime"). This is
  the acceptance proof that the stamp SOURCE changed from clock to file mtime.
  - Also update/extend the existing AC8 stamping test (`test_update.py`
    `FR-W11-AC8` case): the stamp is now the source mtime, so if that test pins a
    "recent now" stamp it must instead pin the fixed source mtime. Re-point it at
    a `os.utime`-fixed source and assert the recorded stamp matches that fixed
    time (surfaced via `query -l`).
- MULTI-VERSION `.wal` LastWriteTime (the HIGH-1 regression guard): a new test
  (`test_query.py`, e.g.
  `test_long_lastwrite_tracks_the_current_version_on_a_multi_version_wal`) that
  `add`s a `.wal` file with a KNOWN-early `os.utime` mtime, then updates it with
  a KNOWN-later `os.utime` mtime (two versions, Versions == 2), then `query -l`
  and asserts the LastWriteTime column equals the LATER (current-version) time,
  NOT the first-edit time. This is the acceptance proof that `current_record_ts`
  selects by direction (forward: last splice record) rather than blindly
  `.first()`; a single-version fixture cannot catch the bug (count 0 falls back
  to `attributes.mtime`, count 1 has `first() == last()`). Mirror it in the Rust
  `tests/query.rs` with a two-version `.wal` fixture asserting the parsed
  LastWriteTime field is the second save's stamp. `@covers`
  `CLI-DESIGN-5.1-query-lastwrite` + `FR-W11-AC8`.

### requirements.py updates

- Update descriptions for `FR-W11-AC1`, `CLI-DESIGN-5.1-query-terse`,
  `CLI-DESIGN-5.1-query-long` to the new shapes (above).
- Add `CLI-DESIGN-2.1-shortid-longpath` (long-path round-trip),
  `CLI-DESIGN-5.1-query-lastwrite` (LastWriteTime = source mtime). Keep
  `FR-W11-AC8` (now "source-file mtime, wall-clock fallback for stdin").

### Harness helpers

- `make_multi_version` and `seed_archive` build archives the tests parse; no
  change to the helpers themselves, but the TESTS that parse their query output
  change as listed. If many tests need "the Versions count for a name under -l",
  add ONE helper to `harness.py` (e.g. `query_versions(self, arc, name) -> int`
  that runs `query -l`, finds the name's row, and parses the Versions column by
  splitting on whitespace runs) so the count-parsing lives in one place rather
  than being re-derived per test.

---

## Testability summary

- Change 1 is pure library logic: `mint_id` and `wchg_member_name` /
  `bound_tail_bytes` are unit-tested in `walrus` (deterministic ids; member path
  <= 100 bytes for a pathological name). The end-to-end long-path crash is an
  acceptance (black-box CLI) test. The byte-identical container property is
  assertable directly (write the same inputs twice, compare bytes) per the
  testing rule.
- Change 2 is CLI rendering: unit-testable in `crates/walrus-cli/tests/query.rs`
  (terse = byte-sorted bare names; long = header + separator + aligned rows),
  plus the acceptance tests. Column alignment is deterministic (fixed widths), so
  assertions are stable. `render_last_write` is a pure total function unit-tested
  directly: a conforming 20-char stamp renders `YYYY-MM-DD HH:MM:SS`; a short or
  malformed stamp returns `"-"` (the panic-safety guard from MEDIUM-2); and a
  NIT-2 boundary case -- a 20-BYTE string with a multi-byte char at an interior
  position (e.g. position 5) -- is fed in and asserted to return `"-"` without
  panicking (the `is_char_boundary` guard). `current_record_ts` is also directly
  testable: a two-version forward (`.wal`) fixture asserts it returns the LAST
  splice record's `ts` (current version), and a two-version reverse (`.wbl`)
  fixture asserts it returns the FIRST splice record's `ts` -- the HIGH-1 guard
  at the unit level.
- The additive `Archive::set_attributes_mtime` setter is unit-testable in the
  walrus crate: setting it on a tracked file populates `attributes.mtime` and
  touches no other attribute; calling it for an absent name returns
  `NotTracked { name }`.
- Change 3 is CLI-side mtime reading + library stamp reuse: the library stamp
  path is already unit-tested (`mutate_stamp.rs`); the new piece is
  `clock::file_mtime_utc`, unit-testable by pointing it at a tempfile with a
  known mtime (`std::fs::set_times` / `filetime`-free: set via the harness) and
  asserting the formatted string. The acceptance test uses `os.utime` for a
  deterministic, clock-free assertion.

All new/changed tests follow the TDD rule: write the failing test first
(ls-style terse, aligned long table, long-path round-trip, source-mtime
surfacing), watch it fail against the current shape, then implement.

---

## Files touched (summary)

Spec (already edited this step):
- `FORMAT.md` -- section 2.1 (short-id scheme; `<name>.wchg` ustar-legal note),
  section 3 (`id` field note). No Change-2 or Change-3 edit (CLI-only / existing
  `ts` field).

Change 1:
- `crates/walrus/src/mutate_add.rs` -- `mint_id` rewrite (name-independent
  `f<N>`), drop `slugify`, update the create-path caller, rewrite the slug test.
- `crates/walrus/src/archive.rs` -- `wchg_member_name` bounds the rendered name
  (`bound_tail_bytes`, `WCHG_NAME_MAX_BYTES`); unit tests for the <= 100-byte
  guarantee.

Change 2:
- `crates/walrus-cli/src/cmd_query.rs` -- terse ls-style, long aligned table,
  sort names, Bytes from reconstruct(index 0), LastWriteTime rendering.
- `crates/wal-cli-common/src/names.rs` -- remove `names::log_bytes` AND its
  `#[cfg(test)]` unit test (`cmd_query.rs::long_line` was its only PRODUCTION
  caller; the only other caller is `tests/query.rs`, handled below).
- `crates/walrus-cli/tests/query.rs` -- the four named tests from MEDIUM-1:
  rename `query_subcommand_lists_tracked_files_with_index_size_rc0` ->
  `query_long_versions_column_counts_recorded_versions_rc0` (re-point at `-l`
  Versions); rename + rewrite `query_long_listing_shows_attributes_and_log_bytes`
  -> `query_long_listing_is_an_aligned_versions_lastwrite_bytes_name_table`;
  `bare_archive_defaults_to_query_listing_rc0` and
  `query_mask_filters_to_matching_files` stay unchanged; add the multi-version
  `.wal` LastWriteTime test (HIGH-1 guard).

Change 3:
- `crates/walrus-cli/src/clock.rs` -- add `file_mtime_utc` (FS read, CLI-only).
- `crates/walrus-cli/src/cmd_add.rs` -- per-source `ts` from source mtime;
  populate `attributes.mtime` on create.
- `crates/walrus-cli/src/cmd_update.rs` -- per-source `ts` from source mtime.
- `crates/walrus-cli/src/recurse.rs` -- carry `ts` on `Collected`.
- `crates/walrus-cli/src/stdin_src.rs` (or its caller) -- wall-clock fallback ts.
- `crates/walrus/src/mutate_stamp.rs` -- additive
  `Archive::set_attributes_mtime(&mut self, name, mtime) -> Result<()>` metadata
  setter (clock/FS-free string setter; errors `NotTracked { name }`).

Acceptance / Rust tests:
- `tests/acceptance/walrus_cli/`: `test_query.py`, `test_add.py`, `test_stdin.py`,
  `test_update.py`, `test_condense.py`, `requirements.py`, `harness.py` (new
  `query_versions` helper), plus a long-path round-trip test and a source-mtime
  LastWriteTime test.

---

## Responses to design review -- SECOND pass (current design-review.json)

Revision pass against the latest `design-review.json` (verdict
CHANGES_REQUESTED: 1 HIGH + 1 MEDIUM + 2 NIT). All four findings are ADDRESSED;
none backlogged or ignored. Each was verified against live worktree source
before editing.

- HIGH-1 (`last_write_time` reads the WRONG record for a forward `.wal` file) --
  ADDRESSED. CONFIRMED against source: `version.rs` documents record order as
  "oldest-applied first is record order along the log", and `mutate_add.rs::
  store_save` APPENDS a forward save (`file.records.push`) but INSERTS a reverse
  save at index 0 (`file.records.insert(0, rec)`). So `records.first()` is the
  OLDEST step for a forward file; the current version (index 0) is
  `records.last()`. The design's old `.first()` + "newest record = current
  version" comment was wrong for the DEFAULT `--wal` case (the user's scenario),
  masked only at count 0 (attributes fallback) and count 1 (first == last).
  FIX: introduced `current_record_ts(archive, name)` that reads
  `archive.header(name).direction` and selects `records.iter().rev().find(is_splice)`
  for Forward and `records.iter().find(is_splice)` for Reverse (the
  `is_splice()` filter also skips a leading/trailing checkpoint record).
  `last_write_time` now calls it, falling back to `attributes.mtime` through the
  same `render_last_write`. Deleted the misleading comment; updated the
  LastWriteTime column prose and the Change-3 resolution-order note. Added a
  MULTI-version `.wal` acceptance test + a Rust `tests/query.rs` test + a direct
  `current_record_ts` unit test (forward and reverse fixtures) asserting the
  LATEST save's stamp is shown, not the first. All API used is confirmed public
  (`Archive::header`/`records`, `WchgHeader.direction`, `Direction` re-export,
  `ChangeRecord::is_splice`).

- MEDIUM-1 (Rust `tests/query.rs` changes + `log_bytes` test deletion
  understated) -- ADDRESSED. Replaced the generic "any assertion ... update to
  match" with the FOUR named tests and each one's exact fate:
  `query_subcommand_lists_tracked_files_with_index_size_rc0` BREAKS and is
  re-pointed/renamed to the `-l` Versions column; `query_long_listing_shows_
  attributes_and_log_bytes` is renamed + rewritten to assert the
  Versions/LastWriteTime/Bytes/Name header + separator + a parsed data row;
  `bare_archive_defaults_to_query_listing_rc0` and
  `query_mask_filters_to_matching_files` are stated as UNCHANGED decisions
  (name-containment only). Added the `names.rs` `log_bytes` `#[cfg(test)]`
  unit-test deletion explicitly to the wal-cli-common Files-touched line.

- NIT-1 (`terse_line` signature vs call site) -- ADDRESSED (no design change
  needed, per the finding). Added an explicit note at the `run` wiring that the
  call site drops the unused `&Archive` argument to match the one-arg
  `terse_line(name: &str)`; the design was already internally consistent.

- NIT-2 (`render_last_write` guard mid-char slice on a 20-byte non-conforming
  string) -- ADDRESSED. Added `stamp.is_char_boundary(10)/(11)/(19)` to the
  guard so a 20-byte string with an interior multi-byte char can never slice
  mid-char; added a boundary unit-test case (multi-byte char at position 5,
  asserts `"-"` and no panic) to the testability summary.

## Responses to design review -- FIRST pass (prior design-review)

Revision pass. All four MEDIUM and four NIT findings are ADDRESSED; none
backlogged or ignored. The review's verified assumptions confirmed the design's
load-bearing claims against live source; these responses tighten the HOW.

- MEDIUM-1 (`set_attributes_mtime` load-bearing, under-specified) -- ADDRESSED.
  Promoted option (a) to THE chosen design and deleted the option-(b) fallback
  (dead alternative). Pinned the setter: it lives in `mutate_stamp.rs`, signature
  `set_attributes_mtime(&mut self, name: &str, mtime: &str) -> Result<()>`,
  resolves via the existing `pub(crate)` `id_for_selector`, errors
  `NotTracked { name }` (matching the sibling name-based mutate seams), sets ONLY
  `attributes.mtime`, and is clock-free/FS-free. Added its `# Errors` doc and
  recorded WHY it is mandatory (no public mutable manifest accessor exists on
  `Archive`).

- MEDIUM-2 (LastWriteTime slice could panic) -- ADDRESSED. Added a single TOTAL
  helper `render_last_write(stamp: &str) -> String` that validates the fixed
  shape (`len == 20`, byte 10 == `b'T'`, byte 19 == `b'Z'`) BEFORE slicing and
  returns `"-"` otherwise. Both the record-`ts` path and the `attributes.mtime`
  fallback route through it via `last_write_time`, so a malformed external stamp
  degrades to `-` and never aborts the listing. Added a unit-test note.

- MEDIUM-3 (rendered example conflicts with format spec) -- ADDRESSED. Deleted
  the hand-drawn "Rendered shape" ASCII block and declared the Rust
  format-specifier block the single authority. Pinned the `Name` separator at 4
  dashes (`"-".repeat(4)`, the header label width), explicitly NOT the longest
  name, so the separator row is deterministic.

- MEDIUM-4 (sort order ambiguous / byte-vs-locale) -- ADDRESSED. Added an
  explicit SORT CONTRACT: `names.sort()` is `Vec<String>::sort` (byte-wise
  lexicographic), applied to the FINAL `names` vector AFTER the match so it
  covers BOTH the mask and no-mask branches, making terse and long order
  identical and deterministic. Noted the expected test arrays are in byte order.

- NIT-1 (`log_bytes` removal left as a grep decision) -- ADDRESSED. Stated as
  fact: `cmd_query.rs::long_line` is its only caller; delete `log_bytes` and its
  unit test. Updated the Files-touched line to match.

- NIT-2 (`WCHG_NAME_MAX_BYTES` id reservation overstates id size) -- ADDRESSED.
  Noted the real id is `f<N>` (2 bytes typical, <= ~8 bytes for tens of millions
  of files), so the 16-byte reservation is pure safety margin, not a constraint.

- NIT-3 (empty-archive-under-`-l` left open) -- ADDRESSED. Committed to the
  default: empty `-l` prints header + separator and no rows; terse-empty prints
  nothing. The header push is unconditional on `args.long`, not guarded by
  `!names.is_empty()`.

- NIT-4 (`bound_tail_bytes` lacked signature/tests) -- ADDRESSED. Gave it
  `fn bound_tail_bytes(s: &str, max: usize) -> String` with the
  forward-to-char-boundary rule, and named two unit tests
  (`wchg_member_name_bounds_a_deep_path_to_ustar_legal_length`,
  `bound_tail_bytes_keeps_a_utf8_boundary`).

No FORMAT.md change was triggered by any finding: all four MEDIUM items are CLI
rendering or an additive library metadata seam, consistent with Change 2 being
CLI-only and Change 3 reusing the existing `ts` field. The FORMAT.md section
2.1/3 edits from the first pass stand unchanged.
