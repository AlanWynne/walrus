# Walrus File Format Specification

Version: 0.1 (draft)
Status: DRAFT -- multi-file revision. Subject to change until 1.0. This revision
replaces the earlier single-file framing: an archive now holds MANY tracked
files, and the inner per-file change log is a `.wchg` file with its own header.
This revision also DECOUPLES per-file direction from the archive variant and the
file extension: the per-file `.wchg` `direction` is the sole authority for how a
file reconstructs, the archive `variant` only seeds the default direction for
newly added files, and an archive MAY hold a mix of forward and reverse files.

This document defines the `.wal` and `.wbl` container formats independently of
any implementation, so the formats can be read and written in any language. The
`walrus-fmt` Rust crate in this repository is the reference implementation.

All multi-byte integers are little-endian. All text is UTF-8.

---

## 1. Concept

A Walrus archive bundles a set of tracked text documents together with, for each
document, an ordered log of the changes that relate one version of that document
to another. Because the whole history travels inside one artefact, a reader can
reconstruct any version of any tracked file and "play" through its versions
forward or backward.

There are two symmetric variants:

| Ext    | Base is...        | Log direction | Reconstruct the base by | Reconstruct the other end by |
|--------|-------------------|---------------|-------------------------|------------------------------|
| `.wal` | the ORIGINAL text | forward       | reading the base        | applying the log forward     |
| `.wbl` | the CURRENT text  | reverse       | reading the base        | applying the log backward    |

```
.wal :  original (base)  + forward_log  = current
.wbl :  current  (base)  - reverse_log  = original
```

Direction is a PER-FILE property, not an archive-wide rule. Each tracked file
carries its own `direction` in its `.wchg` header (section 4.2), and THAT header
is the sole authority for how the file reconstructs. The extension (`.wal` /
`.wbl`) and the manifest `variant` are CONVENTIONS and DEFAULTS -- they are never
consulted to reconstruct a file. A single archive MAY therefore hold a mix of
forward and reverse files under either extension.

By convention (not by rule):
- a `.wal` archive is forward-heavy: it defaults newly added files to a forward
  log over an original base, favouring cheap append-on-save (the base never
  moves as edits append).
- a `.wbl` archive is reverse-heavy: it defaults newly added files to a reverse
  log over a current base, favouring distribution (each base IS the current,
  correct text, so a naive reader that extracts only the bases already sees the
  right content).

"Forward" and "reverse" below describe a single file's log:

```
forward file :  original (base)  + forward_log  = current
reverse file :  current  (base)  - reverse_log  = original
```

The two senses are inter-convertible: applying every log record to one base
yields the other base, and inverting each record yields the other direction's
log. The `walrus` tool performs this conversion per file (and all other
management operations); `walx` is the extract-only companion.

---

## 2. Physical container

A Walrus archive is a **compressed tar archive**.

- The outer layer is **zstd** compression (default, codec id `zstd`). An
  implementation MAY support `gzip` as an alternative outer layer; the codec is
  identified by the archive's magic bytes, NOT by the file extension.
- The inner layer is an uncompressed **tar** archive (ustar) containing a
  top-level manifest plus one directory per tracked file.

A reader MUST sniff the outer codec from magic bytes:
- zstd: `0x28 0xB5 0x2F 0xFD`
- gzip: `0x1F 0x8B`

The file extension (`.wal` / `.wbl`) is a CONVENTIONAL hint for the archive's
default direction (section 3), not the compression codec and not a per-file
reconstruction rule. Each file's `.wchg` `direction` is authoritative for how
that file replays (section 4.2).

### 2.1 Tar members (per-file layout)

An archive contains exactly one top-level `manifest.json`, and for each tracked
file a directory `files/<id>/` holding that file's base and its `.wchg` change
log. `<id>` is a stable, archive-unique identifier for the tracked file (see the
manifest `id` field in section 3); it is a filesystem-safe slug and is NOT the
original filename (the original name, which may contain path separators or
awkward characters, is recorded in the manifest).

| Member path                 | Required | Contents                                                              |
|-----------------------------|----------|-----------------------------------------------------------------------|
| `manifest.json`             | yes      | JSON manifest describing the archive and every tracked file (section 3) |
| `files/<id>/base`           | yes      | The base document bytes for file `<id>` (original for `.wal`, current for `.wbl`) |
| `files/<id>/<name>.wchg`    | yes      | The per-file change log for file `<id>` (section 4)                   |
| `meta/`                     | no       | Optional free-form sidecar files (e.g. author notes)                  |

