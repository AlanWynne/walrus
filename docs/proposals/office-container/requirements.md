# Office container requirements -- ODF / OOXML unpack-version-repack

Status: REQUIREMENTS (active implementation). The normative office-format
contract is FORMAT.md section 8 (0.1 draft), and these requirements are derived
from that contract, the existing stream contract in
`.agents/tasks/requirements-walrus.md`, and the owner's decision thread captured
in `docs/proposals/office-container/decisions.md`. This document is the shared
contract for the OFFICE-CONTAINER work stream. It slots ALONGSIDE
`requirements-walrus.md` and extends -- never breaks -- the public API surface
that document already lists (Archive / TrackedFile / FileId / Version /
Direction / add / update / update_with_splices / remove / condense / convert /
write).

ASCII only (`->`, `<->`, `<=`, `>=`, `--`) per the documentation steering rule.

## 0. Scope and ownership

This stream adds the ability to store an OFFICE DOCUMENT inside a Walrus archive
by UNPACKING it into its inner parts, tracking each part as its own tracked file
grouped under one logical document, and REPACKING the parts back into a
byte-identical office file on extract.

In scope (ZIP-based office containers only):
- OpenDocument Format (ODF): `.odt` (text), `.ods` (spreadsheet),
  `.odp` (presentation), and the ODF family in general (`.odg`, `.odf`, ...).
- Office Open XML (OOXML / OPC): `.docx`, `.xlsx`, `.pptx`.

Out of scope (stated explicitly):
- Legacy Microsoft binary formats `.doc` / `.xls` / `.ppt`. These are OLE2 /
  CFBF compound binary files, NOT ZIP containers, so the unpack-to-XML-parts
  model does not apply. They are DEFERRED (see FR-OC1 AC3 and the risks section
  of `design.md`); a future stream may add an OLE2 reader.
- Content EXPORT / transformation (html/pdf/docx conversion of CONTENT). That is
  a Walker-only concern (project-conventions INV-2) and unaffected here. This
  feature stores and reproduces the ORIGINAL office bytes; it never transforms
  content.

This stream owns:
- A NEW office module tree in `crates/walrus` (`walrus-fmt`), gated behind a new
  optional cargo feature (working name `office`; MAY split into
  `container-odf` / `container-ooxml`). This is where the heavy ZIP (un)pack and
  envelope logic lives.
- Additive manifest changes in `crates/walrus/src/manifest.rs` (a document-group
  concept) that stay backward-compatible with pre-feature archives.
- New office-ingest and office-extract entry points on `Archive`, available only
  with the `office` feature.
- A spec revision proposed in `office-container/format-change-proposal.md` to be
  pasted into `FORMAT.md` FIRST (INV-1).
- CLI surface additions on `walx` (extract) and `walrus` (add) IF any are
  required (FR-OC9).

This stream does NOT touch `crates/walker`.

### Load-bearing invariants (project-conventions), enforced by review

- INV-1 FORMAT.md is the source of truth; code must not diverge. The
  document-group, envelope-recipe, version-alignment, and binary-member rules
  MUST be written into FORMAT.md (via the proposal doc) BEFORE any code lands.
