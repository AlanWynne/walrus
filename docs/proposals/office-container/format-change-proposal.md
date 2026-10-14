# FORMAT.md change proposal -- office document groups, envelope recipe, version alignment, binary members

Status: PROPOSAL (spec-first, INV-1). This document is NOT FORMAT.md; it is the
normative text to be PASTED INTO FORMAT.md during the implementation phase,
BEFORE any office code lands. It is written in FORMAT.md's existing voice
(normative MUST / SHOULD / MAY) with proposed new section numbers so it drops in
cleanly. Do NOT edit FORMAT.md as part of the design task -- only produce this
proposal.

ASCII only (`->`, `<->`, `<=`, `>=`, `--`) per the documentation steering rule.

Companion documents: `requirements.md`, `design.md`, `decisions.md`.

## Why these additions are ADDITIVE and BACKWARD-COMPATIBLE

Every addition below is OPTIONAL in the manifest and layered on top of the
existing multi-file model. The guarantees that make it safe for older readers:

- An archive with NO document groups conforms exactly as today; the new `groups`
  array defaults to empty and absent.
- An office document's members are REAL tracked files (`files/<id>/base` +
  `files/<id>/<name>.wchg`), so a reader that does not understand groups still
  reads and reconstructs each member as a plain tracked file. It cannot REPACK
  the office file, but it never mis-reads or corrupts the archive -- it degrades
  SAFELY, exactly like the codec-forward-compatibility posture in FORMAT.md
  section 2.
- The spec version stays `0.1` (a draft, changes allowed pre-1.0). These are new
  OPTIONAL sections, not a breaking change to the existing sections 1-6.

The following is the proposed FORMAT.md text.

---

## 8. Document groups (office containers)

A Walrus archive MAY bundle an OFFICE DOCUMENT -- an OpenDocument Format (ODF)
file (`.odt`, `.ods`, `.odp`, and the ODF family) or an Office Open XML (OOXML /
OPC) file (`.docx`, `.xlsx`, `.pptx`) -- by UNPACKING its inner ZIP members and
tracking each member as its own tracked file (section 2.1), tied together by a
DOCUMENT GROUP. Legacy OLE2 / CFBF binary office files (`.doc`, `.xls`, `.ppt`)
are NOT ZIP containers and are OUT OF SCOPE for this mechanism.

A document group is OPTIONAL metadata in `manifest.json`. An archive with no
document groups conforms exactly as a plain multi-file archive. A reader that
does not implement document groups MUST still read every member as an ordinary
tracked file (it simply cannot repack the office file); it MUST NOT reject the
archive on account of an unknown `groups` array and SHOULD ignore unknown group
fields, as elsewhere in this spec.

### 8.1 The `groups` manifest array

`manifest.json` MAY carry a top-level `groups` array. When absent or empty, the
archive has no document groups. Each element describes one logical office
document:

```json
{
  "id": "g0",
  "name": "my-file.odt",
  "kind": "odf",
  "member_ids": ["f0", "f1", "f2", "f3", "f4"],
  "envelope_id": "f5",
  "version_map": [
    { "doc_version": 0, "members": { "f0": 0, "f1": 0, "f2": 0, "f3": 0, "f4": 0 } }
  ]
}
```

Field rules:
- `id` -- a stable, archive-unique, opaque group identifier. A writer MUST keep
  it unique for the life of the group and SHOULD mint sequential `g0`, `g1`, ...
  ids. A reader MUST treat it as opaque.
- `name` -- the logical office document name and relative path as the user
  supplied it (may contain `/`). Authoritative for the human path.
- `kind` -- MUST be `"odf"` or `"ooxml"`. It selects which packing convention
  (section 8.3) the envelope replay applies. A reader asked to REPACK a group
  whose `kind` it does not recognise MUST reject with a typed unknown-kind error
  (it MUST NOT silently no-op or force-fit a convention), but it MAY still read
  the member files as ordinary tracked files.
- `member_ids` -- the ids of the tracked files that are the unpacked ZIP members.
  This is the UNION of every member ever present across all document versions of
  the group, used for VALIDATION and LISTING only. It is NOT the per-version
  member set and NOT the authoritative repack order: the member set and ORDER for
  a given document version come from that version's `version_map` row
  (section 8.4) and the envelope's `zip.members[]` reconstructed at that version
  (section 8.2). Every id MUST reference an existing tracked file in the `files`
  array. The envelope-recipe tracked file is NOT listed here (it is not a ZIP
  member).
- `envelope_id` -- the id of the tracked text file holding the envelope recipe
  (section 8.2). It MUST reference an existing tracked file.
- `version_map` -- the version-alignment table (section 8.4). It MUST contain at
  least a `doc_version: 0` row.

