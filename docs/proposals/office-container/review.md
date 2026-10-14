# Office-container design review (iteration 4)

Reviewer: design-review subagent (fresh read, no authoring context).
Scope: DOCUMENTS-ONLY. Four planning documents reviewed against the authoritative
sources. No code was written or expected; nothing was built or tested.

Documents reviewed:
- `requirements.md`
- `design.md`
- `format-change-proposal.md`
- `decisions.md`

Authoritative sources consulted:
- `FORMAT.md` (section 5 byte-identical per-file reconstruction; section 7 open
  questions, especially (b) binary/base64 insert).
- `.agents/tasks/requirements-walrus.md` (FR-/AC/INV/NFR structure and tone).
- `.agents/tasks/walker-viewer/design.md` (design-doc style).
- `crates/walrus/src/diff.rs`, `record.rs`, `manifest.rs`, `encoding.rs`,
  `error.rs`.

## Verdict summary

No HIGH findings. No MEDIUM findings. Three NIT findings (non-blocking).

VERDICT: APPROVED.

## Mandatory review checks

(a) VERSION ALIGNMENT -- PASS. The rule is fully specified and unambiguous in
design.md section 5, FORMAT.md proposal section 8.4, and requirements FR-OC4. A
per-group `version_map` holds one row per document version; each row's `members`
object has a key for EXACTLY the members present at that version, mapping each to
the concrete member index current at that save. Membership change is modelled by
key presence/absence (ADD: earlier rows omit the id; REMOVE: later rows omit it)
with no sentinel index and no out-of-range fallback. The envelope's reconstructed
`zip.members[]` is named the single source of truth for per-version membership
and order, and a consistency check against the row is specified (design section 7
step 3). "Document version k" maps deterministically to each member's own index
even when members have different save counts. This correctly accounts for the
per-file independent index range confirmed in `manifest.rs`
(`Versions`/`TrackedFile`).

(b) ENVELOPE RECIPE -- PASS. design.md section 4 and proposal section 8.2
enumerate every field needed for byte-identical repack: ordered `members[]`
(local-header and central-directory order), per-member `method`
(`store`/`deflate`) with `deflate_level`, `crc32`, `uncompressed_size`,
`compressed_size`, `dos_time`, `gp_flags`, `version_made_by`, `version_needed`,
`external_attrs`, `internal_attrs`, `data_descriptor`, the ODF
`mimetype_first_stored` rule, the OOXML `[Content_Types].xml` note (section 8.3),
the EOCD `comment`, and the `stored_compressed` last-resort fallback for a
DEFLATE member whose stream the toolchain cannot reproduce. The design correctly
forbids silently re-emitting an originally-DEFLATEd member as STORED (that would
be Option B) and requires ingest-time re-DEFLATE verification or a typed failure.

(c) MANIFEST GROUPING ADDITIVE -- PASS. `groups` is an OPTIONAL `#[serde(default)]`
array; members are REAL `files/<id>/` tracked files with their own bases and
`.wchg` logs and their own independent index ranges, so the proposal does not
contradict FORMAT.md's per-file index rule (section 5). A group-unaware reader
still reconstructs each member; it only cannot repack. Backward compatibility is
stated in all three places (requirements FR-OC3 AC2/AC4, design section 3,
proposal section 8 preamble + 8.7).

(d) DEPENDENCY BOUNDARY (INV-2) -- PASS. All heavy logic (ZIP codec, office
awareness, envelope build/replay, ingest, repack) is behind an optional `office`
cargo feature with `zip = { optional = true }`; only the plain-serde grouping
TYPES/accessors are always-compiled. A `cargo tree` boundary proof (default vs
`--features office`, asserting no ZIP and no GUI/app crate) is specified in
FR-OC8, NFR-3, and design section 11.

(e) ASCII-ONLY -- PASS. A regex scan of all four documents for non-ASCII outside
the permitted box-drawing range (U+2500-U+257F) returned no matches. ASCII `->`,
`<->`, `<=`, `>=`, `--` are used throughout.

(f) BINARY SECTION-7 (b) DEPENDENCY -- PASS. Called out explicitly, not glossed,
in ADR-5, FR-OC7 AC4 + prerequisite P2, design section 6 and section 10, and
proposal section 8.6. The 0.1 implementation ships the dedup-by-content
attachment fallback (needs no splice-record change); whole-blob versioned binary
members are gated on resolving section 7 (b).

Additional required checks:
- Containerisation: option 2 chosen; options 1 (single inner tar) and 3 (nested
  `.wal`) recorded as rejected with reasons -- decisions ADR-1, design section 9,
  requirements section 6. PASS.
- Identity: Option A (byte-identical) chosen over Option B (semantic identity)
  with justification -- decisions ADR-2, design section 9. PASS.
- OLE2 legacy formats explicitly out of scope -- decisions ADR-3, requirements
  scope + FR-OC1 AC3, proposal section 8, design section 10. PASS.
- Four documents mutually consistent: the `member_ids`-is-union rule, the
  members-present-only `version_map` row, the `stored_compressed` DEFLATE rule,
  the derived `blobs/<content_sha256>` blob path (no separate reference field),
  and the OLE2/binary scope all agree across the four files. PASS.
