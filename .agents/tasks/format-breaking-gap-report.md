# Walrus Format-Breaking Gap Report

Read-only audit of the Walrus codebase against the rewritten authoritative spec
in `FORMAT.md` (multi-file revision) and the design-decision companion
`CLI-DESIGN.md`. No code was modified. This report cites concrete file paths and
symbol names so the follow-up implementation can act on it directly.

---

## 1. Summary answer

The current implementation is a faithful reader/writer of the OLD single-file
format and must change in essentially every library module. The format changed
on five axes that break the code at once:

1. **Single-file -> multi-file.** The library models exactly ONE document per
   archive (`WalrusFile { manifest, base, log }`). The spec now requires MANY
   tracked files, each with its own base, its own `.wchg` log, and its own
   independent index range. This is the deepest change: it reshapes the central
   `WalrusFile` type, the manifest, the container reader/writer, and every API
   that takes "the base" / "the log".
2. **Tar layout.** `container.rs` / `write.rs` hard-code the three fixed members
   `manifest.json`, `base`, `log.jsonl`. The spec now requires `manifest.json`
   plus a per-file `files/<id>/base` and `files/<id>/<name>.wchg`.
3. **Inner log renamed + reframed.** `log.jsonl` becomes a self-describing
   `.wchg` with a MANDATORY first-line header object. No header type exists
   today and the parser/writer have no concept of it.
4. **Index convention flipped.** The code bakes in "index 0 = base = original"
   (`reconstruct.rs`, `Versions::base_index = 0`, `Version::Base`). The spec now
   says "index 0 = CURRENT for both variants; higher = older", with
   `base_index = count` for `.wal`. Reconstruct and version addressing are wrong
   under the new convention.
5. **Generations / re-base + mutate API.** The spec adds a per-file
   `generations` cap enforced by a re-base, plus the whole add/update/remove/
   condense/convert mutate surface and transactional temp-file-then-rename
   writes. NONE of this exists; the library is read + a single `new_wal`
   constructor only.

CLI and GUI consequences follow from the library: `walx` must be re-scoped to
extract-only and a new `walrus` binary built; `walker` must grow a two-pane
navigator over a mutate API it does not have today. The dependency boundary
(`walrus-fmt` GUI-free) is intact today and must stay so as the mutate surface
is added.

---

## 2. Evidence by file (Question A: what assumes the OLD single-file format)

### 2.1 `crates/walrus/src/lib.rs` -- the central type

- `struct WalrusFile { manifest: Manifest, base: Vec<u8>, log: Vec<ChangeRecord> }`
  models a SINGLE document. There is one `base` and one `log`. Multi-file
  requires a collection of tracked files (each with id, name, base, log,
  generations, checkpoints, versions).
- `WalrusFile::read` / `WalrusFile::open` call `container::read_container(bytes)`
  which returns the single `(Manifest, Vec<u8>, Vec<ChangeRecord>)` tuple -- must
  become a manifest + a map/list of per-file bases and logs.
- `new_wal(base, log)` builds one-document archives; multi-file needs a
  constructor over a set of files (or an empty-archive constructor plus `add`).
- `len()` / `is_empty()` / `resolve()` / `reconstruct()` / `to_bytes()` /
  `write()` all operate on "the" base and log with no file selector. Every one
  needs a per-file dimension (e.g. `reconstruct(file_id, version)`).
- Public re-exports in `lib.rs` leak the old shape:
  `pub use manifest::{Checkpoint, DocumentInfo, Manifest, Variant, Versions};`
  `DocumentInfo` (single top-level document) and the single-doc `Versions`
  placement change.
- Unit tests at the bottom (`empty_file_reconstructs_to_base`,
  `forward_splice_reaches_next_version`,
  `write_then_read_round_trips_every_version`) construct `WalrusFile` literally
  with single `base`/`log` fields and assert index 0 == base -- they encode BOTH
  the single-file shape AND the old index convention.

### 2.2 `crates/walrus/src/manifest.rs` -- manifest struct

