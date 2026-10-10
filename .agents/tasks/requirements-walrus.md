# Walrus stream requirements -- `walrus-fmt` library + `walrus` / `walx` CLIs

Status: REQUIREMENTS (planning). Derived from the authoritative design in
`FORMAT.md`, `CLI-DESIGN.md`, and the audit in
`.agents/tasks/format-breaking-gap-report.md`. This document is the shared
contract for the WALRUS work stream (its own chat + the
`.worktrees/walrus-core` worktree, branch `feature/walrus-core`).

ASCII only (`->`, `<->`, `<=`, `>=`, `--`) per the documentation steering rule.

## 0. Scope and ownership

This stream owns:
- `crates/walrus` (`walrus-fmt`): the multi-file format, reconstruct engine,
  version addressing, diff/mutate surface, transactional writes. GUI-free.
- `crates/walx`: the extract-only CLI (`extract` + `cat`).
- A NEW `crates/walrus-cli` (binary `walrus`): the full management CLI.

This stream does NOT touch `crates/walker` (the WALKER stream owns it).

Hard invariants (project-conventions), enforced by review:
- INV-1 FORMAT.md is the source of truth; code must not diverge. A needed format
  change updates FORMAT.md first.
- INV-2 `walrus-fmt` stays GUI-free and app-free: no egui/eframe/ff-*; export
  backends behind optional features only. The diff engine must be pure-Rust with
  no app/GUI dependency.
- INV-3 `.wal` and `.wbl` are symmetric and inter-convertible; reconstruction is
  byte-identical for the same index across variants.
- INV-4 Splices are the single change primitive (`{at, delete_len, insert}`).
- INV-5 Container reader sniffs codec from magic bytes, not extension.
- Source rules: every `.rs` under 400 non-test lines; strict ASCII; no
  unwrap/expect in library code (thiserror + crate::Result); TDD (failing test
  first for all pure logic).

## 1. Cross-stream contract (what WALKER depends on)

The WALKER stream builds against the `walrus-fmt` PUBLIC API below. These
signatures are the coordination contract: Walker's API-dependent work is
BLOCKED until the item is marked implemented here. Names are the committed
target; a change to any of them must be mirrored into the Walker requirements.

Required public surface (illustrative signatures, finalise in design):

```
// Identity + per-file model
pub struct FileId(/* opaque, archive-unique */);
pub struct FileAttributes { size: u64, mtime: Option<i64>, mode: Option<u32> }
pub struct TrackedFile { /* id, name, encoding, mime, attributes,
                            index_size (count), generations, checkpoints */ }

// Archive (reshaped WalrusFile)
pub struct Archive { /* manifest + per-file (base, log, wchg header) */ }
impl Archive {
    pub fn open(path) -> Result<Archive>;          // read + sniff codec
    pub fn files(&self) -> impl Iterator<Item=&TrackedFile>;
    pub fn file(&self, sel: &str) -> Option<&TrackedFile>;   // name/id
    pub fn matching(&self, mask: &str) -> Vec<&TrackedFile>;  // glob mask
    pub fn reconstruct(&self, file: &FileId, v: Version) -> Result<Vec<u8>>;
    pub fn log_bytes(&self, file: &FileId) -> u64;  // for bloat ratio
    // mutate (transactional at the archive level)
    pub fn add(&mut self, name, bytes, generations: Option<u32>) -> Result<()>;
    pub fn update(&mut self, name, bytes, generations: Option<u32>) -> Result<()>;
    pub fn remove(&mut self, mask, force: bool) -> Result<()>;
    pub fn condense(&mut self, mask, keep: usize) -> Result<()>;
    pub fn set_generations(&mut self, file, n: u32) -> Result<()>;
    pub fn convert(&self) -> Result<Archive>;       // .wal <-> .wbl
    pub fn write(&self, path) -> Result<()>;         // transactional
}

// Version addressing (index 0 = CURRENT for both variants)
pub enum Version { Index(usize), Current, Original, Relative(i64), Label(String),
                   Timestamp(i64) }
pub enum Direction { Forward, Reverse }
```

WALKER-FACING GUARANTEES:
- `reconstruct(file, Version::Index(0))` is the CURRENT version for BOTH variants.
- `reconstruct` returns the stored bytes; Walker renders/exports from those bytes.
- All mutate ops are transactional (temp-file-then-atomic-rename); a failed batch
  leaves the archive byte-for-byte unchanged.
- `log_bytes` + current-version byte length give Walker its bloat ratio.

## 2. Functional requirements

Each FR has acceptance criteria (AC) to be met by a failing-test-first cycle.
Ordering follows the gap report's re-work sequence (buildable at each step).