Notes:
- There is one `files/<id>/` directory per tracked file; the set of members is
  therefore NOT fixed -- it grows and shrinks with the tracked files. An archive
  MAY contain zero tracked files (an empty archive still has a valid
  `manifest.json`).
- `<name>` in `files/<id>/<name>.wchg` is a readable, filesystem-safe rendering
  of the tracked file's name, present only for human convenience when the log is
  extracted standalone. The AUTHORITATIVE original filename, path, and
  attributes live in the manifest's per-file entry (section 3), not in the
  member path.
- Each tracked file's `base` member carries no implied text encoding beyond "the
  document bytes"; the manifest records that file's declared encoding and MIME
  type.
- Each tracked file has its OWN independent version index range, defined by its
  own `.wchg` log. There is no global/archive-wide index.

---

## 3. manifest.json

The manifest describes the archive as a whole and carries a `files` array with
one entry per tracked file.

```json
{
  "format": "walrus",
  "spec_version": "0.1",
  "variant": "wal",
  "base_role": "original",
  "log_direction": "forward",
  "created": "2026-10-08T12:00:00Z",
  "generator": "walrus 0.1.0",
  "files": [
    {
      "id": "a1b2c3",
      "name": "src/document.txt",
      "encoding": "utf-8",
      "mime": "text/plain",
      "attributes": {
        "size": 2048,
        "mtime": "2026-10-08T11:59:30Z",
        "mode": "0644"
      },
      "versions": {
        "count": 42,
        "base_index": 42,
        "head_index": 0
      },
      "generations": 0,
      "checkpoints": [
        { "index": 42, "label": "original" },
        { "index": 20, "label": "before refactor" },
        { "index": 0,  "label": "current" }
      ],
      "rebase": {
        "count": 3,
        "last": "2026-10-08T12:30:00Z",
        "dropped": 7
      }
    }
  ]
}
```

Archive-level field notes:
- `variant` is ALWAYS a concrete `"wal"` or `"wbl"` (never `"mixed"`). Its ONLY
  job is to drive the DEFAULT direction for newly added files: `wal` defaults a
  new file to forward, `wbl` defaults it to reverse (section 5). It is
  informational for defaulting and is NEVER consulted to reconstruct a file --
  reconstruction direction comes from each file's `.wchg` header (section 4.2).
  A file whose direction differs from the archive `variant` is LEGAL.
- `base_role` and `log_direction` are DERIVED DESCRIPTORS of what `variant`
  defaults to (`wal` -> original base / forward log; `wbl` -> current base /
  reverse log). They describe the DEFAULT for new files, not a per-file
  constraint, and do not override any file's own `.wchg` `direction`.
- "Mixedness" (an archive holding both forward and reverse files) is an
  EMERGENT, COMPUTED property, derived by scanning the per-file `.wchg` headers.
  It is NOT a stored manifest value; `variant` never becomes `"mixed"`. A
  listing tool (e.g. `walrus query -l`) MAY surface that an archive is mixed.
- `created` and `generator` describe the archive provenance.

Per-file field notes (each element of `files`):
- `id` -- stable, archive-unique, filesystem-safe identifier matching the
  `files/<id>/` directory. MUST NOT change for the life of the tracked file.
- `name` -- the original filename and relative path as the user supplied it
  (the authoritative name; may contain `/`).
- `encoding` / `mime` -- the declared text encoding and MIME type for this
  file's `base` bytes and reconstructed versions.
- `attributes` -- recorded original file attributes (size, mtime, mode, and any
  other platform attributes a writer chooses to preserve).
- `versions.count` -- the number of change records in this file's `.wchg` log.
  Valid version indices for this file run from `0` (current) to `count` (the far
  end). Index 0 is ALWAYS the current version for both variants; higher indices
  are older (see section 5).
- `versions.base_index` / `head_index` -- the index that the stored `base`
  represents and the opposite end. For `.wal` the base is the original, so
  `base_index = count` and `head_index = 0`. For `.wbl` the base is the current,
  so `base_index = 0` and `head_index = count`.
