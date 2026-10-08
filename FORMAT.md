# Walrus File Format Specification

Version: 0.1 (draft)
Status: DRAFT -- subject to change until 1.0.

This document defines the `.wal` and `.wbl` container formats independently of
any implementation, so the formats can be read and written in any language. The
`walrus-fmt` Rust crate in this repository is the reference implementation.

All multi-byte integers are little-endian. All text is UTF-8.

---

## 1. Concept

A Walrus file stores a text document together with an ordered log of the changes
that relate one version of the document to another. Because the whole history
travels inside one artefact, a reader can reconstruct any version and "play"
through the versions forward or backward.

There are two symmetric variants:

| Ext    | Base is...        | Log direction | Reconstruct the base by | Reconstruct the other end by |
|--------|-------------------|---------------|-------------------------|------------------------------|
| `.wal` | the ORIGINAL text | forward       | reading the base        | applying the log forward     |
| `.wbl` | the CURRENT text  | reverse       | reading the base        | applying the log backward    |

```
.wal :  original (base)  + forward_log  = current
.wbl :  current  (base)  - reverse_log  = original
```

Rationale:
- `.wal` favours cheap append-on-save: new edits append to the forward log and
  the base (original) never moves.
- `.wbl` favours distribution: the base IS the current, correct text, so a naive
  reader that extracts only the base already sees the right content.

The two are inter-convertible: applying every log record to one base yields the
other base, and inverting each record yields the other direction's log. The
`walx` tool performs `.wal <-> .wbl` conversion.

---

## 2. Physical container

A Walrus file is a **compressed tar archive**.

- The outer layer is **zstd** compression (default, codec id `zstd`). An
  implementation MAY support `gzip` as an alternative outer layer; the codec is
  identified by the archive's magic bytes, not the file extension.
- The inner layer is an uncompressed **tar** archive (ustar) containing a fixed
  set of member files.

A reader MUST sniff the outer codec from magic bytes:
- zstd: `0x28 0xB5 0x2F 0xFD`
- gzip: `0x1F 0x8B`

### 2.1 Tar members (fixed names)

| Member path     | Required | Contents                                                        |
|-----------------|----------|-----------------------------------------------------------------|
| `manifest.json` | yes      | JSON manifest (section 3)                                       |
| `base`          | yes      | The base document bytes (original for `.wal`, current for `.wbl`) |
| `log.jsonl`     | yes      | The change log, one JSON record per line (section 4)            |
| `meta/`         | no       | Optional free-form sidecar files (e.g. author notes)            |

The `base` member carries no implied text encoding beyond "the document bytes";
`manifest.json` records the declared encoding and MIME type.

---

## 3. manifest.json

```json
{
  "format": "walrus",
  "spec_version": "0.1",
  "variant": "wal",
  "base_role": "original",
  "log_direction": "forward",
  "document": {
    "encoding": "utf-8",
    "mime": "text/plain",
    "name": "document.txt"
  },
  "versions": {
    "count": 42,
    "base_index": 0,
    "head_index": 42
  },
  "checkpoints": [
    { "index": 0,  "label": "original" },
    { "index": 20, "label": "before refactor" },
    { "index": 42, "label": "current" }
  ],
  "created": "2026-10-08T12:00:00Z",
  "generator": "walx 0.1.0"
}
```

Field notes:
- `variant` is `"wal"` or `"wbl"`; `base_role` and `log_direction` MUST agree
  with the table in section 1.
- `versions.count` is the number of log records. Version indices run from
  `0` (the base) to `count` (the far end).
- `checkpoints` is an optional, sorted list of named indices for human
  addressing. Indices MUST be in `[0, count]`.

---

## 4. The change log (log.jsonl)

Each line is one JSON **change record**. Records are stored in natural order:
record `i` transforms version `i` into version `i+1` (for `.wal`, forward) or
version `i` into version `i-1`... see "Addressing" below for how direction maps
to indices.

A change record is a **splice**: at a byte offset, delete `delete_len` bytes and
insert `insert` bytes. This is deliberately the inverse-splice primitive that a
piece-table / undo journal already produces.

```json
{ "op": "splice", "at": 128, "delete_len": 3, "insert": "walrus", "ts": "2026-10-08T12:01:02Z", "note": null }
```

- `at` -- byte offset into the version the record applies to.
- `delete_len` -- number of bytes removed at `at`.
- `insert` -- UTF-8 string inserted at `at` after deletion (may be empty).
- `ts` -- optional ISO-8601 timestamp.
- `note` -- optional human label for this single step.

A record is self-inverting given the version it applied to: to undo it, delete
`insert.len()` bytes at `at` and insert the bytes that were deleted. Reverse-log
(`.wbl`) records store the already-inverted form so that applying `log.jsonl`
top-to-bottom walks CURRENT -> ... -> ORIGINAL.

### 4.1 Checkpoint records (optional)

A line may instead be a checkpoint marker that carries a full snapshot hash for
integrity (not a splice). Readers that do not verify MAY skip these.

```json
{ "op": "checkpoint", "index": 20, "sha256": "..." }
```

---

## 5. Addressing a version

A requested version is resolved to an integer **index** in `[0, count]`:

1. **By index**: the integer directly (canonical form; always valid).
2. **By checkpoint label**: look up `checkpoints[].label` -> `index`.
3. **By timestamp** (`--at T`): the largest index whose record `ts <= T`
   (records before the first timestamp resolve to the base, index 0).
4. **Relative**: `base + n`, `head - n`, or `current +/- n`.

Reconstruction:
- `.wal`: start from `base` (index 0 = original). To reach index `k`, apply
  forward records `1..=k`.
- `.wbl`: start from `base` (index 0 = current). To reach index `k`, apply
  reverse records `1..=k` (walking back toward the original).

An implementation MUST produce byte-identical output for the same index
regardless of variant once the variant's index semantics are accounted for.

---

## 6. Conformance

A conforming reader MUST:
- sniff and decompress the outer codec;
- read `manifest.json` and reject unknown `format`/incompatible `spec_version`;
- reconstruct any index in `[0, count]` by replaying splices;
- treat `base` bytes using the declared `document.encoding`.

A conforming writer MUST:
- emit the fixed tar members with a valid manifest;
- keep `versions.count` equal to the number of splice records;
- keep `checkpoints` indices within range and sorted.

---

## 7. Open questions (pre-1.0)

- Whether to allow line-oriented records (line/col) in addition to byte splices.
- Whether `insert` should support a binary (base64) form for non-text documents.
- A compaction/re-baseline record format for very long logs.