### FR-W1 Multi-file manifest + `.wchg` header types
Reshape `manifest.rs`: archive-level fields (`format`, `spec_version`, `variant`,
`base_role`, `log_direction`, `created`, `generator`) at top; a
`files: Vec<TrackedFile>` array, each entry carrying `id`, `name`, `encoding`,
`mime`, `attributes`, per-file `versions {count, base_index, head_index}`,
`generations`, `checkpoints`. Add `WchgHeader {format:"wchg", spec_version,
direction, generations, name, encoding, count, granularity_hint}` and
`Direction {Forward, Reverse}`.
- AC1 Manifest serde round-trips (serialize -> parse -> equal) for a 0-file,
  1-file, and N-file archive.
- AC2 `base_index`/`head_index` match the spec per variant: `.wal` ->
  base_index=count, head_index=0; `.wbl` -> base_index=0, head_index=count.
- AC3 `validate()` iterates files and rejects a file whose checkpoint index >
  count, and rejects a `.wchg` header whose `direction` disagrees with the
  archive variant.
- AC4 A missing/non-`wchg` header is a typed error (not a mis-parsed record).

### FR-W2 Container reader for the per-file layout
Rewrite `container.rs` to read `manifest.json`, then for each `files/<id>/`
collect `files/<id>/base` and `files/<id>/<name>.wchg` (header line first, then
records). Codec sniffing (zstd/gzip magic) is UNCHANGED (INV-5).
- AC1 Reads a hand-built in-memory multi-file archive into an `Archive` with the
  correct per-file bases, logs, and headers.
- AC2 First `.wchg` line parsed as the header; lines 2..N as records; a bad
  record reports its file and line number (header-adjusted).
- AC3 zstd magic still sniffed; unknown magic rejected; gzip remains a documented
  deferral (reader errors clearly, not a panic).

### FR-W3 Index flip in reconstruct + version addressing (INV-3, highest risk)
`reconstruct` and `version::resolve` become variant/direction aware. Index 0 =
current for both; `.wal` reaching index k applies `count-k` forward records;
`.wbl` reaching index k applies the first k reverse records. Add
`Version::Timestamp`.
- AC1 `reconstruct(file, Index(0))` == current for a `.wal` AND a `.wbl` fixture.
- AC2 Byte-identical output for the same index across a `.wal` and its converted
  `.wbl` (strict assertion of INV-3).
- AC3 `Current`/`Original`/`Relative`/`Label`/`Timestamp` resolve per-file and
  clamp at range ends; out-of-range index is a typed error.

### FR-W4 Container writer + transactional writes
Rewrite `write.rs` to emit `manifest.json` + per-file `files/<id>/base` and
`files/<id>/<name>.wchg` (header line first). Route all writes through atomic
temp-file-then-rename.
- AC1 write-then-read reconstructs every index of every file byte-identically.
- AC2 A simulated mid-write failure leaves the original archive byte-for-byte
  unchanged (no partial/corrupt output).

### FR-W5 Re-base primitive + generations + condense
One variant-aware re-base (fold oldest into base). `generations=N` auto-trims on
add/update; `generations=0` unlimited (default). `condense(mask, keep)` is the
one-shot manual form sharing the same trim.
- AC1 Adding past `generations=N` drops the oldest via re-base; count decreases;
  index 0 stays current; reconstructions of surviving indices unchanged.
- AC2 `condense --keep n` keeps indices 0..=n and discards older; `--keep 0`
  collapses to current only.
- AC3 Re-base is variant-correct for both `.wal` and `.wbl`.

### FR-W6 Diff engine (text -> splices)
Pure-Rust diff behind a thin adapter emitting byte-offset splices. PRIMARY:
`similar` (dependency-free). Adaptive granularity (line vs word/char by content
sniff); grapheme-aware for char granularity; granularity recorded in the `.wchg`
header. Binary input REJECTED on add/update with a clear error.
- AC1 diff(old,new) produces splices that reconstruct `new` exactly.
- AC2 Line-dense content diffs at line granularity; a no-line-break blob falls
  back to word/char; char splits land on grapheme boundaries (CJK fixture).
- AC3 Binary input is rejected (typed error), not turned into garbage splices.
- AC4 `cargo tree -p walrus-fmt --features <diff>` adds no app/GUI crate (INV-2).

### FR-W7 Mutate API + convert
`add` (create-or-new-version), `update` (RC-8 if absent), `remove(mask, force)`,
`set_generations`, `convert()` (invert splices + re-base to the other variant).
All transactional (FR-W4).
- AC1 add creates a missing file (first version = base = current); a second add
  diffs against current and appends a record.
