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
- INV-3 Forward and reverse files are symmetric and inter-convertible;
  reconstruction is byte-identical for the same index of a given file regardless
  of its direction. This is a PER-FILE property: direction is taken from the
  file's own `.wchg` header (authoritative), the archive `variant`/extension is
  never consulted for reconstruction, and an archive MAY hold a mix of forward
  and reverse files.
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
    // `dir` overrides the default direction ONLY when CREATING the file;
    // it is IGNORED for an already-tracked file (direction is immutable
    // once tracked -- only `convert` may flip an existing file).
    pub fn add(&mut self, name, bytes, generations: Option<u32>,
               dir: Option<Direction>) -> Result<()>;
    // `update` diffs old-vs-new to PRODUCE forward splices, then funnels
    // through the same storage/transaction path as `update_with_splices`.
    pub fn update(&mut self, name, bytes, generations: Option<u32>) -> Result<()>;
    // Editor-facing: caller supplies forward-sense splices directly (its
    // coalesced piece list for one save). The library applies the file's
    // stored direction (forward: store as-is; reverse: invert + advance base).
    pub fn update_with_splices(&mut self, name, splices: &[Splice]) -> Result<()>;
    pub fn remove(&mut self, mask, force: bool) -> Result<()>;
    pub fn condense(&mut self, mask, keep: usize) -> Result<()>;
    pub fn set_generations(&mut self, file, n: u32) -> Result<()>;
    // per-file direction flip is the primitive; whole-archive is the
    // no-mask degenerate case. Optional file mask + target direction.
    pub fn convert(&self, mask: Option<&str>, to: Option<Direction>)
        -> Result<Archive>;
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
  count. It does NOT reject a `.wchg` header whose `direction` disagrees with the
  archive `variant`: direction is per-file and authoritative, so a mixed-direction
  archive is legal (a file forward or reverse under either variant passes).
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
`reconstruct` and `version::resolve` become direction aware. Reconstruction
direction is taken PER FILE from that file's `.wchg` header, NOT from the archive
`variant` or extension. Index 0 = current for both directions; a forward file
reaching index k applies `count-k` forward records; a reverse file reaching
index k applies the first k reverse records. Add `Version::Timestamp`.
- AC1 `reconstruct(file, Index(0))` == current for a forward AND a reverse file
  fixture, including a MIXED archive holding one of each.
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
`add` (create-or-new-version), `update` (RC-8 if absent), `update_with_splices`
(editor-supplied forward splices), `remove(mask, force)`, `set_generations`,
`convert()` (per-file invert splices + re-base; see FR-W11). All transactional
(FR-W4).

Lifecycle (add-first then update-after): `add` establishes the base; `update`
and `update_with_splices` append a change record and fail with the typed "must
add first" error (RC-8) on an untracked file. Add-on-first-save: an editor adds
a file on its FIRST SAVE with the real content (not on file-create). Index 0 is
ALWAYS the current version; a freshly-added file has count=0 and only index 0
exists. An empty first version exists ONLY if the caller deliberately adds empty
content, and that empty state lands at the FAR (highest) index, never at index 0.
One record per SAVE: a writer coalesces the editor's piece list since the last
save into the single NET splice, so the version index counts saves, not
keystrokes.
- AC1 add creates a missing file (first version = base = current, count=0); a
  second add diffs against current and appends a record.
- AC2 update (and `update_with_splices`) on a missing file fails with the typed
  "must add" error (RC-8).
- AC3 remove of a multi-match mask without `force` refuses and lists matches;
  with `force` removes all.
- AC4 convert round-trip: forward -> reverse -> forward reconstructs every index
  identically to the original (INV-3).
- AC5 `update_with_splices` with caller-supplied FORWARD splices reconstructs
  byte-identically and respects the file's STORED direction: a forward file
  stores the splice as-is (base unchanged); a reverse file INVERTS the incoming
  splice and ADVANCES the base to the new current bytes. `update(bytes)` is the
  convenience path that diffs old-vs-new to produce the same forward splices and
  funnels through the same storage/transaction path.

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
- AC4 `extract` and `cat` accept the version selectors `--version current`
  (default, index 0), `--version original` (each file's oldest -- resolves PER
  FILE, RC 0 no warning), `--index <n>` (numeric; beyond-last -> original + warn
  + RC 4 per AC2), `--version <+/-n>` (relative), and `--at <T>` (UTC-timestamp
  snapshot: per file, the version whose record is newest at-or-before `T`; `T`
  before a file's earliest record -> that file's current). Each selector resolves
  per file against that file's own `[0, count]` via the library `Version` enum
  (FR-W3); supplying two selectors is RC 2. `--label` is deferred (OPEN). `--at`
  requires `T` in the fixed UTC shape `YYYY-MM-DDThh:mm:ssZ`.