- `generations` -- the per-file retention cap mirrored from this file's `.wchg`
  header (section 4.2). `0` = unlimited (default); `N >= 1` = rolling window of N
  versions. The manifest copy is a convenience for listings; the `.wchg` header
  is authoritative so a standalone log stays self-describing.
- `checkpoints` -- an optional list of named indices for human addressing, each
  with `index` in `[0, count]`. Indices are in this file's own index space.
- `rebase` -- OPTIONAL audit metadata for history dropped by `condense` or by the
  `generations` rolling-window trim (section 4.3). Re-base leaves NO marker in
  the log; this object is where the fact is recorded instead. Fields:
  `count` (how many re-base operations have occurred for this file), `last` (ISO
  timestamp of the most recent re-base), `dropped` (total versions discarded by
  re-basing). Absent means the file has never been re-based. A `.wchg` viewer MAY
  surface this as "history before the current base was condensed". These are
  informational and do not affect reconstruction of the retained range.

A reader MUST treat the `files` array as the authoritative directory of tracked
files; the tar member layout (section 2.1) is derived from it.

---

## 4. The per-file change log (`.wchg`)

Each tracked file has one `.wchg` log at `files/<id>/<name>.wchg`. A `.wchg` is
self-describing: it begins with a structured header line, then carries one
change record per line. This framing lets a `.wchg` extracted from the archive
be replayed STANDALONE, without the parent archive, because the header states
the log direction and the base it expects.

### 4.1 Framing

A `.wchg` is a UTF-8, newline-delimited text stream:

- **Line 1 -- header object.** A single JSON object on the first line (the
  "header line"). It MUST appear exactly once, as the first line.
- **Lines 2..N -- records.** One JSON change record (or checkpoint record) per
  line, in natural order. Record `i` (counting from the line after the header)
  transforms version `i-1` into version `i` along the log's declared direction;
  see section 5 for how direction maps onto indices.

This is JSONL with a mandatory header line. A standalone reader reads the first
line as the header, then streams the remaining lines as records.

Record terminator (STRICT): every line -- the header and each record -- is
terminated by a single **LF (`\n`, `0x0A`)**. CR (`\r`) is NOT a terminator.
- A writer MUST emit LF only and MUST NOT emit CRLF.
- A reader MUST split on LF only. A trailing `\r` (i.e. a CRLF-terminated line)
  is a malformed `.wchg` and the reader MUST reject it, rather than silently
  tolerating it. This keeps framing unambiguous (git enforces LF similarly;
  modern editors adapt easily).
- Because records are JSON, any newline inside document content is escaped as
  `\n` WITHIN a JSON string (per JSON rules), so a raw `0x0A` byte in the stream
  is ALWAYS a record boundary and NEVER part of content. This is strictly less
  ambiguous than a plain-text line format.
- This LF rule governs the `.wchg` LOG FRAMING ONLY. It does NOT constrain the
  line endings of the TRACKED DOCUMENTS: a tracked file may contain CRLF (or any)
  bytes internally; those bytes live inside a record's `insert` string and are
  stored and reconstructed faithfully, untouched.
- Record content is NEVER trimmed. Whitespace inside a record (including trailing
  spaces in an `insert` string -- e.g. a Markdown hard line break) is document
  DATA and is preserved exactly; trimming it would corrupt the document and break
  byte-identical reconstruction.

### 4.2 Header object

```json
{ "format": "wchg", "spec_version": "0.1", "direction": "forward", "generations": 0, "name": "src/document.txt", "encoding": "utf-8", "count": 42 }
```

Header fields:
- `format` -- MUST be `"wchg"`. A reader MUST reject any other value.
- `spec_version` -- the `.wchg` spec version (tracks this document).
- `direction` -- `"forward"` or `"reverse"`. This is the DIRECTION ATTRIBUTE
  that makes the log self-describing:
  - `"forward"` -- the log replays on top of the ORIGINAL base to move toward
    the current version (the `.wal` sense).
  - `"reverse"` -- the log replays on top of the CURRENT base to move toward the
    original version (the `.wbl` sense).
  A reader opening a bare `.wchg` outside any archive uses `direction` to know
  which base the log expects and which way to replay, without consulting a
  parent archive's extension. This `direction` is PER FILE and AUTHORITATIVE for
  reconstruction; it does NOT have to agree with the archive `variant`. The
  archive `variant` only seeds the DEFAULT direction for newly added files
  (section 5); a file may be forward or reverse under either archive variant.