- `struct Manifest` carries single-document fields at the TOP level:
  `document: DocumentInfo`, `versions: Versions`, `checkpoints: Vec<Checkpoint>`.
  The spec (section 3) moves ALL of these INTO a new `files: [...]` array, one
  entry per tracked file, and keeps only archive-level fields
  (`format`, `spec_version`, `variant`, `base_role`, `log_direction`, `created`,
  `generator`) at the top. There is NO `files` field today.
- `struct DocumentInfo { encoding, mime, name }` is the embryo of the per-file
  entry but is missing the spec's per-file fields: `id`, `attributes`
  (size/mtime/mode), `generations`, and the per-file `versions` + `checkpoints`.
- `struct Versions { count, base_index, head_index }` has
  `base_index` documented and constructed as "always 0 in 0.1"
  (`manifest.rs` doc comment + `new_wal` sets `base_index: 0`). The spec now
  says for `.wal` `base_index = count`, `head_index = 0`; for `.wbl`
  `base_index = 0`, `head_index = count`. This field is the index convention
  baked into the manifest and is WRONG for `.wal` under the new spec.
- `Manifest::new_wal(log_count)` hard-codes `base_index: 0, head_index: log_count`
  -- the OLD convention.
- `Manifest::validate()` only checks the format tag, spec version, and that each
  checkpoint index `<= versions.count`. It must become per-file validation
  (iterate `files[]`, validate each file's count/base_index/head_index/
  generations/checkpoints and the `.wchg` header mirror).
- No `generations` field anywhere in the manifest.

### 2.3 `crates/walrus/src/record.rs` -- change records

- Module doc says `//! Change-log records (log.jsonl).` -- stale name.
- `enum Op { Splice { at, delete_len, insert }, Checkpoint { index, sha256 } }`
  and `struct ChangeRecord { op, ts, note }` match the spec's section 4.4/4.5
  record shapes and are LARGELY REUSABLE. The gap is NOT the record body but the
  surrounding framing: there is no `.wchg` HEADER type
  (`{ format:"wchg", spec_version, direction, generations, name, encoding, count }`,
  spec section 4.2). A new header struct (e.g. `WchgHeader`) must be added here
  or in a new module, with a `direction: Direction { Forward, Reverse }` enum.
- `ChangeRecord::splice` constructor is fine; `is_splice()` is fine.

### 2.4 `crates/walrus/src/container.rs` -- reader

- Module doc: "inner: ustar tar with fixed members `manifest.json`, `base`,
  `log.jsonl`" -- the fixed-member assumption is the core break.
- `read_container` scans tar entries and matches EXACT paths
  `"manifest.json"`, `"base"`, `"log.jsonl"` into three `Option`s, then errors
  with `MissingMember("base")` / `MissingMember("log.jsonl")` if absent. Must
  instead: read `manifest.json`, then for each `files/<id>/` directory collect
  `files/<id>/base` and `files/<id>/<name>.wchg`, keyed by `<id>`.
- `parse_log(bytes)` treats EVERY non-empty line as a `ChangeRecord`. Under the
  new spec the FIRST line is the mandatory `.wchg` header and must be parsed
  separately; lines 2..N are records. The current parser would try to deserialise
  the header as a `ChangeRecord` and fail (or mis-parse).
- Error `WalrusError::Record { line, .. }` numbering is relative to `log.jsonl`;
  needs to account for the header line and name the file.
- gzip is still deferred (`decompress` returns an error for gzip) -- unchanged by
  this revision but worth noting for conformance (section 2 lists gzip as MAY).
- Container tests (`sniffs_zstd_magic`, `parses_jsonl_log_skipping_blanks`,
  `reports_line_number_on_bad_record`) encode the headerless `log.jsonl` framing
  and will break with the header requirement.

### 2.5 `crates/walrus/src/reconstruct.rs` -- reconstruct engine (Question B hot spot)

- `reconstruct(base, log, target_index)` applies the FIRST `target_index` splice
  records to `base`, with `applied == target_index` as the stop. This hard-codes
  the OLD convention: index 0 = base (0 records applied), higher index = more
  records applied = further from base. Under the new spec:
  - index 0 is CURRENT for both variants;
  - for `.wal` (forward log) the base is the ORIGINAL at `base_index = count`, so
    reaching index `k` means applying `count - k` forward records, NOT `k`;
  - for `.wbl` (reverse log) the base is CURRENT at index 0, so reaching index
    `k` means applying the FIRST `k` reverse records (this matches the current
    "apply k records" shape, but only for `.wbl`).
  The function currently has no notion of variant/direction at all -- it must
  take the variant (or direction) and the file's `count` to map an index to the
  number and direction of records to apply.