A conforming writer MUST store each member as an ordinary tracked file
(section 2.1) whose `name` is `"<group-name>/<member-path>"` (the group name
followed by the member's ZIP path). The AUTHORITATIVE member path used for
repack is recorded in the envelope recipe (`zip.members[].path`, section 8.2) and
the group `name`, NOT in the derived `files/<id>/<name>.wchg` tar member path.
A deep member path (e.g. `my-presentation.pptx/ppt/slideLayouts/slideLayout11.xml`)
can exceed the ustar `name`-field limit once the `files/<id>/` prefix and `.wchg`
suffix are added, so office members defer UNCHANGED to the section 2.1 rule: the
`<name>` portion of the `.wchg` member path is advisory and SHORTENED when it
would overflow, and the reader locates the log by the `files/<id>/` directory and
`.wchg` extension, never by reconstructing the `<name>` text. Office group
members therefore introduce NO new path-length requirement. A conforming reader
MUST be able to reconstruct each member file independently of the group.

### 8.2 The envelope recipe

Each document group MUST reference, via `envelope_id`, a tracked TEXT file (the
"envelope recipe") that captures everything needed to REPACK the office ZIP
byte-identically. Because it is an ordinary tracked text file, it is versioned
through the splice primitive (section 4.4) alongside the member parts: between
saves it changes only in the fields of changed members, so its stored delta is
small.

The envelope recipe content is a UTF-8 JSON object:

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

Rules:
- `zip.members` MUST list every ZIP member in the exact order the members appear
  in the original ZIP. A repack MUST write the local file headers and the central
  directory in this order.
- Each member entry MUST record enough to reproduce the member's bytes: `method`
  (`"store"` or `"deflate"`; for `"deflate"`, `deflate_level` and any
  strategy field the writer used), `crc32`, `uncompressed_size`,
  `compressed_size`, `dos_time`, `gp_flags`, `version_made_by`, `version_needed`,
  `external_attrs`, `internal_attrs`, `data_descriptor` (whether sizes/CRC
  were written in a trailing data descriptor), and the OPTIONAL `stored_compressed`
  flag (below). A repack MUST write these fields verbatim rather than recomputing
  them.
- Byte-identical reconstruction (section 5) is non-negotiable and applies to the
  WHOLE office container. For a member the original ZIP compressed with DEFLATE,
  re-emitting it STORED is NOT byte-identical (it changes the compression-method
  field in the local header and central directory, the compressed bytes, and
  `compressed_size`), so a writer MUST NOT fall back to `"store"` for an
  originally-DEFLATEd member. Instead:
  - At ingest, for each `"deflate"` member, the writer MUST VERIFY that
    re-DEFLATE at the recorded `deflate_level` reproduces the member's original
    compressed bytes exactly.
  - If verification SUCCEEDS, repack re-DEFLATEs the reconstructed member and the
    result is byte-identical.
  - If verification FAILS, the writer MUST either (a) store the member's ORIGINAL
    COMPRESSED bytes verbatim and set `stored_compressed: true` (repack then
    copies them, writing the recorded `deflate` method and sizes, so the member
    is byte-identical), or (b) FAIL the ingest with a typed non-reproducible
    error naming the member. A writer MUST NOT silently produce a non-identical
    repack.
  - `"store"` as a member `method` is valid ONLY for a member the original ZIP
    already stored uncompressed (e.g. the ODF `mimetype`).
- `stored_compressed` -- OPTIONAL boolean, default `false`. When `true`, the
  member's tracked file holds the member's ORIGINAL COMPRESSED bytes (not the
  decompressed content) and repack copies them without re-compressing. This is
  the exact-but-opaque fallback for a `deflate` member whose stream the writer's
  toolchain cannot reproduce; it costs that member its per-part splice-diff
  benefit but preserves byte-identity.
- `binary_members` lists non-text members by content hash. Each entry carries
  `content_sha256` (which also DERIVES the blob path `blobs/<content_sha256>`,
  section 8.5 -- there is no separate reference field) plus the same ZIP header
  fields and `stored_compressed` rule as a text member, so binary members repack
  byte-identically too.
- The envelope MUST serialise deterministically (stable key order, no
  serialise-time wall clock) so identical input yields identical envelope bytes.

### 8.3 Packing conventions per kind

- `kind: "odf"` -- the archive member named `mimetype` MUST be the FIRST ZIP
  member and MUST be STORED (uncompressed), per the ODF packaging rule. The
  envelope's first member entry MUST reflect this (`method: "store"`,
  `mimetype_first_stored: true`).
- `kind: "ooxml"` -- the package carries a `[Content_Types].xml` member and
  relation parts per the OPC convention. There is no mimetype member. The envelope
  member list reproduces the OOXML member order and headers.

### 8.4 Version alignment (document version -> member indices)

Each tracked file has its OWN independent index range (section 5). A document
group therefore MUST record how a DOCUMENT VERSION maps to each member's index,
because members change at different rates.

The group's `version_map` is an ordered list of rows, one per document version:

```
version_map[i] = { "doc_version": i, "members": { member_id: member_index, ... } }
```