- `generations` -- the retention cap (section 4.3). `0` = unlimited (default);
  integer `N >= 1` = rolling window of N versions. This field is authoritative;
  the manifest mirrors it.
- `name` -- the original filename/path, for standalone readability. The archive
  manifest remains authoritative when the log lives inside an archive.
- `encoding` -- the declared text encoding of the base and reconstructed
  versions.
- `count` -- the number of change records that follow (equal to the manifest's
  per-file `versions.count`). A reader MAY use it to validate the record count.

A writer MAY include additional header fields; a reader MUST ignore unknown
header fields.

### 4.3 The generations cap

`generations` bounds how many versions a file retains:

- `generations = 0` (DEFAULT): unlimited retention. The index range grows
  without an upper bound as versions are added.
- `generations = N` (`N >= 1`): keep at most N versions as a ROLLING WINDOW.
  When adding a new version would make the retained count exceed N, the OLDEST
  version is dropped automatically as part of the add/update.

Dropping the oldest version is a RE-BASE, NOT a plain delete:
- In a `.wal` the oldest version IS the base (the original). Re-basing folds the
  oldest change record into the base so that the next-oldest version becomes the
  new base, then removes that record from the log.
- In a `.wbl` the oldest version is the far end of the reverse log. Re-basing
  removes the farthest record and the remaining log still reconstructs every
  retained version from the (unchanged current) base.

This is the SAME re-base operation that `condense` performs; the generations cap
simply triggers it automatically on each add/update. Effect on the valid index
range: after a drop, `count` decreases by one and the highest valid index for
the file decreases by one (index 0 remains the current version). With
`generations = N`, the valid index range is `[0, min(count, N-1)]`.

Re-base leaves NO marker record in the log. The `.wchg` stays a clean sequence of
change records; a re-base folds the dropped record into the base and emits no
pseudo-record, so a reader never has to skip a non-splice. The audit trail of
re-basing is instead recorded as METADATA in the manifest's per-file entry (the
`rebase` object, section 3), so the information is preserved without polluting
the log. This resolves the earlier open question about a re-baseline marker: the
answer is a manifest note, not a log record.

### 4.4 Change records

Each record line (after the header) is a JSON **change record**. A change record
is a **splice**: at a byte offset, delete `delete_len` bytes and insert `insert`
bytes. The splice is deliberately the inverse-splice primitive that a
piece-table / undo journal already produces, and it is the SINGLE change
primitive in the format.

Line-orientation is a DIFF GRANULARITY concern, NOT a second record format. A
writer MAY diff line-by-line to produce larger, line-aligned changes (good for
source code and a readable history view), but the stored record is STILL a
byte-offset splice -- line granularity only makes the splice's `at`/`delete_len`
fall on line boundaries. The format does NOT define a `{line, col}` record type:
byte offsets are unambiguous, whereas line/column counting depends on line-ending
convention, tab width, and multi-byte characters. Keeping the byte splice as the
one primitive avoids that ambiguity and keeps every reader, the reconstruct
engine, the viewer, and conversion handling a single record shape.

```json
{ "op": "splice", "at": 128, "delete_len": 3, "insert": "walrus", "ts": "2026-10-08T12:01:02Z", "note": null }
```

- `at` -- byte offset into the version the record applies to.
- `delete_len` -- number of bytes removed at `at`.
- `insert` -- UTF-8 string inserted at `at` after deletion (may be empty).
- `ts` -- optional ISO-8601 timestamp.
- `note` -- optional human label for this single step.

A record is self-inverting given the version it applied to: to undo it, delete
`insert` byte-length bytes at `at` and insert the bytes that were deleted.
A `direction: "reverse"` log stores the already-inverted form so that applying
its records top-to-bottom walks CURRENT -> ... -> ORIGINAL; a
`direction: "forward"` log walks ORIGINAL -> ... -> CURRENT.

How a writer OBTAINS the splice for a save is not constrained by the format. A
writer MAY diff old-vs-new text to compute the net splice, OR take it directly
from a caller's edit journal / piece list coalesced into the single NET splice
for that save (one record per save, see section 5). Either way the stored record
is the same byte-offset splice -- this is NOT a new record type (the splice stays
the single primitive). The caller always supplies the FORWARD-sense edit (what it
did to move from the old version to the new one); the writer then applies the
file's stored direction: for a `direction: "forward"` file it stores the splice
as-is and the base (original) is unchanged; for a `direction: "reverse"` file it
INVERTS the incoming forward splice and ADVANCES the base to the new current
bytes. Direction handling therefore lives entirely in the writer; the caller
neither knows nor cares about the file's direction.