- The unit tests here (`replays_in_order`, `delete_then_insert`) assert
  `reconstruct(base, log, 0) == base` and `reconstruct(..., 1)` == one record
  applied -- i.e. the OLD index-0-is-base convention. They must be rewritten.

### 2.6 `crates/walrus/src/version.rs` -- version addressing (Question B)

- `enum Version { Index, Base, Head, Label, ForwardFromBase, BackFromHead }` and
  its doc comment: "0 is the stored base and len is the far end of the log". This
  is the OLD convention stated explicitly. Under the new spec, index 0 is CURRENT
  and `head`/`base` meanings depend on variant.
- `Version::Base => 0` and `Version::Head => len` in `resolve` bake in
  base==0/head==len. New semantics: for `.wal` base is `count` and head is 0; the
  `current`/`original` relative selectors from spec section 5
  (`current +/- n`, `head - n`, `base +/- n`) are only partially modelled
  (`ForwardFromBase`, `BackFromHead`) and under the wrong anchor.
- No `Version::Timestamp(T)` selector yet (spec section 5 item 3; a TODO comment
  acknowledges it).
- `resolve` takes a single `(manifest, len)` -- must take the per-file entry
  (its own count, base_index, checkpoints) because addressing is now PER FILE.
- Tests (`base_and_head_resolve_to_endpoints`, `relative_selectors_clamp`, etc.)
  assert base==0/head==len -- old convention, will break.

### 2.7 `crates/walrus/src/write.rs` -- writer

- `write_container(manifest, base, log)` writes the three fixed members via
  `build_tar`, which appends `"manifest.json"`, `"base"`, `"log.jsonl"`. Must
  become: write `manifest.json`, then for each tracked file write
  `files/<id>/base` and `files/<id>/<name>.wchg` (header line first, then
  records).
- `encode_log(log)` writes one JSON record per line with NO header. Needs to
  prepend the `.wchg` header object as line 1.
- `append_member` is generic and reusable (good), but the write path has no
  transactional temp-file-then-rename logic (spec section 6 / CLI-DESIGN 5.4a).
  `WalrusFile::write` in `lib.rs` does a plain `std::fs::write(path, bytes)` --
  not atomic.

### 2.8 `crates/walrus/src/error.rs` -- errors

- `MissingMember(&'static str)` with `"base"` / `"log.jsonl"` literals ties to
  the fixed layout. Needs variants for per-file members and a missing/invalid
  `.wchg` header, and probably a file-id dimension.
- `Record { line, reason }` doc says "within `log.jsonl`" -- rename/retarget to
  `.wchg` and include which file.
- No error variants for: unknown/invalid `.wchg` header, direction mismatch
  (header direction vs archive variant), file-not-found-in-archive (RC 8 cases),
  generations/re-base failures, or transactional-write failures.

### 2.9 `crates/walrus/src/export.rs`

- Operates on already-reconstructed `doc: &[u8]`, so it is variant/layout
  agnostic and LARGELY UNAFFECTED by the format change. The only indirect effect:
  callers now pick a (file, version) before exporting. No structural change
  needed here for the format break itself. (CSV export is still a stub, pdf/docx
  placeholders -- orthogonal to this audit.)

### 2.10 `crates/walx/src/main.rs` -- CLI (Question: keep vs move)

- Subcommands today: `info`, `extract`, `convert` (convert is a `bail!` stub).
- `cmd_info` prints `m.document.name/mime/encoding` and `doc.len()` -- assumes a
  single document; must iterate `files[]` for the new manifest.
- `cmd_extract(path, index)` reconstructs `Version::Index(index)` of the single
  document -- no file-mask, no multi-file selection.