- requirements.md matches the FR-/AC/INV/NFR structure of
  `requirements-walrus.md` (status header, ASCII note, scope/ownership, invariants,
  cross-stream/API impact, FR with ACs, NFRs, prerequisites, ordered delivery).
  PASS.
- Spec-first ordering stated (FORMAT.md change lands before code) -- INV-1,
  decisions ADR-6, requirements P1 + delivery step 1, proposal status line. PASS.

## Source-accuracy verification

- diff.rs: `diff(old: &str, new: &str)` takes UTF-8 `&str`; `reject_binary`
  returns `WalrusError::BinaryInput` on a NUL byte. The docs state this
  correctly (FR-OC7, design section 6, ADR-5).
- record.rs: `Op::Splice { insert: String }` is UTF-8, not bytes. The docs state
  this correctly.
- manifest.rs: `TrackedFile.name` is a plain `String` permitting `/`, and the
  `f<N>` id is decoupled from the name. The docs' member-naming claim
  (`<group-name>/<member-path>` stored in `name`, no format change needed) is
  accurate. Each `TrackedFile` has its own `Versions.count`, confirming the
  independent per-file index range the version-alignment rule relies on.
- error.rs: `WalrusError::BinaryInput` exists exactly as cited; the enum is
  `#[non_exhaustive]`, so the proposed new `WalrusError::OfficeNonReproducible`
  variant is a backward-compatible addition. The docs present it as a
  to-be-added variant, which is correct.
- FORMAT.md section 5 is a PER-FILE byte-identical guarantee; the docs correctly
  describe the office whole-file guarantee as "built on INV-3 per inner part plus
  a byte-exact repack recipe" rather than claiming the existing section 5 already
  covers whole containers.
- FORMAT.md section 7 open question (b) is correctly quoted and only SCOPED (not
  resolved) by proposal section 8.6.

## Findings

1. NIT -- `design.md` section 0 "Locked constants" lists the ZIP crate as
   `zip = "2"` and section 8 adds `default-features = false, features =
   ["deflate"]`, while section 10 / section 4 refer to the DEFLATE toolchain's
   reproducibility in general terms. The pure-Rust `zip` crate's `deflate`
   feature historically selects a specific backend (`flate2`/miniz vs a
   `zlib`-based one), and byte-exact re-DEFLATE reproducibility depends on which
   backend and version is linked. The docs correctly make this an
   implementation-time verification (verify re-DEFLATE at ingest; else
   `stored_compressed` or fail), so this is not a design gap. Concrete
   suggestion: when the crate is pinned at implementation time, record in
   design.md section 8 WHICH DEFLATE backend the chosen `zip` minor pulls in, so
   the reproducibility experiment is run against a named backend rather than
   "the toolchain". Non-blocking.

2. NIT -- `record.rs` module doc still says "Change-log records (`log.jsonl`)."
   whereas FORMAT.md and these documents use `.wchg`. This is a pre-existing
   `.rs` discrepancy that design.md section 12 already records as BACKLOGGED
   (NIT-3 from an earlier iteration) and correctly defers because this task may
   not edit `.rs`. No action required for the documents; re-stated here only so
   the backlog item is visible to the implementer who next touches `record.rs`.
   Non-blocking.

3. NIT -- The envelope JSON sketches use a `dos_time` value in ISO-8601 form
   (e.g. `"2026-10-08T12:00:00"`). A ZIP local header stores an MS-DOS
   date/time (2-second resolution, packed 16+16 bits), which cannot represent
   odd seconds and has a 1980 epoch. Storing it as an ISO-8601 string in the
   envelope is fine as a REPRESENTATION, but the design should state at
   implementation time that the recorded value is the EXACT MS-DOS field
   (round-tripped losslessly to/from the packed bits), not a re-derived
   wall-clock time, so repack reproduces the original packed bytes. design.md
   section 4 already lists `dos_time` among "the ZIP header fields that must be
   reproduced ... written verbatim", which covers the intent; adding a one-line
   note that `dos_time` encodes the raw MS-DOS field (not a converted timestamp)
   would remove any ambiguity. Non-blocking.

## Verified assumptions

- The design's claims about diff.rs (`&str` UTF-8 only; NUL -> `BinaryInput`),
  record.rs (`Op::Splice.insert: String`), and manifest.rs (`name` permits `/`,
  id decoupled, per-file independent index) are all confirmed against the source.
- `WalrusError::BinaryInput` exists; the enum is `#[non_exhaustive]`, so a new
  `office`-gated error variant is additive.
- FORMAT.md section 5 is per-file; the docs build the whole-container guarantee
  on top of it correctly rather than overstating the existing guarantee.
- FORMAT.md section 7 (b) is open and only scoped, not resolved, by the proposal.
- All four documents are ASCII-clean (box-drawing permitted, none present
  outside fenced blocks anyway).
- requirements.md structurally matches requirements-walrus.md; design.md matches
  the walker-viewer design.md house style (status line, locked constants,
  numbered sections, trailing review-responses log).

## Unverified / wrong assumptions

- None. Every source-level claim the documents make that could be checked against
  the repository was checked and found accurate. The ZIP-crate DEFLATE-backend
  reproducibility (NIT-1) and the exact `zip` minor/MSRV are correctly deferred
  to implementation time by the documents themselves, so they are not design
  assumptions that could be verified now; they are explicitly flagged as
  implementation-time verifications.