### FR-W11 `walrus` CLI (new binary)
New `crates/walrus-cli` (binary `walrus`), subcommand canonical with GNU/POSIX
conventions and convenience flag aliases:
`query`, `extract`, `cat`, `add`, `update`, `remove`, `condense`, `convert`.
No `export` subcommand (export is Walker-only; INV-2). Return codes per the
table (0 ok, 2 usage, 4 index-beyond-last, 8 target-missing).
`add` and `update` additionally accept a single `-` source operand that reads
content from STDIN with a required `--name <stored-name>` (CLI-DESIGN section
5.4e); see AC9.
- AC1 `query` (default) lists tracked files + index size; `-l` adds attributes +
  log bytes.
- AC2 Mutually exclusive operations: supplying two ops is RC 2.
- AC3 `update`/`remove`/`condense` on a non-existent target -> RC 8.
- AC4 `convert` flips per-file direction (optional file-mask; whole-archive is
  the no-mask degenerate case) with a `--to wal|wbl` target, round-trips
  (FR-W7 AC4), MAY update the manifest `variant` to the new predominant
  direction and MAY SUGGEST an extension rename, but MUST NOT auto-rename the
  file on disk (an explicit `-o <name>` or a user rename changes the extension).
- AC5 `add` accepts `--wal`/`--wbl` (equivalently `--direction forward|reverse`)
  that overrides the default direction ONLY when creating a new file; for an
  already-tracked file the flag is IGNORED and the existing direction kept
  (per-file direction is immutable once tracked). When CREATING a brand-new
  archive with no manifest, the seed order is: explicit flag, else extension
  (.wal->wal, .wbl->wbl), else default wal/forward for a bare name. For an
  EXISTING archive the manifest `variant` drives the default (extension not
  consulted), keeping behaviour identical on extension-blind Unix systems.
- AC6 Subcommand behaviour verified over fixtures (not just unit tests).
- AC7 `add` and `update` accept `-R` / `--recurse` (CLI-DESIGN section 5.4b).
  This is a CLI-LAYER argument expansion, NOT a new library primitive: the CLI
  walks each directory argument and calls the existing mutate API
  (`Archive::add` / `Archive::update`) once per discovered file, passing the
  file's path RELATIVE to the recursion root (root name included, `/`-separated
  on every platform) as the `name`. The manifest `name` already permits `/`
  (FR-W1), so no format change is needed; INV-1 is untouched.
  - AC7a Without `-R`, a directory argument is a usage error (RC 2).
  - AC7b With `-R`, `walrus add <arc> <dir> -R` stores every regular file under
    `<dir>` as `<dir-name>/<relative-path>`; a following `walx extract` recreates
    the identical on-disk tree (FR-W10 AC1).
  - AC7c The walk is deterministic (entries ingested in sorted relative-path
    order), follows regular files only (symlinks not followed; devices/sockets/
    FIFOs skipped with a stderr note, not a batch failure), and leaves empty
    directories unrepresented.
  - AC7d A binary file found during the walk is rejected like any binary input
    (FR-W6 AC3); under the batch transaction (FR-W4) that aborts the whole add/
    update and leaves the archive byte-for-byte unchanged.
  - AC7e `update <arc> <dir> -R` matches each recursed file by its relative name
    against the archive; a recursed file not already tracked fails RC 8 (FR-W7
    AC2) and, being transactional, changes nothing. (`--skip-binary` and
    include/exclude globs are noted OPEN in CLI-DESIGN 5.4b, not required here.)
- AC8 `add` / `update` stamp each recorded change with a UTC timestamp in the
  fixed shape `YYYY-MM-DDThh:mm:ssZ` (the save instant, seconds resolution), so
  `--at` snapshot addressing (FR-W10 AC4) works on CLI-produced archives.
  Timestamps are stored and compared in UTC only, never local time; the library
  parser accepts only the fixed `...Z` shape (FORMAT.md 4.1 / section 5). Display
  in local time is a UI concern (Walker), never stored. Fixtures/tests use fixed
  timestamp strings; only the live add/update path reads the wall clock.