- Per CLI-DESIGN section 4/7: `walx` becomes EXTRACT-ONLY. Keep the extract path
  (re-scoped to multi-file with a file-mask and `--index`); the full management
  set (query/add/update/remove/condense/convert) moves to a NEW `walrus` binary.
  `cmd_info` maps to `walrus query`; `cmd_convert` maps to `walrus convert`.
- `crates/walx/Cargo.toml` description still says "inspect, extract ..., and
  convert" and pulls export features; the description and scope need updating,
  and a new `crates/walrus-cli` (binary `walrus`) crate must be created.

### 2.11 `crates/walker/src/` -- GUI (where single-file assumptions live)

- `app.rs`: `WalkerApp` holds a single `player: Option<Player>` and
  `current_text`. `open_file` builds ONE `Player::new(file)` from the whole
  archive -- it assumes an archive IS one document. Multi-file requires a file
  LIST (the internal navigator pane) and a player bound to the SELECTED tracked
  file.
- `app.rs` has only an "Open..." action and Player/CSV views; there is NO
  navigator, no import(add/update), no remove, no condense, no generations
  control, no bloat indicator (all required by CLI-DESIGN section 6b).
- `player.rs`: `Player { file: WalrusFile, index, len, ... }` wraps the whole
  file and calls `file.reconstruct(Version::Index(self.index))`. `len = file.len()`
  is "the" log length -- must become per-selected-file. The loop/stop/wrap state
  machine itself is pure and reusable; only its binding to a single-doc
  `WalrusFile` and the index semantics (index 0 = current now) change. Note the
  seek slider / `step_forward`/`step_backward` assume higher index = toward head
  = "forward"; under index-0=current that is toward OLDER, which inverts the
  transport's semantic labels.
- `crates/walker/src/lib.rs` just re-exports `app` + `player`; no change beyond
  what those modules need.

### 2.12 Examples and docs

- `crates/walrus/examples/generate_walk.rs` and `generate_story.rs` both build a
  single-document `.wal` via `WalrusFile::new_wal(base, log)` and set
  `m.document = DocumentInfo { .. }`. Under multi-file + the removed top-level
  `document` field, these will not compile. They also rely on the OLD convention
  (base at index 0, head at `len`) and on `Version::Head`/`Version::Index(0)`
  meaning original/far-end. Both must be regenerated for the new manifest shape,
  the `.wchg` header, and the flipped index.
- Committed fixtures `examples/walrus.wal` and `examples/walrus-story.wal` are in
  the OLD on-disk layout (single `base` + `log.jsonl`, old manifest, no `.wchg`
  header). They will fail to open under a conforming new reader and must be
  REGENERATED after the writer lands.
- `README.md` describes "a base document plus an ordered change log" (singular),
  the three-crate table saying `walx` converts, and no mention of multi-file,
  `.wchg`, `walrus` CLI, generations, or the walker-as-full-frontend scope -- all
  stale.
- `ROADMAP.md` "Done" section describes the single-file reader/writer/examples as
  complete and does not reflect the multi-file re-work; `walx` convert is listed
  as a TODO but the `walrus` binary, multi-file, `.wchg`, generations, remove,
  condense, and transactional writes are absent from the roadmap.
- `crates/walrus/README.md` (not re-read in full here) is referenced as the
  crate readme; expect the same single-doc staleness.

---

## 3. Question B: where the index convention is baked in (precise citations)

The flip to "index 0 = current, higher = older" must be corrected at every one
of these symbols:

1. `crates/walrus/src/reconstruct.rs` -> `reconstruct()` -- stop condition
   `applied == target_index` encodes index == records-applied-from-base. This is
   the primary engine fix; it must map an index to records using the file's
   `count` and `direction`/`variant`.
2. `crates/walrus/src/manifest.rs` -> `struct Versions` field `base_index` (doc
   "always 0 in 0.1") and `Manifest::new_wal` setting `base_index: 0,
   head_index: log_count`. For `.wal` the new values are `base_index = count`,
   `head_index = 0`.
