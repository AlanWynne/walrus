# Office container -- Technical Design

Status: DESIGN (planning, first iteration). Scope: HEAVY feature with a
format-spec impact. All code paths below are in `crates/walrus` (`walrus-fmt`)
unless noted; CLI touches are in `crates/walx` and `crates/walrus-cli`. This
design is written to be implemented without further architectural decisions; the
technology stack named here is LOCKED once approved. NO `.rs` is written and
FORMAT.md is NOT edited by this task -- the normative spec text lives in the
sibling `format-change-proposal.md` and is pasted into FORMAT.md FIRST (INV-1).

The companion documents are `requirements.md` (FR-OC1..FR-OC10),
`format-change-proposal.md` (the spec-first FORMAT.md additions), and
`decisions.md` (the ADR list).

---

## 0. Locked constants

| Constant | Value | Notes |
|----------|-------|-------|
| Cargo feature | `office` | MAY split into `container-odf` / `container-ooxml` later; one `office` gate to start. Mirrors the existing `diff` / `i18n` / `export-*` gating. |
| ZIP crate | `zip = "2"` (pure-Rust) | CHOSEN below (section 8); exact minor pinned and MSRV-verified at implementation time. |
| Workspace MSRV | 1.95 | Inherited (egui 0.36 set it); any crate forcing > 1.95 raises the WORKSPACE value and is recorded (follows the export-backend procedure). |
| Office scope | ODF + OOXML (ZIP) only | OLE2 `.doc`/`.xls`/`.ppt` deferred. |
| Identity | Option A: byte-identical repack via envelope recipe | Option B (semantic-only) rejected (section 9). |
| Containerisation | Option 2: per-part grouped in the outer archive | Options 1 and 3 rejected (section 9). |
| Envelope member | `envelope.json`, a tracked TEXT file in the group | Versioned alongside the parts. |

---

## 1. Overview

An ODF or OOXML office file is a ZIP container of XML parts plus binary resources
(images, fonts, thumbnails). Walrus already shines at versioning TEXT through its
splice engine (`crates/walrus/src/diff.rs`, `record.rs`), but it rejects binary
input (`WalrusError::BinaryInput`) and its stored insert is a UTF-8 `String`
(`Op::Splice { insert: String }`). So the design UNPACKS the office ZIP, tracks
each XML part as its own tracked text file through the EXISTING per-file storage
path (`Archive::add` / `update`, `crates/walrus/src/mutate_add.rs`), ties the
parts together with a NEW additive manifest DOCUMENT GROUP, stores a per-document
ENVELOPE RECIPE (a tracked text file) that captures exactly how the ZIP was
packed, and on extract reconstructs each part at the right version and REPLAYS
the envelope to emit a BYTE-IDENTICAL office file.

Two obligations frame every decision below:
- INV-1 (spec-first): the group, envelope, version-alignment, and binary-member
  rules are NORMATIVE additions to FORMAT.md, proposed in
  `format-change-proposal.md` and pasted into FORMAT.md before code.