### 4.5 Checkpoint records (optional)

A record line may instead be a checkpoint marker that carries a full snapshot
hash for integrity (not a splice). Readers that do not verify MAY skip these.

```json
{ "op": "checkpoint", "index": 20, "sha256": "..." }
```

The `index` is in the tracked file's own index space.

---

## 5. Addressing a version (per file)

Addressing is PER TRACKED FILE: each file has its own index range `[0, count]`
defined by its own `.wchg`. A requested version of a given file is resolved to an
integer **index** in that file's `[0, count]`:

1. **By index**: the integer directly (canonical form; always valid for the
   file). Index 0 is the current version; higher indices are older.
2. **By checkpoint label**: look up the file's `checkpoints[].label` -> `index`.
3. **By timestamp** (`--at T`): the index whose record best matches `T` within
   the file's log (records before the earliest timestamp resolve to the current
   version, index 0).
4. **Relative**: `current +/- n`, `head - n`, or `base +/- n`, evaluated within
   the file's own range.

Reconstruction for a file is driven by that file's OWN `.wchg` `direction`
header (section 4.2), NOT by the archive `variant` or the file extension:
- forward file (`direction: "forward"`): the base is the original. The current
  version is at the head; to reach index `k` (where index 0 is current and
  `count` is the original), apply the forward records needed to arrive at that
  version.
- reverse file (`direction: "reverse"`): the base is the current (index 0). To
  reach index `k`, apply the first `k` reverse records (walking back toward the
  original at index `count`).

An implementation MUST produce byte-identical output for the same index of the
same tracked file regardless of direction, once that file's base and direction
are accounted for. This is a PER-FILE guarantee; because direction is per file,
an archive may mix forward and reverse files and each still reconstructs from its
own header.

---

## 6. Conformance

A conforming reader MUST:
- sniff and decompress the outer codec from magic bytes (section 2);
- read `manifest.json`, reject unknown `format` / incompatible `spec_version`,
  and treat the `files` array as the directory of tracked files;
- for each tracked file, read its `files/<id>/base` and parse its
  `files/<id>/<name>.wchg` header line before the records;
- reconstruct any index in a file's `[0, count]` by replaying that file's
  splices in the direction its OWN `.wchg` header declares -- NOT the archive
  `variant` or the file extension, and WITHOUT requiring the file's direction to
  match the archive `variant` (a mixed-direction archive is legal);
- treat each file's `base` bytes using that file's declared `encoding`.

- split `.wchg` lines on LF only and REJECT a CRLF-terminated (trailing `\r`)
  line as malformed (section 4.1);
- preserve record content byte-for-byte, never trimming whitespace inside a
  record (section 4.1).

A conforming reader of a STANDALONE `.wchg` (outside an archive) MUST:
- parse the first line as the header object and reject a missing or non-`wchg`
  header;
- use the header `direction` to decide which base the log expects and which way
  to replay.

A conforming writer MUST:
- emit a valid `manifest.json` plus, for every tracked file, a `files/<id>/base`
  and a `files/<id>/<name>.wchg` whose header line is present and well-formed;
- keep each file's `versions.count` equal to the number of splice records in its
  `.wchg`, and keep the `.wchg` header `generations` and the manifest mirror in
  agreement;
- keep every file's `checkpoints` indices within that file's range;
- enforce each file's `generations` cap by re-basing out the oldest version on
  add/update (section 4.3);
- perform archive mutations transactionally (build to a temp file and atomically
  rename over the original) so a failed write leaves the original unchanged.

---

## 7. Open questions (pre-1.0)

Resolved in this revision (previously open): multi-file archives (section 2),
the `.wchg` naming and self-describing direction header (section 4), and the
per-file generations cap (section 4.3). These are now normative above.

Still open:
- Whether to allow line-oriented records (line/col) in addition to byte splices.
- Whether `insert` should support a binary (base64) form for non-text documents.
- The exact record format, if any, that `condense` / generation re-basing should
  leave behind to mark a re-baseline point in a very long log (currently a
  re-base simply folds the dropped record into the base and emits no marker).