3. `crates/walrus/src/version.rs` -> `enum Version` doc ("0 is the stored base,
   len is the far end"), and `resolve()` arms `Version::Base => 0`,
   `Version::Head => len`, `ForwardFromBase`, `BackFromHead`.
4. `crates/walrus/src/lib.rs` -> `reconstruct` doc ("Valid indices are
   `0..=len()`") and all three unit tests asserting `Index(0) == base`.
5. `crates/walrus/src/reconstruct.rs` tests (`replays_in_order`,
   `delete_then_insert`) asserting `reconstruct(.., 0) == base`.
6. `crates/walker/src/player.rs` -> `index`/`len`, `step_forward`/`step_backward`
   semantics and the transport labels in `crates/walker/src/app.rs`
   ("|< base", "head >|", forward/backward) which assume higher index = toward
   head = newer.

---

## 4. Question C: the NEW public API surface walrus-fmt must expose

Both `walrus` (CLI) and `walker` (GUI) need the same read + mutate surface. None
of the following exists today (the crate exposes only read + `new_wal` +
`reconstruct` + `to_bytes`/`write`). Proposed additions (names illustrative):

Types that do not exist yet:
- `WchgHeader` (format, spec_version, `Direction`, generations, name, encoding,
  count) and a `Direction { Forward, Reverse }` enum -- the `.wchg` header
  (spec 4.2).
- A per-file entry type (e.g. `TrackedFile` / `FileEntry`) carrying
  `id, name, encoding, mime, attributes, versions, generations, checkpoints` --
  the element of the manifest `files[]` array (spec 3).
- A `FileAttributes` type (size, mtime, mode) -- spec 3 `attributes`.
- An archive-level `Archive` (or reshaped `WalrusFile`) owning `manifest` plus a
  collection of `(id -> base, log, header)`.
- A `FileId`/newtype for the stable archive-unique id (rust-standards favours a
  newtype over a bare `String`).

Enumeration / read:
- `archive.files() -> impl Iterator<Item = &TrackedFile>` (the `walrus query` /
  navigator listing: name, attributes, index size, generations, log byte size).
- `archive.file(id|name) -> Option<&TrackedFile>` and a mask/glob matcher for
  file-masks (query/extract/remove/condense all take a mask).
- `archive.reconstruct(file, Version) -> Result<Vec<u8>>` (per-file reconstruct).
- Per-file `index size` (count) and `log byte size` accessors (for query `-l`
  and the GUI bloat ratio).

Mutate API (the whole surface for add/update/remove/condense/convert +
generations), currently entirely absent:
- `add(file_name, bytes, generations: Option<u32>)` -- create-or-new-version;
  diffs against the current version to produce a splice record (CLI-DESIGN 5.3).
  This also requires a DIFF function (text -> splice) that does not exist; today
  records are only hand-authored by the example generators.
- `update(file_name, bytes, generations)` -- like add but fails RC 8 if absent
  (CLI-DESIGN 5.4).
- `remove(mask, force)` -- delete tracked file(s) (CLI-DESIGN 5.5).
- `condense(mask, keep: usize)` -- re-base to current, keeping N (CLI-DESIGN 5.6).
- `set_generations(file, n)` and the AUTOMATIC re-base trigger on add/update when
  the cap is exceeded (spec 4.3) -- shared re-base/trim primitive used by both
  `condense` and the generations cap.
- A re-base primitive: fold the oldest record into the base (variant-aware per
  spec 4.3) -- the single shared trimming operation.
- `convert()` / `to_wal()` / `to_wbl()` -- invert each splice and re-base to the
  opposite variant (spec 1; CLI-DESIGN; today a `walx convert` stub).
- Transactional write: build to a temp file and atomically rename
  (spec section 6 conformance; CLI-DESIGN 5.4a). `WalrusFile::write`'s plain
  `std::fs::write` must be replaced by this for all mutations.
- Standalone `.wchg` read/write (parse a bare `.wchg` using its header
  `direction` without a parent archive) -- spec section 6 "standalone reader".

Note: the diff engine (produce a minimal splice from old-bytes -> new-bytes) is
an implied dependency of add/update that has no home in the current code and no
dependency chosen; it must stay within the GUI-free boundary (a pure-Rust diff,
no app deps).

---

## 5. Question D: tests/fixtures that break, and untested new behaviour

Existing tests that WILL break (they encode the single-file layout and/or old
index convention):
- `crates/walrus/src/lib.rs` tests: `empty_file_reconstructs_to_base`,
  `forward_splice_reaches_next_version`,
  `write_then_read_round_trips_every_version` (construct `WalrusFile` with
  single `base`/`log`; assert index 0 == base).
- `crates/walrus/src/reconstruct.rs` tests: `replays_in_order`,
  `delete_then_insert`, `out_of_bounds_is_rejected` (old index semantics).
- `crates/walrus/src/version.rs` tests: `base_and_head_resolve_to_endpoints`,
  `relative_selectors_clamp`, `label_resolves_and_rejects_unknown`,
  `out_of_range_index_errors` (base==0/head==len).
- `crates/walrus/src/container.rs` tests: `parses_jsonl_log_skipping_blanks`,
  `reports_line_number_on_bad_record` (headerless `log.jsonl` framing).
  (`sniffs_zstd_magic`/`gzip`/`rejects_unknown_magic` survive -- codec sniffing
  is unchanged.)
- `crates/walker/tests/harness.rs`: `step_forward_button_advances_version_index_headless`
  builds `WalrusFile::new_wal(base, log)` and asserts `index 0` is the base /
  `step >` moves 0 -> 1 toward head. The constructor signature and the transport
  semantics change.

Fixtures to regenerate:
- `examples/walrus.wal`, `examples/walrus-story.wal` -- old on-disk layout; must
  be regenerated by updated `generate_walk.rs` / `generate_story.rs` after the
  multi-file writer + `.wchg` header land. The steering rule treats
  `examples/walrus.wal` as an integration fixture (forward-then-backward must be
  byte-identical) -- that test needs re-establishing on the new format.

New behaviour with NO current test coverage (per the repo's TDD discipline these
need failing tests written FIRST):
- Multi-file archive: multiple `files/<id>/` members read/written; per-file
  independent index ranges; empty archive (zero tracked files) still valid.
- `.wchg` header: mandatory first line present/well-formed; reject missing or
  non-`wchg` header; standalone `.wchg` replay using header `direction`;
  direction-vs-variant agreement.
- Index flip: `reconstruct(file, Index(0)) == current` for BOTH `.wal` and
  `.wbl`; byte-identical output for the same index across variants (the spec
  section 5 "MUST produce byte-identical" invariant; the steering rule calls this
  out explicitly).
- Generations cap: adding past `generations = N` drops the oldest via re-base;
  `count` decreases; index 0 stays current; `generations = 0` unlimited.
- Re-base / condense: `condense --keep n` equivalence to the generations trim;
  re-base folds the oldest record into the base variant-correctly.
- Transactional writes: a failed batch leaves the original byte-for-byte
  unchanged (temp-file-then-rename).
- `.wal <-> .wbl` conversion: round-trip equality (convert then convert back ==
  original; same reconstructions).
- Diff engine: old-bytes -> new-bytes produces a splice that reconstructs the new
  bytes.
- CLI (`walrus`/`walx`) subcommand behaviour over fixtures (query listing,
  extract RC 4 beyond-last-index, update RC 8 missing, remove RC 2 multi-match
  without `--force`) -- the return-code table in CLI-DESIGN section 6.
- Walker: navigator listing, import=add/update resolution, bloat indicator bands
  (green `R<2`, amber `2<=R<5`, red `R>=5`, 50-version floor) -- drive headlessly
  via `egui_kittest` per the testing steering.

---

## 6. Question E: recommended ordered re-work sequence

Ordered so each step leaves the workspace buildable and later steps build on
earlier ones. Keep `walrus-fmt` GUI-free throughout (invariant 2) and follow the
repo TDD cycle (write the failing test first, especially for the pure logic:
reconstruct, version addressing, re-base, player). Scoped `-p <crate>` checks per
the shell-and-gate rule; the full gate is the owner's manual step.

1. **Spec-anchored types first (manifest + `.wchg` header).** Reshape
   `manifest.rs`: move `document`/`versions`/`checkpoints` into a new
   per-file `TrackedFile` entry inside a `files: Vec<TrackedFile>`; add `id`,
   `attributes`, `generations`; fix `base_index` semantics per variant. Add the
   `WchgHeader` + `Direction` types (in `record.rs` or a new `wchg.rs`). Update
   `validate()` to iterate files. No behaviour yet -- just the data model + its
   serde round-trip tests. This unblocks everything else.
2. **Container reader for the per-file layout.** Rewrite `container.rs` to read
   `manifest.json` + `files/<id>/base` + `files/<id>/<name>.wchg` (parse the
   header line, then records). Update `WalrusFile`/`Archive` in `lib.rs` to hold
   the per-file collection. Reader round-trip tests against hand-built in-memory
   archives.
3. **Index flip in reconstruct + version addressing.** Make `reconstruct` and
   `version::resolve` variant/direction aware so `Index(0) == current` for both,
   and assert byte-identical output across variants (spec section 5). This is
   pure logic -- strict TDD. Fix the `lib.rs`/`reconstruct.rs`/`version.rs` tests
   to the new convention.
4. **Writer for the per-file layout + `.wchg` header + transactional write.**
   Rewrite `write.rs` to emit the new members and the header line; add atomic
   temp-file-then-rename and route `WalrusFile::write` through it. Round-trip
   tests: write then read reconstructs every index of every file.
5. **Re-base primitive, then generations + condense.** Implement the single
   variant-aware re-base (fold oldest into base), then `condense(keep)` and the
   automatic generations trim on add/update sharing it. Tests for cap enforcement
   and condense equivalence.
6. **Diff + mutate API (add/update/remove/set-generations) and convert.** Add the
   pure-Rust diff (bytes -> splice), then `add`/`update`/`remove`/
   `set_generations`, and `.wal <-> .wbl` `convert` (invert splices + re-base).
   All mutations transactional. Convert round-trip test.
7. **Regenerate examples + fixtures.** Update `generate_walk.rs` /
   `generate_story.rs` to the new multi-file manifest + `.wchg` + flipped index;
   regenerate `examples/*.wal`; re-establish the forward-then-backward
   byte-identical integration test.
8. **CLI re-scope.** Re-scope `walx` to extract-only (multi-file + file-mask +
   `--index`, RC 4 behaviour); create a new `walrus` binary crate implementing
   query/add/update/remove/condense/convert with the POSIX/GNU conventions and
   the RC table (CLI-DESIGN sections 4-6). Subcommand behaviour tests over
   fixtures.
9. **Walker full frontend.** Add the two-pane navigator, per-file player binding,
   import=add/update resolution, remove/condense/set-generations, and the bloat
   indicator -- all over the walrus-fmt mutate API (boundary preserved, GUI deps
   stay in walker). Headless `egui_kittest` tests for navigator/import/bloat
   bands.
10. **Docs.** Update `README.md`, `crates/walrus/README.md`, and `ROADMAP.md` to
    the multi-file reality, the `walrus`/`walx` split, `.wchg`, generations, and
    walker-as-frontend. (`FORMAT.md`/`CLI-DESIGN.md` are already the source of
    truth.)

Dependency-boundary note: the diff engine (step 6) and all mutate/re-base logic
MUST live in `walrus-fmt` with no app/GUI dependency, so both the `walrus` CLI
and `walker` call the identical surface -- this is exactly what CLI-DESIGN
section 6b requires and what keeps the FFWB CR-NR-107 consumer able to embed.

---

## 7. Conclusions

- The format change touches every library module except `export.rs` (and even
  there, callers must now pick a file+version). The single-file assumption is
  structural, living in `WalrusFile`'s fields, the manifest's top-level
  single-document fields, the fixed tar members, and the headerless `log.jsonl`.
- The index flip is small in code but spread across six symbols (section 3) and
  is the highest-risk correctness change because the spec mandates byte-identical
  cross-variant output; it must be TDD'd.
- The largest NEW work is the mutate surface (add/update/remove/condense/convert
  + generations re-base + a diff engine + transactional writes), which does not
  exist at all today and is the shared foundation both the `walrus` CLI and the
  `walker` frontend depend on.
- Fixtures and all current tests encode the old format and will need
  regenerating/rewriting; several brand-new behaviours have zero coverage.
- The dependency boundary is currently intact and is preserved by putting the
  entire mutate surface in `walrus-fmt`; no GUI/app dependency should enter the
  library.

No code was changed in producing this report.