- INV-2 (dependency boundary): the ZIP codec and office logic are HEAVY and live
  behind the `office` feature; the default `walrus-fmt` build links neither, so a
  playback-only embedder (and FFWB's CR-NR-107 consumer) stays lean.

The headline guarantee (FR-OC6) is byte-identical reproduction of the WHOLE outer
office file. This is NOT automatic, because ZIP packing is not canonical:
DEFLATE level/strategy, central-directory entry order, per-entry MS-DOS
timestamps, CRC32 values, "version made by" / "version needed" fields,
general-purpose bit flags, data-descriptor usage, and the ODF-specific rule that
the `mimetype` member MUST be first and STORED (uncompressed) all vary between
packers. OOXML/OPC additionally carries a `[Content_Types].xml` part and relation
parts. Reproducing the exact bytes therefore REQUIRES recording a per-member
recipe (section 4) rather than re-zipping from scratch.

Guiding principles: FORMAT.md is the source of truth; every `.rs` stays under 400
non-test lines; plain ASCII in source and docs; TDD (a failing test precedes each
pure-logic behaviour -- the repack round-trip and the version-alignment mapping
especially); stored office bytes are UNTRUSTED data (we parse the ZIP structure,
never execute or fetch anything referenced inside the document).

---

## 2. Chosen containerisation strategy (LOCKED): per-part grouped in the outer archive

DECIDED: option 2. Unpack the office ZIP and add EACH inner member as its own
tracked file in the OUTER Walrus archive, tied together by a new manifest
document-group concept. Rationale, stated as the locked decision:

- Text XML parts (`content.xml`, `styles.xml`, `meta.xml`,
  `META-INF/manifest.xml`, OOXML `word/document.xml`, etc.) get CLEAN per-part
  splice history through the existing diff engine -- this is where Walrus is
  strongest, and it is the whole point of unpacking.
- A member that did not change between two document versions costs ZERO extra
  records (its tracked file simply gains no save), so history stays compact and
  meaningful.
- The cost is bounded and ADDITIVE: a backward-compatible manifest grouping
  concept, a cross-member version-alignment rule, and a per-document envelope
  recipe. It is NOT a format rewrite -- the inner tar layout, the `.wchg`
  framing, and the splice primitive are all unchanged.

Options 1 (single inner tar) and 3 (nested `.wal`) are documented and rejected in
section 9. The owner chose option 2 ("keeps all the features without too much
cost") and the recommendation agrees.

Three new ingredients make option 2 work and are specified below:
- (a) the manifest DOCUMENT GROUP (section 3);
- (b) the per-document ENVELOPE RECIPE, a tracked text file (section 4);
- (c) the cross-member VERSION ALIGNMENT rule (section 5).

---

## 3. Manifest document-group schema (additive, backward-compatible)

A document group is a new, OPTIONAL manifest concept. It names a logical office
document, lists the member tracked-file ids, points at the envelope-recipe
tracked file, and carries the version-alignment table (section 5). It is PLAIN
serde data -- no heavy dependency -- so the types MAY be compiled in the default
build (only the ZIP/envelope LOGIC is feature-gated), and a reader that does not
understand groups still sees the raw member tracked files.

Concrete JSON sketch (a new top-level `groups` array in `manifest.json`,
alongside the existing `files` array):

```json
{
  "format": "walrus",
  "spec_version": "0.1",
  "variant": "wal",
  "base_role": "original",
  "log_direction": "forward",
  "files": [
    { "id": "f0", "name": "my-file.odt/mimetype", "mime": "text/plain", "...": "..." },
    { "id": "f1", "name": "my-file.odt/content.xml", "mime": "application/xml", "...": "..." },
    { "id": "f2", "name": "my-file.odt/styles.xml", "...": "..." },
    { "id": "f3", "name": "my-file.odt/meta.xml", "...": "..." },
    { "id": "f4", "name": "my-file.odt/META-INF/manifest.xml", "...": "..." },
    { "id": "f5", "name": "my-file.odt/envelope.json", "mime": "application/json", "...": "..." }
  ],
  "groups": [
    {
      "id": "g0",
      "name": "my-file.odt",
      "kind": "odf",
      "member_ids": ["f0", "f1", "f2", "f3", "f4"],
      "envelope_id": "f5",
      "version_map": [
        { "doc_version": 0, "members": { "f0": 0, "f1": 0, "f2": 0, "f3": 0, "f4": 0 } },
        { "doc_version": 1, "members": { "f0": 0, "f1": 1, "f2": 0, "f3": 1, "f4": 0 } }
      ]
    }
  ]
}
```

Field notes:
- `groups` is an OPTIONAL array (`#[serde(default)]`), so a manifest without it
  parses unchanged (FR-OC3 AC2). Absent means "no office documents in this
  archive".
- `id` -- a stable, archive-unique group id, minted like file ids (`g0`, `g1`,
  ...) and opaque.
- `name` -- the logical office document name as the user supplied it (e.g.
  `my-file.odt`); authoritative for the human path.
- `kind` -- `"odf"` or `"ooxml"`, selecting the packing convention the envelope
  replay applies.
- `member_ids` -- the UNION of every member tracked-file id ever seen across all
  document versions of this group, used for VALIDATION and LISTING only. It is NOT
  the per-version member set and NOT the authoritative repack order: the member
  set and order for a given document version come from that version's `version_map`
  row (section 5) and the envelope's `zip.members[]` reconstructed at that version
  (section 4). Members are listed in a stable order (first-seen order) for
  readability. The envelope file itself is NOT in `member_ids` (it is not a ZIP
  member); it is referenced separately by `envelope_id`.
- `envelope_id` -- the tracked-file id of the `envelope.json` recipe.
- `version_map` -- the version-alignment table (section 5).

Backward compatibility (FR-OC3 AC4): the members are REAL `files/<id>/` entries
with ordinary bases and `.wchg` logs. A group-unaware reader (one built without
the office feature, or an older reader) sees them as plain tracked files and can
reconstruct each one; it simply cannot repack them into an office file. The group
is pure metadata layered on top; deleting the group leaves the members intact.

Member naming: each member is stored under `name = "<group-name>/<member-path>"`
(e.g. `my-file.odt/word/document.xml`). The manifest `name` already permits `/`
(FORMAT.md section 3; confirmed in `manifest.rs` where `name` is a plain string
and the `f<N>` id is decoupled from it), so no format change is needed for the
member paths themselves -- only the `groups` array is new. The AUTHORITATIVE
member path for repack lives in the envelope (`zip.members[].path`, section 4)
and the group `name`, NOT in the derived `.wchg` member filename. A deep OOXML
member path (e.g. `my-presentation.pptx/ppt/slideLayouts/slideLayout11.xml`) can
push the derived `files/<id>/<name>.wchg` tar member path toward or past the
ustar 100-byte `name`-field limit, so office members defer UNCHANGED to the
FORMAT.md section 2.1 rule: the `<name>` in the `.wchg` member path is advisory
and SHORTENED when it would overflow, and the reader locates the log by its
`files/<id>/` directory and `.wchg` extension, never by reconstructing the full
`<group-name>/<member-path>` text. Office introduces NO new path-length concern:
repack never reads the member path from the `.wchg` filename; it reads it from
the envelope and the group. The group-relative member path is thus recorded in
the envelope as the authoritative key for repack; the tracked `name` is the
human-facing (advisory, shortenable) rendering.

Validation (FR-OC3 AC3): on `validate()`, every `member_id` and the `envelope_id`
MUST reference an existing tracked file; every `version_map` member index MUST be
within that member's `[0, count]`. These checks live in `manifest.rs` alongside
the existing per-file validation.

---

## 4. Envelope recipe schema (tracked text file)

The envelope recipe is the byte-exact packing instruction set for one office
document. It is stored as its OWN tracked TEXT file (`envelope.json`) in the
group, so it versions through the ordinary splice path: between saves it changes
only in a handful of fields (changed members' CRC32 / sizes / timestamps), so the
net splice is tiny and the outer zstd compresses it again (FR-OC5 AC3).

Concrete JSON sketch (the reconstructed CONTENT of the `envelope.json` tracked
file at one document version):

```json
{
  "schema": "walrus-office-envelope",
  "schema_version": "0.1",
  "kind": "odf",
  "zip": {
    "comment": "",
    "members": [
      {
        "path": "mimetype",
        "member_id": "f0",
        "method": "store",
        "mimetype_first_stored": true,
        "crc32": "2b4c1f3a",
        "uncompressed_size": 39,
        "compressed_size": 39,
        "dos_time": "2026-10-08T12:00:00",
        "gp_flags": 0,
        "version_made_by": 20,
        "version_needed": 10,
        "external_attrs": 0,
        "internal_attrs": 0,
        "data_descriptor": false
      },
      {
        "path": "content.xml",
        "member_id": "f1",
        "method": "deflate",
        "deflate_level": 6,
        "crc32": "9f3c22ab",
        "uncompressed_size": 10482,
        "compressed_size": 2277,
        "dos_time": "2026-10-08T12:00:00",
        "gp_flags": 0,
        "version_made_by": 20,
        "version_needed": 20,
        "external_attrs": 0,
        "internal_attrs": 0,
        "data_descriptor": false
      }
    ]
  },
  "binary_members": [
    {
      "path": "Pictures/img1.png",
      "content_sha256": "ab12...",
      "method": "deflate",
      "deflate_level": 6,
      "crc32": "7d9e01cc",
      "uncompressed_size": 48213,
      "compressed_size": 47980,
      "dos_time": "2026-10-08T12:00:00",
      "stored_compressed": true
    }
  ]
}
```

Note on the blob reference: a binary member is identified SOLELY by
`content_sha256`; the stored blob lives at the member path `blobs/<content_sha256>`
in the tar (section 6, matching `format-change-proposal.md` section 8.5). There is
NO separate `store_ref` field -- the blob path is DERIVED from `content_sha256`
(`blobs/` + the hash), so a distinct reference field would be redundant and is
omitted. A binary member carries the same ZIP header fields as a text member
(`method`, `crc32`, sizes, `dos_time`, and `stored_compressed` when its DEFLATE
cannot be reproduced) so repack writes it byte-identically; its BYTES, however,
come from the content-addressed blob rather than a reconstructed text part.

Field notes and the byte-identical obligation:
- `members` is in the EXACT order the members appear in the ZIP (both local file
  headers and the central directory order are reproduced from this list). ODF
  REQUIRES `mimetype` FIRST and STORED (uncompressed); `mimetype_first_stored`
  and `method: "store"` capture that. OOXML has no mimetype member but carries
  `[Content_Types].xml`; `kind: "ooxml"` plus the member list reproduce it.
- `method` -- `"store"` or `"deflate"`. For `deflate`, `deflate_level` (and, if
  the chosen ZIP crate exposes it, strategy) is recorded. Byte-identical repack of
  a DEFLATE member REQUIRES reproducing the original compressed stream bit-for-bit;
  a `store` fallback does NOT preserve byte-identity for a member the original ZIP
  compressed, because emitting it STORED changes the compression-method field in
  both the local header and the central directory, the compressed bytes, and
  `compressed_size` -- that is Option B (semantic identity), which this design
  rejects (section 9). So the rule is:
  - At INGEST, for every `deflate` member, the writer MUST VERIFY it can reproduce
    the original compressed bytes exactly (re-DEFLATE at the recorded level and
    compare to the member's original compressed stream). If verification SUCCEEDS,
    repack re-DEFLATEs and the result is byte-identical.
  - If verification FAILS (the toolchain's DEFLATE does not reproduce the original
    stream), the writer MUST NOT silently fall back to `store`. It MUST either
    store the ORIGINAL compressed member stream VERBATIM (see
    `stored_compressed` below) so repack is a pure byte copy, or FAIL `add_office`
    with a typed `WalrusError::OfficeNonReproducible` error naming the member and
    the toolchain. The headline byte-identical AC (FR-OC6 AC1/AC2) is thus never
    silently downgraded.
  - A `store` method entry is valid ONLY for a member the ORIGINAL ZIP already
    stored uncompressed (e.g. the ODF `mimetype`).
- `stored_compressed` -- an OPTIONAL boolean. When `true`, the member's tracked
  file holds the member's ORIGINAL COMPRESSED bytes verbatim (not the decompressed
  XML), and repack copies them without re-compressing. This is the exact-but-
  opaque path for a `deflate` member whose stream the toolchain cannot reproduce;
  it trades the per-part splice-diff benefit (FR-OC10) for guaranteed byte-
  identity. It is the fallback of last resort: text members whose DEFLATE
  verifies use the decompressed-XML storage (the normal, diff-friendly path), and
  only an unverifiable member is stored compressed. The design PREFERS re-DEFLATE
  verification succeeding (the common case for a single pinned toolchain) and uses
  `stored_compressed` only where it does not.
- `crc32`, `uncompressed_size`, `compressed_size`, `dos_time`, `gp_flags`,
  `version_made_by`, `version_needed`, `external_attrs`, `internal_attrs`,
  `data_descriptor` -- the ZIP header fields that must be reproduced to get the
  same bytes. These are recorded as data; the repack writes them verbatim rather
  than letting the ZIP crate recompute them.
- `binary_members` -- see section 6. Binary members are referenced by content
  hash (dedup) rather than carrying their bytes inline in the envelope.
- Determinism (FR-OC5 AC1): the envelope is serialised with STABLE key order and
  no wall-clock-at-serialise field, so two ingests of identical input produce
  identical envelope bytes.

The envelope's `member_id` cross-references the manifest group `member_ids`. On
extract, the repack reads the envelope member list, reconstructs each referenced
member tracked file at the version-aligned index (section 5), and writes the ZIP
member-by-member using the recorded header fields.

---

## 5. Version-alignment model (LOCKED): per-group document-version table

PROBLEM: each member tracked file has its OWN independent index range (FORMAT.md
section 5; confirmed in `manifest.rs` -- every `TrackedFile` has its own
`versions.count`). Members change at different rates: saving a document might add
a record to `content.xml` and the envelope but leave `styles.xml` untouched. So
"document version k" does NOT map to "index k of every member".

DECISION: a per-group DOCUMENT-VERSION TABLE (`version_map` in the group entry,
section 3). Each row maps a `doc_version` to the set of member indices that were
current at THAT document save, OVER ONLY THE MEMBERS PRESENT AT THAT VERSION:

```
version_map[doc_version] = { member_id -> member_index_current_at_that_save }
                           // keys are EXACTLY the members present at this version
```

The per-version member set is thus the KEY SET of the row (equivalently, the set
of members the envelope reconstructed at that version lists in `zip.members[]`,
section 4). The envelope -- itself a per-version tracked text file -- is the
single source of truth for both MEMBERSHIP and ORDER; the `version_map` row
agrees with it on membership and supplies each present member's index. A member
whose id is ABSENT from the row is NOT a ZIP member at that document version.

Rules:
- Document version 0 (current) maps every member CURRENTLY present to index 0
  (each member's current), consistent with the format-wide "index 0 is current"
  invariant (FR-OC4 AC4).
- On each office save (`add_office` of a new version of an existing group), the
  ingest records a NEW `version_map` row whose keys are the members present in
  THAT save. For each present member it stores the member index that this save
  produced (0 for a member that changed this save, or the index that member's
  current slid to -- because adding a new save to a member pushes older versions
  to higher indices, FORMAT.md section 5 / `mutate_add.rs` `rederive_versions`).
  A member that did NOT change this save keeps its mapping pointing at the same
  logical bytes, which have slid to a higher index iff the member changed in a
  LATER save -- the table records the concrete index per save so no arithmetic is
  needed at read time.
- Resolving a `Version` against a group (FR-OC4 AC2): map the requested document
  version to a `version_map` row, then reconstruct ONLY the members whose ids are
  keys in that row, each at its recorded `members[member_id]` index, in the order
  the envelope reconstructed at that version lists them. A member id ABSENT from
  the row is not reconstructed and MUST NOT be repacked; there is no "earliest
  recorded state" fallback and no out-of-range index.
- MEMBERSHIP CHANGE (FR-OC4 AC5, FR-OC6 AC4) is represented by membership of the
  row, not by a sentinel value:
  - A member ADDED at a later save first appears as a key in the row for the save
    that added it; earlier rows OMIT its id, so repack of an earlier document
    version does not emit it. (Real OOXML example: a save that inserts an image
    adds `word/media/image1.png` and a rels part and changes
    `[Content_Types].xml`; the pre-image rows have none of the genuinely-new ids,
    while the changed `[Content_Types].xml` is an ordinary content change on an
    always-present member.)
  - A member REMOVED at a later save stops appearing as a key from the removing
    save's row onward; earlier rows still carry it, so repack of an earlier
    version still emits it.
  The group's flat `member_ids` is the UNION of all ids ever seen (section 3) and
  is used only for validation/listing; it is NEVER consulted to decide a version's
  membership.
- The mapping is a PURE function of the group's `version_map` and the requested
  `Version` -- no I/O -- so it is unit-tested directly, test-first (FR-OC4 AC3,
  including the ADD and REMOVE cases).

WHY a stored table rather than computed arithmetic: because members slide at
different rates, deriving a member's index for an older document version from
counts alone requires replaying the whole save history per member. Storing the
concrete per-save index set is O(members) per document version, is unambiguous,
survives generations/condense re-base on individual members (the table is
updated when a member is re-based, mirroring the manifest `rebase` audit), and is
trivially testable. The table lives in the group entry so it is one serde object,
versioned with the manifest.

ALTERNATIVE considered: tag each member save with a shared "document generation"
counter and resolve by scanning. Rejected: it pushes resolution logic into a scan
over every member's `.wchg` and couples member re-base to a global counter;
the explicit table is simpler and directly testable.

---

## 6. Binary-member handling

Binary members (images, fonts, thumbnails) cannot use the splice path:
`crates/walrus/src/diff.rs` `reject_binary()` returns `WalrusError::BinaryInput`
on a NUL byte, and `crates/walrus/src/record.rs` `Op::Splice.insert` is a UTF-8
`String`. Two strategies, with a recommendation:

(i) WHOLE-BLOB VERSIONED MEMBERS (natural fit, DEPENDS on section-7 (b)). Store
each binary member as a tracked "file" whose versions are whole blobs. This needs
a binary / base64 `insert` form in the `.wchg` -- i.e. FORMAT.md section 7 open
question (b) resolved. Until that lands, this path is not available.

(ii) DEDUP-BY-CONTENT ATTACHMENTS carried by the envelope (FALLBACK, available
today). Store each distinct binary member body ONCE, keyed by its content hash
(`content_sha256`), as an opaque blob member at the tar path `blobs/<content_sha256>`
(the proposal doc section 8.5 fixes this spelling). The envelope's
`binary_members[]` references the blob by `content_sha256` per document version;
the blob path is derived from the hash, so no separate reference field is stored
(see section 4). An UNCHANGED binary across versions points at the SAME hash
(stored once, FR-OC7 AC2); a CHANGED binary is a new hash / new blob (FR-OC7 AC3).
A binary member's original compressed stream is preserved verbatim via
`stored_compressed` (section 4) when its DEFLATE cannot be reproduced, so repack
stays byte-identical for binary members too.

RECOMMENDATION: implement strategy (ii) FIRST, because it needs no format change
to the splice record and delivers the dedup-across-versions win immediately. Keep
(i) as a later enhancement once section-7 (b) is decided; at that point a binary
member MAY migrate to whole-blob versioned storage for a cleaner single model.
The design records this dependency explicitly (FR-OC7 AC4): the implementation
takes path (ii); path (i) is gated on the spec decision. Either way, ingest NEVER
attempts to diff a binary member and NEVER aborts the whole ingest on encountering
one (FR-OC7 AC1).

Encoding is a non-issue for the TEXT parts: ODF/OOXML inner XML is UTF-8 already,
so the parts flow straight through the UTF-8 storage path; the existing
`encoding_rs` path (`crates/walrus/src/encoding.rs`, `i18n` feature) covers any
legacy-encoded text part if one ever appears.

---

## 7. Byte-identical repack procedure (step by step)

`extract_office(group, v)` produces the office file bytes:

1. Resolve `group` (by name or id) to its `DocumentGroup`.
2. Resolve `v` to a `doc_version` and look up the `version_map` row (section 5),
   yielding `{member_id -> index}` over EXACTLY the members present at that
   document version. Member ids absent from the row are not present at this
   version and are not repacked.
3. Reconstruct the `envelope.json` tracked file at the envelope's aligned index
   (the envelope is itself tracked and carries its own `version_map` entry) and
   parse it. Its `zip.members[]` is the AUTHORITATIVE per-version membership and
   ORDER; it agrees member-for-member with the row's key set from step 2 (a
   consistency check: if they diverge, fail with a typed error rather than guess).
4. For each `member` in the envelope's ordered `members[]`: reconstruct that
   member tracked file at its aligned index (from the step-2 row) -> the member's
   uncompressed bytes.
5. For each `binary_member`: resolve the content hash to its stored blob ->
   the member's bytes.
6. Write a ZIP stream member-by-member in the envelope's recorded order, using
   the recorded header fields VERBATIM (method, CRC32, sizes, dos_time, flags,
   version fields, external/internal attrs, data-descriptor usage). For ODF,
   write `mimetype` FIRST and STORED. For each `deflate` member whose DEFLATE was
   VERIFIED reproducible at ingest (section 4), re-DEFLATE the reconstructed XML
   at the recorded level -- this reproduces the original compressed bytes exactly.
   For a member marked `stored_compressed` (ingest could not reproduce its DEFLATE),
   copy its stored original compressed bytes VERBATIM, writing the recorded
   `deflate` method and sizes so the member is byte-identical. A member whose
   method is `store` is written uncompressed as recorded. The writer NEVER
   substitutes `store` for an originally-DEFLATEd member -- that path does not
   exist (section 4); byte-identity is preserved in every case.
7. Write the central directory in the recorded order with the recorded fields,
   then the end-of-central-directory record with the recorded comment.
8. Return the assembled bytes. For the ORIGINAL ingested version this equals the
   input office file byte-for-byte (FR-OC6 AC1/AC2); for an older version it
   equals that version's original bytes (FR-OC6 AC3).

The repack writes header fields from the recipe rather than letting the ZIP crate
recompute them, which is why a lean ZIP crate that exposes raw member writing
(section 8) is required.

---

## 8. Module decomposition + cargo feature / dependency plan

New files under `crates/walrus/src/office/`, each well under 400 non-test lines,
ALL behind `#[cfg(feature = "office")]` except the always-compiled grouping types
in `manifest.rs`:

| File | Responsibility |
|------|----------------|
| `office/mod.rs` | Re-exports; `OfficeKind`; the `add_office` / `extract_office` entry points on `Archive` (thin -- delegate to the modules below). No logic beyond dispatch. |
| `office/detect.rs` | Content-based detection (FR-OC1): ZIP magic, ODF `mimetype`-first sniff, OOXML `[Content_Types].xml` sniff, OLE2 reject. Pure, unit-testable. |
| `office/zip.rs` | Thin wrapper over the chosen ZIP crate: unpack members (path, bytes, raw header fields) on ingest; write members with verbatim header fields on repack. |
| `office/envelope.rs` | The envelope-recipe type + serde (section 4), deterministic serialisation, and the parse/build helpers. Pure, unit-testable. |
| `office/odf.rs` | ODF specifics: the mimetype-first-and-stored rule, ODF media-type table, ODF member conventions. |
| `office/ooxml.rs` | OOXML/OPC specifics: `[Content_Types].xml`, relation parts, OOXML member conventions. |
| `office/ingest.rs` | Orchestrates `add_office`: detect -> unpack -> add each member via the existing `Archive::add`/`update` -> build envelope -> record/extend the group + version_map -> handle binary members (section 6). |
| `office/repack.rs` | Orchestrates `extract_office`: the section-7 procedure. |
| `office/align.rs` | The version-alignment mapping (section 5). Pure, unit-tested FIRST. |

Grouping TYPES (`DocumentGroup`, `OfficeKind` serde, `groups()`/`group()`
accessors, group validation) live in `crates/walrus/src/manifest.rs` (or a small
`manifest_group.rs` sibling if `manifest.rs` nears the 400-line cap) and are
ALWAYS compiled -- they are plain serde data with no heavy dependency, so a
default build still parses an office archive's manifest and sees the raw members
(backward compatibility). Only the ZIP/envelope/ingest/repack LOGIC is gated.

Cargo plan (`crates/walrus/Cargo.toml`), mirroring the export-backend pattern:

```toml
[dependencies]
zip = { version = "2", optional = true, default-features = false, features = ["deflate"] }

[features]
office = ["dep:zip"]
# (MAY later split: container-odf / container-ooxml, both -> dep:zip)
```

ZIP crate choice (LOCKED pending MSRV check): `zip = "2"` (the `zip-rs`
`zip` crate), pure-Rust, actively maintained, exposes per-member method / raw
header control sufficient for verbatim repack, and `default-features = false`
keeps its own optional codecs out unless named. The EXACT minor and its MSRV MUST
be verified against the workspace MSRV 1.95 at implementation time (as the export
backends do); if `zip 2.x` requires > 1.95, either pin the newest `zip` that
builds on 1.95 or raise the WORKSPACE `rust-version` and record the forcing crate
in `ROADMAP.md` and a `Cargo.toml` comment (the established procedure). If `zip`
cannot reproduce byte-exact DEFLATE for the recorded level across toolchains
(section 10), the fallback is to store the member's ORIGINAL COMPRESSED bytes
verbatim (`stored_compressed`, section 4) so repack is a pure byte copy -- NOT to
re-emit an originally-DEFLATEd member as `store`, which would break byte-identity.
This keeps the headline byte-identical AC intact at a size cost on the
unverifiable member.

---

## 9. Alternatives / decision record

### Containerisation strategies

Option 1 -- SINGLE INNER TAR. Unpack the office ZIP, pack its members into one
uncompressed inner tar, and track THAT single tar blob as one tracked file.
- Pros: simplest manifest (one tracked file, one index space, grouping implicit).
- Cons: loses per-part diffing. A tar interleaves 512-byte headers and NUL
  padding, so the blob is binary and `diff.rs` rejects it (NUL bytes); even if it
  did not, editing one member shifts every later byte offset, so a one-line change
  produces a huge, meaningless delta. Weakest for history quality -- it throws
  away the exact strength (clean per-part text history) that unpacking exists to
  gain. REJECTED.

Option 2 -- PER-PART GROUPED IN THE OUTER ARCHIVE (CHOSEN, section 2). Each inner
member is its own tracked file in the outer archive, tied by a new manifest group
with a version-alignment table and a per-document envelope recipe.
- Pros: clean per-part splice history for the XML; unchanged members cost nothing;
  additive and backward-compatible; reuses the existing per-file storage and
  reconstruct paths; byte-identical repack via the envelope.
- Cons: needs the three new ingredients (group, envelope, alignment). Bounded,
  additive, NOT a format rewrite. CHOSEN -- owner's pick and the recommendation.

Option 3 -- NESTED `.wal`. Make each office document its OWN inner `.wal` archive
(parts tracked inside it with full per-part history and its own manifest), stored
as a single opaque tracked member in the outer `.wal`.
- Pros: conceptually clean -- the group IS the inner archive; reuses the whole
  format recursively; per-part diffing lives inside the inner archive.
- Cons: the OUTER level cannot delta a COMPRESSED inner archive (it is an opaque
  binary blob -> needs binary-member handling at the outer level), so outer-level
  DOCUMENT versioning is not possible; and double compression (inner zstd then
  outer zstd) must be avoided by a special "store nested archives without outer
  re-compression" rule. More moving parts for less outer-level capability.
  REJECTED.

### Identity: Option A vs Option B

Option A -- PRESERVE BYTE-IDENTICAL (CHOSEN). Record a per-member envelope recipe
so repack reproduces the original office file byte-for-byte. For a DEFLATE member,
byte-identity requires reproducing the compressed stream exactly: the writer
VERIFIES re-DEFLATE at ingest and, when it cannot, stores the original compressed
bytes verbatim (`stored_compressed`, section 4) or FAILS -- it NEVER re-emits an
originally-compressed member as STORED (that would be Option B). This keeps the
headline INV-3 / FORMAT.md section-5 byte-identical guarantee intact for the whole
office file, which is what makes Walrus trustworthy for round-tripping documents
other tools will open. Cost: the envelope recipe (small, text, diffs cheaply),
plus a size cost on any member whose DEFLATE the toolchain cannot reproduce.

Option B -- SEMANTIC-ONLY IDENTITY (REJECTED). Re-zip from the parts on extract,
accepting that the bytes differ (same logical document, different ZIP packing).
This WEAKENS INV-3 / section 5 from "byte-identical" to "semantically
equivalent", which is a format-level downgrade and breaks the trust property.
REJECTED; Option A is chosen.

---

## 10. Risks / open questions

- NON-DETERMINISTIC DEFLATE. Byte-identical repack of a `deflate` member requires
  re-DEFLATE at the recorded level to reproduce the original compressed stream
  bit-for-bit. Different zlib/miniz builds can produce different bytes for the
  same level. Mitigation (section 4, NOT a `store` substitution): the writer
  VERIFIES at ingest that re-DEFLATE reproduces the member's original compressed
  bytes; if it does, repack re-DEFLATEs exactly; if it does NOT, the writer stores
  the member's ORIGINAL COMPRESSED bytes verbatim (`stored_compressed: true`) so
  repack is a pure byte copy, or FAILS `add_office` with a typed
  `WalrusError::OfficeNonReproducible` error -- it NEVER falls back to `store` and
  claims success, because emitting an originally-DEFLATEd member as STORED changes
  the method field, compressed bytes, and `compressed_size` and would silently
  downgrade to Option-B semantic identity (rejected, section 9). The
  `stored_compressed` fallback keeps FR-OC6 byte-identity intact at a size cost
  and the loss of that member's splice-diff benefit. The fixtures (section 11)
  pin which path each member takes at implementation time. This is the primary
  risk to FR-OC6 and implementation verifies it against the chosen ZIP crate
  FIRST.
- OOXML vs ODF packing differences. ODF mandates `mimetype` first + stored; OOXML
  has `[Content_Types].xml` and relation parts and no mimetype member. The
  envelope's ordered member list + `kind` cover both, but each needs its own
  fixture (section 11) and its own `odf.rs` / `ooxml.rs` conventions.
- Binary-member VERSIONING depends on FORMAT.md section 7 open question (b)
  (section 6). The implementation ships the dedup-by-content attachment fallback
  (strategy ii); whole-blob versioned members (strategy i) wait on the spec
  decision.
- OLE2 legacy formats (`.doc`/`.xls`/`.ppt`) are explicitly DEFERRED (FR-OC1 AC3).
  They are compound binary, not ZIP; a future stream may add an OLE2 reader. Not
  in this design.
- Data descriptors / streamed ZIPs. Some packers write sizes/CRC in a trailing
  data descriptor rather than the local header. The envelope records
  `data_descriptor` per member so repack reproduces the chosen form; the fixtures
  must include at least one such member if a target packer uses it.
- Encrypted or password-protected office files are out of scope (not detected as
  in-scope; rejected like any unrecognised container).

---

## 11. Testing strategy

Per the testing steering rule, the pure-logic ACs are TDD'd (failing test first):

- VERSION ALIGNMENT (`office/align.rs`, FR-OC4): pure unit tests over hand-built
  `version_map` tables -- doc version 0 maps all present members to 0; an older
  doc version maps an unchanged member to its then-current index; a member id
  absent from a row resolves to "not present" (not an index), with repack
  skipping it; the ADD case (older rows omit a member added later) and the
  symmetric REMOVE case (newer rows omit a member removed later) resolve to
  exactly the member set that existed at that version; out-of-range is a typed
  error. Written FIRST, watched fail, then implemented.
- ENVELOPE ROUND-TRIP (`office/envelope.rs`, FR-OC5): build -> serialise -> parse
  -> equal; deterministic byte-stable serialisation; the two-save delta is a
  small net splice (assert the stored record size is bounded).
- BYTE-IDENTICAL REPACK (FR-OC6, HEADLINE): integration tests in
  `crates/walrus/tests/office_roundtrip.rs` over REAL small captured fixtures
  under `crates/walrus/tests/fixtures/` -- a minimal `.odt` and a minimal
  `.docx`. `add_office` then `extract_office(Index(0))` must `assert_eq!` the full
  byte vector against the original; a two-version ingest then extract at the older
  version must reproduce the older original (FR-OC6 AC3). The MEMBERSHIP-CHANGE
  round-trip (FR-OC6 AC4): ingest v1, ingest v2 that ADDS a member, then extract
  v1 (reproduces v1 WITHOUT the added member) and v2 (reproduces v2 WITH it), both
  byte-identical; and the symmetric REMOVE case. Deterministic: no wall clock,
  fixtures only under `tests/fixtures/`.
- DETECTION (`office/detect.rs`, FR-OC1): ODF/OOXML/OLE2/plain-ZIP fixtures
  classify correctly; OLE2 is a typed out-of-scope error.
- BINARY MEMBERS (FR-OC7): an `.odt` fixture with an embedded PNG ingests without
  diffing the PNG; an unchanged PNG across two versions is stored once (assert one
  blob); a changed PNG is a new blob.
- BOUNDARY PROOF (FR-OC8 / NFR-3): `cargo tree -p walrus-fmt` (default) shows no
  `zip`; `cargo tree -p walrus-fmt --features office` shows `zip`; neither shows a
  GUI/app crate. Captured as the gate's dependency-boundary check.
- SIZE / DELTA (FR-OC10): assert a single stored version is <= the original office
  file size, and a two-version archive is far smaller than two full office files.
- CLI (FR-OC9): `walx extract` / `cat` and `walrus add` office behaviour verified
  over fixtures (not only unit tests), behind the `office` feature.

GUI: none -- this feature is library + CLI only, no `walker` touch, so no
`egui_kittest` harness applies.

---

## 12. Review responses

Iteration 3 -- responses to `office-container/review.json` (verdict
CHANGES_REQUESTED) and the reasoning in `office-container/review.md`.

- MEDIUM-1 (version alignment does not define how a member ADDED or REMOVED
  between document versions is represented or resolved) -- ADDRESSED, using the
  review's PREFERRED representation. The vague "resolves to its earliest recorded
  state, defined by the rule" wording is REMOVED from all three documents and
  replaced by a concrete rule: a `version_map` row's `members` object contains an
  entry for EXACTLY the members present at that document version; a member id
  ABSENT from the row is NOT a ZIP member at that version and MUST NOT be
  repacked (no sentinel, no "absent" marker, no out-of-range index). Per-version
  member ORDER is the order of the envelope's `zip.members[]` reconstructed at
  that same version, making the envelope the single source of truth for both
  membership and order. The group's flat `member_ids` is now documented as the
  UNION of all members ever seen, used only for validation/listing and never for
  per-version repack. The ADD case (older rows omit a member added later) and the
  symmetric REMOVE case (newer rows omit a member removed later) are specified
  explicitly. Fixed in: requirements.md FR-OC3 AC3 (`member_ids` = union),
  FR-OC4 AC2/AC4 + new AC5 (membership change), FR-OC6 new AC4 (ADD/REMOVE
  byte-identical round-trip, with the pure-logic note sliding to AC5); design.md
  section 3 (`member_ids` = union), section 5 (row = members-present-only + ADD/
  REMOVE rules), section 7 step 2-4 (repack iterates the per-version row/envelope,
  consistency check), section 11 (version-alignment and repack ADD/REMOVE tests);
  format-change-proposal.md section 8.1 (`member_ids` = union), section 8.4 (row
  key-set = per-version membership + ADD/REMOVE MUST rules), section 8.7
  (conformance: reconstruct only members present in the row, validate union
  superset). An AC covering a two-version ADD round-trip (v1 repacks without the
  added member, v2 with it, both byte-identical) and the symmetric REMOVE case is
  added (FR-OC6 AC4) and called out as TDD pure logic (FR-OC4 AC5).

Iteration 2 -- responses to `office-container/review.json` (verdict
CHANGES_REQUESTED) and the reasoning in `office-container/review.md`.

- HIGH-1 (DEFLATE-fallback-to-`store` contradicts byte-identity) -- ADDRESSED.
  The "record a DEFLATE member as `store` when re-DEFLATE cannot be guaranteed"
  rule is REMOVED from all four documents; it was Option-B semantic identity in
  disguise. The corrected rule (preferred fix from the review): at ingest the
  writer VERIFIES that re-DEFLATE at the recorded level reproduces each `deflate`
  member's original compressed bytes bit-for-bit; if it succeeds, repack
  re-DEFLATEs exactly; if it fails, the writer either stores the member's
  ORIGINAL COMPRESSED bytes verbatim via a new OPTIONAL `stored_compressed` flag
  (repack is then a pure byte copy) or FAILS `add_office` with a typed
  `WalrusError::OfficeNonReproducible` error. `"store"` is valid ONLY for a member
  the original ZIP already stored. Fixed in design.md sections 4 (member
  `method` + new `stored_compressed` field), 7 (step 6), 8 (cargo-plan trailing
  note), 9 (Option A description), 10 (first risk bullet);
  format-change-proposal.md section 8.2 (member rules + `stored_compressed`);
  decisions.md ADR-2. FR-OC6 AC1/AC2 (full-byte `assert_eq!`) now holds in every
  success case.
- MEDIUM-1 (member `.wchg` path vs ustar 100-byte limit) -- ADDRESSED. Added a
  statement to design.md section 3 ("Member naming") and
  format-change-proposal.md section 8.1 that the authoritative member path lives
  in the envelope (`zip.members[].path`) and the group `name`, NOT in the derived
  `.wchg` filename, so the FORMAT.md section 2.1 advisory-`<name>` shortening rule
  applies unchanged to office members and no new path-length concern is
  introduced. Verified against FORMAT.md section 2.1 (ustar `name` <= 100 bytes;
  reader locates the log by directory + extension).
- NIT-1 (`kind` validation phrasing could permit a silent skip) -- ADDRESSED.
  format-change-proposal.md section 8.1 now states a reader asked to repack an
  unknown `kind` MUST reject with a typed unknown-kind error (it MUST NOT silently
  no-op or force-fit), aligning with FR-OC1 AC4.
- NIT-2 (`store_ref` vs `content_sha256` redundancy; `blob/` vs `blobs/`
  spelling) -- ADDRESSED. The `store_ref` field is DROPPED everywhere; a binary
  member is identified solely by `content_sha256`, and the blob path is DERIVED as
  `blobs/<content_sha256>` (one consistent spelling). Fixed in design.md sections
  4 and 6, and format-change-proposal.md sections 8.2 and 8.5.
- NIT-3 (`record.rs` module doc says `log.jsonl`, FORMAT.md says `.wchg`) --
  BACKLOGGED, by rule. This finding is in `crates/walrus/src/record.rs`, a `.rs`
  source file; the review itself notes it "does not affect this verdict" and is
  for "whoever next touches `record.rs`". This task's ABSOLUTE CONSTRAINT forbids
  editing any `.rs` file, so the fix is deferred to the implementation phase: when
  the office implementer next touches `record.rs`, update its module doc from
  `log.jsonl` to `.wchg`. Recorded here so it is not lost.