- AC2 update on a missing file fails with the typed "must add" error.
- AC3 remove of a multi-match mask without `force` refuses and lists matches;
  with `force` removes all.
- AC4 convert round-trip: `.wal` -> `.wbl` -> `.wal` reconstructs every index
  identically to the original (INV-3).

### FR-W8 Internationalization / encoding
UTF-8 internal storage. On add, detect or accept a declared source encoding,
transcode to UTF-8 via `encoding_rs`, record the original encoding per file. On
extract, optionally transcode back.
- AC1 A GBK/Shift-JIS/Big5 source adds correctly, stores UTF-8, records the
  original encoding; extract with keep-original restores the original bytes.
- AC2 `encoding_rs` adds no app/GUI dependency (INV-2).

### FR-W9 Standalone `.wchg`
A bare `.wchg` is replayable outside its archive using its header `direction`.
- AC1 A standalone `.wchg` + its base reconstructs without a parent manifest.

### FR-W10 `walx` re-scope (extract-only)
`walx` provides `extract` (restore to disk, multi-file + file-mask + `--index`,
`-o <dir>`, `--force`) and `cat` (one file's bytes to stdout). Mutates nothing.
Drop `info`/`convert` from `walx` (they move to `walrus`). Remove export features
from `walx`.
- AC1 `extract` restores selected files at a version under stored names; existing
  target refused unless `--force`.
- AC2 `extract --index` beyond last recorded index writes the ORIGINAL with a
  warning and exits RC 4.
- AC3 `cat` requires exactly one matched file (RC 2 otherwise); writes raw bytes
  to stdout; never touches disk.

### FR-W11 `walrus` CLI (new binary)
New `crates/walrus-cli` (binary `walrus`), subcommand canonical with GNU/POSIX
conventions and convenience flag aliases:
`query`, `extract`, `cat`, `add`, `update`, `remove`, `condense`, `convert`.
No `export` subcommand (export is Walker-only; INV-2). Return codes per the
table (0 ok, 2 usage, 4 index-beyond-last, 8 target-missing).
- AC1 `query` (default) lists tracked files + index size; `-l` adds attributes +
  log bytes.
- AC2 Mutually exclusive operations: supplying two ops is RC 2.
- AC3 `update`/`remove`/`condense` on a non-existent target -> RC 8.
- AC4 `convert` changes the archive variant and round-trips (FR-W7 AC4).
- AC5 Subcommand behaviour verified over fixtures (not just unit tests).

### FR-W12 Regenerate examples + fixtures
Update `generate_walk.rs` / `generate_story.rs` to the multi-file manifest +
`.wchg` + flipped index; regenerate `examples/*.wal`.
- AC1 `examples/walrus.wal` plays forward then backward byte-identically (the
  integration fixture from the testing steering).

## 3. Non-functional requirements
- NFR-1 Each `.rs` under 400 non-test lines; split by concern (e.g. `wchg.rs`,
  `rebase.rs`, `diff.rs`) rather than growing a file.
- NFR-2 `cargo fmt -- --check` and `cargo clippy -- -D warnings` clean.
- NFR-3 Boundary proof: `cargo build -p walrus-fmt` (no features) links none of
  the diff/export/encoding heavy crates per feature gating; `cargo tree` asserts
  it.
- NFR-4 Full gate (`cargo gate --build`) is the owner's manual step; scoped
  `-p walrus-fmt` / `-p walx` / `-p walrus-cli` are the inner loop.

## 4. Build / dependency notes
- `similar` (diff) and `encoding_rs` (i18n) are new `walrus-fmt` dependencies;
  gate them so a playback-only embedder can opt out if they prove heavy (confirm
  dependency-free claim for `similar` at implementation time).
- MSRV currently 1.95 (egui 0.36 on the walker side set it workspace-wide). If a
  new library crate forces > 1.95, raise the WORKSPACE rust-version and record
  the forcing crate (FR note; follows the established procedure).

## 5. Ordered delivery (buildable at each step)
Mirrors the gap report section 6: FR-W1 -> FR-W2 -> FR-W3 -> FR-W4 -> FR-W5 ->
FR-W6 -> FR-W7 (+ FR-W8, FR-W9) -> FR-W12 (regenerate) -> FR-W10 (`walx`) ->
FR-W11 (`walrus`). Keep `walrus-fmt` GUI-free throughout.

## 6. Signal to the WALKER stream
When the public surface in section 1 is implemented and on `main` (or a shared
integration branch), post the resolved API here so the WALKER stream can
un-block its API-dependent requirements. Until then WALKER builds only the
format-agnostic parts (shell, navigator layout, theming, `HistorySource` seam).