The `members` object of a row contains an entry for EXACTLY the members present
at that document version, and no others. The KEY SET of the row is therefore the
per-version member set; a member id ABSENT from a row's `members` object is NOT a
ZIP member at that document version. This is the single representation of
changing membership -- there is no sentinel index and no "absent" marker value.
Per-version member ORDER is the order of the envelope's `zip.members[]`
(section 8.2) reconstructed at that same document version; the envelope and the
row MUST agree on membership.

Rules:
- `doc_version` 0 MUST map every member CURRENTLY present to index 0 (each
  member's current), consistent with the format-wide rule that index 0 is the
  current version (section 5).
- On each office save, a writer MUST append a new `version_map` row whose
  `members` keys are EXACTLY the members present in that save, each recorded at
  the member index that is current for that save (a member that changed this save,
  and any member whose older versions slid to higher indices, are recorded at
  their concrete indices; a member unchanged this save is recorded at the index
  its bytes now occupy).
- MEMBERSHIP CHANGE across versions is represented by presence/absence of the
  key, not by a special value:
  - A member ADDED at a later save MUST appear as a key only from the row of the
    save that added it onward; earlier rows MUST OMIT its id. A reader therefore
    does not repack it for an earlier document version.
  - A member REMOVED at a later save MUST stop appearing as a key from the
    removing save's row onward; earlier rows MUST still carry it.
- To reconstruct document version k, a reader MUST look up `version_map` row k and
  reconstruct EACH MEMBER PRESENT IN THAT ROW at the recorded `member_index` in
  that member's own `[0, count]` (section 5), writing them in the envelope's
  reconstructed `zip.members[]` order. A reader MUST NOT reconstruct or repack a
  member id absent from the row, and MUST NOT substitute an out-of-range or
  sentinel index for an absent member. Every recorded `member_index` MUST be
  valid in its member's range.
- When a member is re-based (section 4.3) by `condense` or the generations cap,
  the writer MUST update affected `version_map` rows so every recorded index
  stays valid, mirroring the per-file `rebase` audit (section 3).

### 8.5 Binary members

An office document MAY contain BINARY members (images, fonts, thumbnails). A
binary member MUST NOT be stored through the splice primitive, because a change
record's `insert` is a UTF-8 string (section 4.4) and the diff path rejects
binary input.

A conforming writer MUST store binary members by ONE of:
- (RECOMMENDED, available in 0.1) DEDUP-BY-CONTENT ATTACHMENT: store each distinct
  binary member body ONCE as an opaque blob member at `blobs/<content_sha256>` in
  the tar (the blob path is DERIVED from the member's `content_sha256`, so no
  separate reference field is stored), and reference it from the envelope's
  `binary_members[]` by `content_sha256`. An unchanged binary across document
  versions MUST reference the same blob (stored once); a changed binary is a new
  hash / new blob.
- (OPTIONAL, gated on the open question below) WHOLE-BLOB VERSIONED MEMBER: track
  the binary member as a tracked file whose versions are whole binary blobs. This
  REQUIRES the binary / base64 `insert` form of section 7 open question (b) to be
  resolved first; until then a writer MUST use the dedup-by-content attachment
  form above.

### 8.6 Resolving section 7 open question (b)

This proposal SCOPES, but does not fully resolve, section 7 open question (b)
("whether `insert` should support a binary (base64) form for non-text
documents"). For office containers in 0.1, binary members are handled by the
dedup-by-content attachment mechanism of section 8.5, which needs NO change to
the splice record. A future revision MAY add a binary `insert` form to enable
whole-blob VERSIONED binary members; such a change remains spec-first (INV-1) and
additive (a new optional record encoding, old readers degrade safely).

### 8.7 Conformance additions

A conforming reader that implements document groups MUST:
- treat `groups` as OPTIONAL; an archive without it conforms as a plain
  multi-file archive;
- validate that every `member_ids` entry (the union of all members ever seen) and
  each `envelope_id` references an existing tracked file, that every member id
  appearing in any `version_map` row is also in `member_ids`, and that every
  `version_map` index is within its member's range;
- reconstruct a document version by resolving the `version_map` row, reconstructing
  ONLY the members present in that row (its key set) each at its aligned index
  (section 8.4), then replaying the envelope recipe (section 8.2) in the envelope's
  reconstructed member order to emit the office file; a member id absent from the
  row MUST NOT be repacked.

A conforming reader that does NOT implement document groups MUST still read every
member as an ordinary tracked file and MUST NOT reject the archive solely because
it carries a `groups` array (safe degradation).

A conforming writer that creates a document group MUST:
- store each ZIP member as an ordinary tracked file and record the group,
  envelope, and `version_map` as above;
- produce an envelope that, replayed, reconstructs the ORIGINAL office file
  byte-for-byte (section 5 byte-identical guarantee, applied to the whole office
  container);
- store binary members per section 8.5;
- keep the whole write transactional (section 6).