- AC9 `add` and `update` read content from STDIN when the single file operand is
  `-`, naming the tracked file with a REQUIRED `--name <stored-name>` (CLI-DESIGN
  section 5.4e). Like `-R` (AC7), this is a CLI-LAYER source: the CLI buffers
  stdin to EOF and calls the existing mutate API (`Archive::add` /
  `Archive::update`) with those bytes and the supplied name -- NO new library
  primitive and NO format change (INV-1 untouched).
  - AC9a `cmd | walrus add <arc> - --name foo.txt` creates/updates `foo.txt`
    from the piped bytes; a following `walx cat <arc> foo.txt` reproduces the
    piped content byte-identically.
  - AC9b A `-` source WITHOUT `--name` is RC 2; `--name` supplied WITHOUT a `-`
    source (i.e. alongside file arguments) is RC 2.
  - AC9c `-` mixed with any file-path argument, `-` given more than once, or `-R`
    combined with `-`, is RC 2 (stdin is a single stream, nothing to walk).
  - AC9d The buffered stdin bytes obey the text-only rule (FR-W6 AC3 / section
    2.2.2): a non-UTF-8 / NUL-bearing stream is rejected with `BinaryInput`
    (non-zero), and under the transaction (FR-W4) the archive is unchanged.
  - AC9e `update <arc> - --name bar.txt` on an untracked `bar.txt` fails RC 8
    (FR-W7 AC2); `--generations`, the create-time direction flags (AC5), the
    no-change no-op (FR-W11 note / CLI-DESIGN 5.4), and transactional writes
    behave identically to a file-sourced add/update.
  - AC9f `--name` is stored VERBATIM as the manifest `name` (`/`-separated,
    section 5.4c) and is subject to the path-escaping / reserved-name refusals
    (section 5.4d); an escaping or illegal `--name` is RC 2.

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

### API READY -- 2026-10-11 (branch `feature/walrus-core`, tip 05c2975)

The section-1 public API surface is IMPLEMENTED, reviewed, and verified green on
the `feature/walrus-core` integration branch (FR-W1 through FR-W9 landed;
FR-W10/W11/W12 -- the CLIs and example regeneration -- are still pending but do
NOT block Walker). WALKER may now un-block its API-dependent requirements and
build the `WalrusSource` adapter against this surface.

Resolved public surface on `walrus_fmt`:
- `Archive::open(path)` / `Archive::read(bytes)` -- read + codec-sniff.
- `Archive::files()`, `Archive::file(sel)`, `Archive::reconstruct(&FileId, Version)`
  -- version 0 = CURRENT for both variants; byte-identical per index (INV-3).
- Mutate (all transactional, temp-file-then-atomic-rename):
  `add(name, bytes, generations, dir)`, `update(name, bytes, generations)`
  (behind the `diff` cargo feature -- it diffs old-vs-new via `similar`),
  `update_with_splices(name, &[splices])` (ALWAYS available; forward-sense
  splices; a reverse file inverts + advances the base), `remove(mask, force)`,
  `set_generations(file, n)`, `condense(mask, keep)`, `enforce_generations(id)`,
  `convert(mask, to)` (per-file direction flip; byte-identical round-trip),
  `write(path)` / `to_bytes()`.
- `Version { Index, Current, Original, Relative, Label, Timestamp }`,
  `Direction { Forward, Reverse }`, `FileId`, `TrackedFile`, `WchgHeader`.
- Typed errors: `NotTracked` (update/remove on an untracked file -- RC-8 sense),
  `AmbiguousRemove` (multi-match without force), `SpliceNotCharBoundary`,
  `BinaryInput`, `Rebase`, plus the existing version/checkpoint/IO variants.

Feature gating (dependency boundary, INV-2): the DEFAULT build links NEITHER
`similar` NOR `encoding_rs` and no GUI/app crate. Enable the `diff` feature for
`update(bytes)` (text -> splices) and the `i18n` feature for the encoding
transcode path. A playback-only embedder uses `update_with_splices` and needs
neither. Verified via `cargo tree` default vs `--features "diff i18n"`.

NOTE on location: the API is on `feature/walrus-core`, the WALRUS stream's
integration branch; it is NOT yet merged to `main`. Walker should build against
`feature/walrus-core` (or a branch based on it) until the stream lands on `main`.