- INV-2 `walrus-fmt` stays GUI-free and app-free and LEAN. A ZIP codec and
  office-format awareness are heavy and MUST live behind the new optional
  `office` feature, mirroring the existing `export-*` / `diff` / `i18n` gating,
  so a playback-only embedder (and FFWB's CR-NR-107 consumer) pays nothing.
- INV-3 Forward and reverse files are symmetric; reconstruction is byte-identical
  for the same index of a given tracked file. For an office document the headline
  guarantee is byte-identical reproduction of the WHOLE outer office file, built
  on INV-3 holding for each inner part plus a byte-exact repack recipe.
- INV-4 Splices are the single change primitive `{at, delete_len, insert}`. Text
  XML parts flow through the existing splice path unchanged; binary members do
  NOT (see FR-OC7 and its dependency on FORMAT.md section 7 open question (b)).
- INV-5 Container reader sniffs codec from magic bytes, not extension. Office
  detection likewise keys on content (ZIP local-file-header magic + inner
  signature parts) with the extension as a secondary hint, never the sole
  authority.
- Source rules: every `.rs` under 400 non-test lines; strict ASCII; no
  unwrap/expect in library code (thiserror + crate::Result); TDD (failing test
  first for all pure logic).

## 1. Cross-stream / API impact

The office feature EXTENDS the `walrus-fmt` public surface documented in
`requirements-walrus.md` section 1 without changing any existing signature.
Plain-serde group types and read accessors are always compiled; ZIP parsing,
ingest, and repack entry points are behind the optional `office` feature. The
default build links no ZIP dependency, so existing embedders remain lean.

Illustrative new surface (finalise in `design.md`):

```
// Group metadata is always compiled; byte processing is office-feature gated.

// A logical office document: a named group of member tracked files plus the
// tracked file that holds its envelope recipe.
pub struct DocumentGroup { /* id, name, kind, member_ids, envelope_id,
                              version_map */ }
pub enum OfficeKind { Odf, Ooxml }   // which packing convention applies

impl Archive {
    // Content-based ODF/OOXML detection; ZIP code is office-feature gated.
    pub fn detect_office(bytes: &[u8]) -> Result<Option<OfficeKind>>;

    // Ingest: unpack an office file's ZIP members, add each as a tracked file,
    // build the envelope recipe, and record a DocumentGroup tying them together.
    // Reuses the existing add/update storage path per member -- NOT a new
    // storage primitive. Office group histories are retained without trimming.
    pub fn add_office(&mut self, name: &str, bytes: &[u8]) -> Result<()>;

    // Extract: reconstruct every member at the document version that maps to
    // `v`, reconstruct the envelope recipe, and replay it to emit a
    // byte-identical office file.
    pub fn extract_office(&self, group: &str, v: Version) -> Result<Vec<u8>>;

    // Read-only accessors over the grouping (ALWAYS compiled -- see note).
    pub fn groups(&self) -> impl Iterator<Item = &DocumentGroup>;
    pub fn group(&self, sel: &str) -> Option<&DocumentGroup>;
}
```

Boundary note: the GROUPING DATA on the manifest (the `DocumentGroup` struct and
the `groups()` / `group()` read accessors) is plain serde data with no heavy
dependency, so it MAY be compiled in the default build; a reader that does not
understand groups still sees the raw member tracked files (backward
compatibility, FR-OC3 AC4). Only the ZIP (un)pack + envelope + office entry
points (`add_office` / `extract_office`) are gated behind `office`. The exact
split (always-compiled grouping types vs feature-gated office logic) is a
`design.md` decision mirroring how `export/mod.rs` keeps its metadata surface
always-compiled while gating the heavy backends.

Guarantees to existing consumers:
- The default `cargo build -p walrus-fmt` links NO ZIP crate and NO office code
  (proof via `cargo tree`, NFR-4 / FR-OC8).
- Every existing `requirements-walrus.md` signature is unchanged; office ingest
  is a new entry point, not a modification of `add` / `update`.
- A pre-feature archive (no groups) parses unchanged; office members, if present,
  are ordinary tracked files to a group-unaware reader.

## 2. Functional requirements

Each FR has acceptance criteria (AC) met by a failing-test-first cycle (TDD).
The pure-logic ACs -- byte-identical repack (FR-OC6), the version-alignment
mapping (FR-OC4), and the envelope round-trip (FR-OC5) -- MUST be written as a
failing test FIRST, per the testing steering rule.

### FR-OC1 Office-format detection and scope
Detect whether an input is an in-scope office container and classify it as ODF or
OOXML. Detection keys on CONTENT first (INV-5): the ZIP local-file-header magic
`50 4B 03 04` (`PK\x03\x04`), then the distinguishing member (ODF: a first
`mimetype` member whose bytes name an ODF media type; OOXML/OPC: a
`[Content_Types].xml` member). The file extension is a secondary hint only.
- AC1 A `.odt` / `.ods` / `.odp` fixture is detected as `OfficeKind::Odf`.
- AC2 A `.docx` / `.xlsx` / `.pptx` fixture is detected as `OfficeKind::Ooxml`.
- AC3 A legacy OLE2 `.doc` fixture (magic `D0 CF 11 E0`) is rejected as
  out-of-scope with a typed error naming the format, NOT misclassified as ODF or
  OOXML and NOT silently added as a single blob.
- AC4 A plain non-ZIP input (e.g. a `.txt`) is not treated as an office container
  (the caller uses the ordinary `add` path); a ZIP that is neither ODF nor OOXML
  (no `mimetype` and no `[Content_Types].xml`) is reported as an unrecognised
  container, not force-fit to a kind.

### FR-OC2 Unpack-and-add-each-member ingest
`add_office(name, bytes, ...)` unpacks the office ZIP and adds EACH inner member
as its own tracked file in the outer archive, reusing the existing per-file
storage path (`Archive::add` semantics). Text XML parts become ordinary tracked
text files; binary members are handled per FR-OC7.
- AC1 Ingesting an `.odt` with members `mimetype`, `content.xml`, `styles.xml`,
  `meta.xml`, `META-INF/manifest.xml` creates one tracked file per member, each
  with its member path recorded as (or recoverable from) the tracked `name`.
- AC2 Each TEXT XML member is stored as UTF-8 text and is independently
  reconstructable at index 0 to its original member bytes.
- AC3 Ingesting a SECOND version of the same logical document (same group name)
  records a new save on each CHANGED member via the existing net-splice path, and
  leaves UNCHANGED members at the same count (a member that did not change
  between v1 and v2 gets no new record).
- AC4 Member ingest order is deterministic (members ingested in the archive's
  recorded member order, FR-OC5), so repeated ingests of identical input produce
  identical tracked-file state.
- AC5 Ingest rejects an office ZIP whose total declared or observed
  uncompressed member data exceeds 1 GiB, using a typed size-limit error before
  reading beyond that limit.

### FR-OC3 Document-group manifest concept
Add an additive `groups` concept to the manifest tying N member tracked files
together as one logical office document, naming the document and pointing at its
envelope-recipe tracked file (FR-OC5) and carrying the version-alignment map
(FR-OC4).
- AC1 A `DocumentGroup` serde round-trips (serialize -> parse -> equal) for a
  group with several members and an envelope member.
- AC2 A manifest with NO groups (a pre-feature archive) parses unchanged and
  `groups()` yields nothing -- backward compatibility.
- AC3 `group(sel)` resolves a group by its logical name or id; a group's
  `member_ids` are the UNION of tracked TEXT member ids ever seen across all
  document versions, and its `envelope_id` references a tracked file (validated).
  Binary members are content-addressed blobs, not tracked files. The version map
  records text-member indices and separately records `envelope_index`; each
  version's envelope defines complete ZIP membership and order.
- AC4 A group-UNAWARE reader (one built without the office feature, or an older
  reader) still reads every member tracked file as a plain tracked file: the
  members are real `files/<id>/` entries, not hidden inside the group. Removing
  the group leaves the raw members intact.

### FR-OC4 Version-alignment rule (document version -> per-member index)
Define precisely how "document version k" maps to the right index of each member
tracked file, given each member has its OWN independent index range (FORMAT.md
section 5) and members change at different rates. The chosen rule (see
`design.md`) is a per-group DOCUMENT-VERSION TABLE: each saved document version
records, for every member id, the member index that was current at that document
save.
- AC1 For a group whose text member A has 3 saves and member B has 1 save,
  document version 0 (current) maps to {A:0, B:0}; an older document version
  maps each text member and the envelope to the index held at THAT document save,
  even if a member did not change. Binary payloads are selected by the hash in
  the version's envelope.
- AC2 Resolving a `Version` against a group yields a per-member index set over
  ONLY the members present at that document version. A member id ABSENT from the
  resolved version's row is NOT a ZIP member at that document version and MUST NOT
  be repacked -- there is no "earliest recorded state" fallback and no
  out-of-range index. Every index that IS present is valid in that member's own
  `[0, count]`. Per-version member ORDER is the order of the envelope's
  `zip.members[]` reconstructed at that same document version (the envelope is the
  single source of truth for both membership and order).
- AC3 The mapping is a PURE function of the group's version table and the
  requested `Version`; it has no I/O and is unit-tested directly (TDD).
- AC4 Document version 0 always resolves EVERY member CURRENTLY present to index 0
  (each member's current), consistent with the format-wide "index 0 is current"
  invariant.
- AC5 Membership change across versions is representable and resolvable. For a
  member ADDED at a later save (e.g. a `.docx` gains `word/media/image1.png` plus
  a rels part and a changed `[Content_Types].xml`), the OLDER document versions'
  rows OMIT that member (its id is simply absent from `members`), so repack of an
  older version does not emit it; the newer rows include it. For a member REMOVED
  at a later save, the NEWER rows omit it while the older rows include it.
  Resolving either case yields exactly the member set that existed at that
  document version. This is unit-tested directly (TDD) over hand-built rows.

### FR-OC5 Envelope recipe (content + storage as tracked text)
Define a per-document ENVELOPE RECIPE that captures everything needed to repack
the office ZIP byte-identically: one ordered list of all text and binary members,
their payload references, compression parameters, exact local and central
metadata, data descriptors, archive comment, and other archive-level bytes.
The ODF rule that `mimetype` is first and STORED and the OOXML
`[Content_Types].xml` convention are preserved. The recipe is stored as its OWN
tracked TEXT file (`envelope.json`) versioned alongside the parts. Unsupported
ZIP structures must be rejected with a typed error rather than reconstructed
non-identically.
- AC1 The envelope recipe serialises to deterministic text (stable key order) so
  two ingests of identical input produce identical envelope bytes.
- AC2 The envelope recipe is stored as a tracked text file and reconstructs at
  any document version via the ordinary reconstruct path.
- AC3 For two successive saves of the same document, the envelope differs only in
  the fields that genuinely change (CRC32 / sizes / timestamps of changed
  members), so the stored delta is a small net splice -- NOT a second full copy.
  (Assert that the stored record for the envelope between v1 and v2 is bounded,
  e.g. far smaller than the full envelope text.)
- AC4 The recipe records the ODF mimetype-first-and-stored rule for an ODF
  fixture and the `[Content_Types].xml` presence for an OOXML fixture.

### FR-OC6 Byte-identical repack round-trip (HEADLINE)
`extract_office(group, v)` reconstructs each member at the version-aligned index,
reconstructs the envelope recipe, and replays it to emit an office file
BYTE-IDENTICAL to the originally ingested bytes. This is Option A (preserve
byte-identical) -- see `decisions.md`.
- AC1 For a valid small `.odt` fixture under `tests/fixtures/`, `add_office` then
  `extract_office(group, Version::Index(0))` reproduces the original `.odt`
  bytes EXACTLY (`assert_eq!` on the full byte vector).
- AC2 For a valid small `.docx` fixture under `tests/fixtures/`, the same round-trip reproduces
  the original `.docx` bytes EXACTLY.
- AC3 The round-trip holds for an OLDER document version too: ingest v1, ingest
  v2 of the same document, then `extract_office(group, <v1>)` reproduces the
  original v1 bytes EXACTLY (version-alignment + envelope-at-that-version).
- AC4 Membership CHANGE round-trips byte-identically. ADD case: ingest v1 (member
  set S), ingest v2 that ADDS a member (set S + {m}); `extract_office(group,
  <v1>)` reproduces v1 EXACTLY (WITHOUT m), and `extract_office(group, <v2>)`
  reproduces v2 EXACTLY (WITH m). REMOVE case (symmetric): ingest v1 (set S +
  {m}), ingest v2 that REMOVES m (set S); extract at v1 reproduces v1 WITH m,
  extract at v2 reproduces v2 WITHOUT m. Both directions assert the full byte
  vector; this proves the version-alignment row's per-version member set drives
  repack membership and order.
- AC5 These ACs are pure logic given the stored bytes + recipe (no wall clock, no
  external files beyond `tests/fixtures/`) and MUST be driven test-first.

### FR-OC7 Binary-member handling
Embedded binary members (images, fonts, thumbnails) are binary, so they CANNOT
go through the splice path: `diff.rs` rejects NUL bytes with
`WalrusError::BinaryInput`, and `Op::Splice { insert: String }` is UTF-8 only
(confirmed in `crates/walrus/src/diff.rs` and `crates/walrus/src/record.rs`).
Define how binary members are stored and DECLARE the dependency on the FORMAT.md
section 7 open question (b) (a binary / base64 `insert` form).
- AC1 Ingesting an office document containing a binary member (e.g. a PNG in an
  `.odt`) does NOT attempt to diff that member and does NOT error out the whole
  ingest; it stores the uncompressed payload as a content-addressed blob and
  references it from the ordered version envelope.
- AC2 An UNCHANGED binary member across two document versions is stored ONCE and
  referenced from both versions (dedup by content hash), NOT re-stored per
  version.
- AC3 A CHANGED binary member is stored as a new whole blob (binary does not
  delta meaningfully); the envelope/version-alignment still reproduces the right
  blob per version.
- AC4 The requirement records that whole-blob binary VERSIONING inside a `.wchg`
  depends on resolving FORMAT.md section 7 open question (b); until that lands,
  binary members are carried as dedup-by-content attachments referenced by the
  envelope recipe (the fallback strategy), and the design states which path the
  implementation takes. This dependency is a prerequisite (section 4), not an
  afterthought.

### FR-OC8 Feature gating / dependency-boundary proof
The ZIP codec and all office logic live behind the `office` feature (INV-2).
- AC1 `cargo tree -p walrus-fmt` (default, no features) shows NONE of: the chosen
  ZIP crate, nor any new office-only dependency.
- AC2 `cargo tree -p walrus-fmt --features office` shows the ZIP crate present.
- AC3 `cargo build -p walrus-fmt` (no features) compiles with the office entry
  points absent (or, for the always-compiled grouping types, present but inert),
  and links no GUI/app crate.
- AC4 Enabling `office` adds no GUI/app dependency (no egui/eframe/ff-*): the
  feature pulls in the ZIP crate plus DEFLATE, SHA-256, and CRC-32 helpers
  required for exact repack and content addressing, asserted by `cargo tree`.

### FR-OC9 CLI surface additions
Expose office ingest/extract through the CLIs only where it does not break the
documented `walx` extract-only / `walrus` management split. Office support is a
CLI-LAYER wrapper over `add_office` / `extract_office`; it adds no new library
primitive beyond those entry points.
- AC1 `walx extract` of a group restores a byte-identical office file to disk
  under its logical name (reuses FR-OC6); extracting at `--index`/`--version`
  resolves the group's version-alignment table (FR-OC4) and still round-trips.
- AC2 `walx cat` of a group writes the byte-identical office file to stdout.
- AC3 `walrus add` detects an in-scope office file (FR-OC1) and routes it to
  `add_office` (or requires an explicit `--office` flag -- the design records
  which; detection-by-content is preferred, with the flag as an override). A
  non-office file is unaffected and uses the ordinary add path.
- AC4 CLI office behaviour is verified over fixture files (not only unit tests),
  per the testing steering rule for `walx`/`walrus`.
- AC5 Office support in BOTH CLIs is behind the `office` feature; a CLI built
  without it behaves exactly as today and reports a clear "office support not
  built" error if asked to ingest an office file, never a panic.

### FR-OC10 Size and delta expectations
Make storage behaviour verifiable without promising a general archive-size ratio,
which depends on the input ZIP compressor, document content, and container
overhead.
- AC1 Ingest does not store the complete source ZIP as an opaque blob alongside
  its tracked members. Text members and the envelope are tracked files; binary
  payloads and non-reproducible compressed streams use only their documented
  content-addressed blobs.
- AC2 On a subsequent save, only changed text members and the envelope gain
  versions; unchanged text members do not gain duplicate versions, and text
  changes use the ordinary splice history.
- AC3 An unchanged binary member is stored once and referenced from each version;
  a changed binary member is stored as a new whole blob (FR-OC7 AC2/AC3).
- AC4 Repacking uses the source compressed stream only when the recorded DEFLATE
  settings cannot reproduce it; it never retains a second opaque copy of the
  entire office ZIP.

## 3. Non-functional requirements
- NFR-1 Each new `.rs` under 400 non-test lines; keep office detection,
  compression, hashing, ZIP parsing, ingest, and repack split by concern.
- NFR-2 `cargo fmt -- --check` and `cargo clippy -- -D warnings` clean.
- NFR-3 Boundary proof: the default `cargo build -p walrus-fmt` links no ZIP /
  office / hash / checksum / DEFLATE helper crate; `cargo tree` default vs
  `--features office` asserts it (FR-OC8).
- NFR-4 Byte-identical determinism: the repack round-trip (FR-OC6) and the
  version-alignment mapping (FR-OC4) are deterministic -- no HashMap iteration
  order, no system time, no external files outside `tests/fixtures/`.
  Deterministic minimal ODF/OOXML fixtures are generated locally and live under
  `tests/fixtures/office/`; they are not captured from office applications.
- NFR-5 Full gate (`cargo gate --build`) is the owner's manual step; scoped
  `-p walrus-fmt` / `-p walx` / `-p walrus-cli` are the inner loop.
- NFR-6 ASCII-only source and docs; no unwrap/expect in library code (typed
  errors via `crate::Result` / `WalrusError`).
- NFR-7 Office ZIP processing enforces the 1 GiB aggregate uncompressed-size
  limit selected by the owner; the limit applies before allocating or reading
  member contents beyond the cap.

## 4. Prerequisites / dependencies
- P1 (SPEC-FIRST, INV-1) DONE on `feature/office-container`: FORMAT.md section 8
  defines document groups, envelope metadata, version alignment, and binary
  attachments before office implementation code. It is authoritative over the
  historical proposal text.
- P2 FORMAT.md section 7 open question (b) (a binary / base64 `insert` form) must
  be DECIDED for whole-blob binary VERSIONING inside a `.wchg`. Until it is, the
  implementation uses the dedup-by-content attachment fallback (FR-OC7 AC4);
  the design states both paths and the chosen one.
- P3 The `office` cargo feature and a lean, well-maintained pure-Rust ZIP crate
  (exact version proposed in `design.md`, verified against the workspace MSRV
  1.95 at implementation time, mirroring how the export backends pin and gate
  their crates).
- P4 The existing multi-file mutate surface (`requirements-walrus.md` FR-W1..W9,
  marked API READY on `feature/walrus-core`) -- office ingest reuses
  `Archive::add` / `update` per member and the reconstruct path per member.

## 5. Ordered delivery (buildable at each step)
Spec-first, then additive types, then ingest, then repack, then CLI, then
fixtures/tests. Keep `walrus-fmt` GUI-free throughout.

1. [DONE] SPEC: add and clarify the FORMAT.md section 8 contract from the
   proposal before implementation.
2. [DONE] TYPES: add the additive `DocumentGroup` manifest types +
   `groups()`/`group()` accessors + validation (FR-OC3). Always-compiled,
   backward-compatible; group version selection uses envelope checkpoints and
   timestamps when requested.
3. [DONE] ENVELOPE + VERSION-ALIGNMENT (pure logic, TDD first): the
   envelope-recipe schema and serde validation (FR-OC5), and version selection
   against document rows and envelope metadata (FR-OC4).
4. [DONE] ZIP CORE (behind `office`): office detection (FR-OC1), ZIP
   parsing, add-each-member, build envelope, record group (FR-OC2), binary
   attachments (FR-OC7), and byte-identical repack (FR-OC6).
5. [DONE] REPACK (behind `office`, HEADLINE TDD): reconstruct members + envelope
   and replay to byte-identical office bytes (FR-OC6).
6. [DONE] CLI: route `walx extract`/`cat` and `walrus add` through the office
   entry points behind the feature (FR-OC9).
7. [IN PROGRESS] FIXTURES / VALIDATION: deterministic minimal ODF and OOXML
   fixtures live under `tests/fixtures/office/`; CLI round trips are covered.
   Finish scoped feature-boundary and structural storage/delta checks
   (FR-OC8 / FR-OC10).

## 6. Notes on containerisation strategy (owner's thread)
The owner evaluated three ways to contain an office document's parts and chose
option 2 (per-part grouped in the outer archive). The full alternatives analysis
-- option 1 (single inner tar), option 2 (per-part grouped, CHOSEN), option 3
(nested `.wal`) -- and the Option A (byte-identical envelope, CHOSEN) vs Option B
(semantic-only identity) decision are recorded in `office-container/design.md`
(Alternatives / decision record) and summarised in
`office-container/decisions.md`. The owner's guidance: "the simplest is a
per-part grouped in outer, keeps all the features without too much cost," and "I
will accept your recommendations on this analysis." The recommendation agrees
with option 2 and with Option A.
