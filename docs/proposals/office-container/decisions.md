# Office container -- decision record (ADR)

Status: DECISIONS (planning). A concise, ADR-style record of the choices that
frame the office-container feature. Fuller reasoning is in `design.md`
(Alternatives / decision record, section 9) and the normative spec text is in
`format-change-proposal.md`. Requirements live in `requirements.md`.

ASCII only (`->`, `<->`, `<=`, `>=`, `--`) per the documentation steering rule.

---

## ADR-1 Containerisation strategy = option 2 (per-part grouped in the outer archive)

- Context: An ODF/OOXML file is a ZIP of XML parts plus binary resources. Three
  ways to store it were evaluated: (1) a single inner tar tracked as one blob;
  (2) each inner member tracked as its own file in the outer archive, grouped;
  (3) each office document as its own nested `.wal` stored opaquely in the outer
  archive.
- Decision: option 2 -- unpack and track each member as its own tracked file,
  tied by a new additive manifest document group with a version-alignment table
  and a per-document envelope recipe.
- Consequences: text XML parts get clean per-part splice history (the point of
  unpacking); unchanged members cost nothing; needs three additive, backward-
  compatible ingredients (group concept, envelope recipe, version alignment);
  not a format rewrite.
- Alternatives rejected: option 1 (single inner tar) loses per-part diffing --
  tar 512-byte headers + NUL padding make the blob binary (diff rejects it) and
  one edit shifts all later offsets; option 3 (nested `.wal`) cannot delta the
  compressed inner archive at the outer level and risks double compression, so
  outer-level document versioning is not possible.
- Authority: owner chose option 2 ("keeps all the features without too much
  cost") and said "I will accept your recommendations on this analysis"; the
  recommendation agrees.

## ADR-2 Identity = Option A (byte-identical repack via an envelope recipe)

- Context: ZIP packing is not canonical (DEFLATE level/strategy, central-directory
  order, timestamps, CRC32, version fields, data descriptors, ODF's
  mimetype-first-and-stored rule). Re-zipping from parts would change the bytes.
- Decision: Option A -- record a per-member envelope recipe so repack reproduces
  the original office file byte-for-byte, preserving the FORMAT.md section-5 /
  INV-3 byte-identical guarantee for the whole office container.
- Consequences: an envelope recipe (small, text, stored as a tracked file that
  diffs cheaply between saves) is required; repack writes recorded header fields
  verbatim. For a DEFLATE member the writer VERIFIES at ingest that re-DEFLATE
  reproduces the original compressed bytes; where it cannot, the writer stores
  the member's ORIGINAL COMPRESSED bytes verbatim (`stored_compressed`, size cost,
  byte-identity kept) or FAILS with a typed error. It NEVER re-emits an
  originally-DEFLATEd member as STORED (that changes the method field and
  compressed bytes and would silently downgrade to Option B), so byte-identity is
  preserved in every success case.
- Alternatives rejected: Option B (semantic-only identity, re-zip from parts)
  downgrades the guarantee to "same logical document, different bytes", breaking
  the trust property that makes Walrus a safe round-trip store.

## ADR-3 Office scope = ODF + OOXML only; OLE2 deferred

- Context: ODF (`.odt`/`.ods`/`.odp`...) and OOXML (`.docx`/`.xlsx`/`.pptx`) are
  ZIP containers of XML + resources. Legacy `.doc`/`.xls`/`.ppt` are OLE2 / CFBF
  compound BINARY files, not ZIP.
- Decision: support ZIP-based ODF and OOXML; detect and REJECT OLE2 as
  out-of-scope with a typed error; defer OLE2 to a possible future stream.
- Consequences: detection keys on content (ZIP magic, ODF `mimetype`, OOXML
  `[Content_Types].xml`); an OLE2 file is never misclassified or force-fit.
- Alternatives rejected: adding an OLE2 reader now -- a large, separate effort
  that the unpack-to-XML-parts model does not fit; out of scope for this feature.

## ADR-4 Heavy logic behind an optional `office` cargo feature (dependency boundary)

- Context: INV-2 keeps `walrus-fmt` GUI-free and LEAN so it stays embeddable
  (FFWB's CR-NR-107 consumer). A ZIP codec and office-format awareness are heavy.
- Decision: gate the ZIP (un)pack, envelope, ingest, and repack logic behind a
  new optional `office` feature (MAY later split `container-odf` /
  `container-ooxml`), mirroring the existing `diff` / `i18n` / `export-*`
  gating. The chosen ZIP crate is `optional = true`. The plain-serde grouping
  TYPES and read accessors MAY be always-compiled (no heavy dependency), so a
  default build still parses an office archive and sees the raw members.
- Consequences: the default `cargo build -p walrus-fmt` links no ZIP/office
  crate (proved by `cargo tree` default vs `--features office`); a playback-only
  embedder pays nothing; a CLI built without the feature reports a clear "office
  support not built" error, never a panic.
- Alternatives rejected: making the ZIP codec non-optional -- violates INV-2 and
  the lean-embeddable posture.

## ADR-5 Binary members depend on the FORMAT.md section-7 binary-insert decision

- Context: binary members (images/fonts/thumbnails) cannot use the splice path --
  `diff.rs` rejects NUL bytes (`WalrusError::BinaryInput`) and
  `Op::Splice.insert` is a UTF-8 `String` (confirmed in the source).
- Decision: store binary members by DEDUP-BY-CONTENT ATTACHMENT (store each
  distinct body once as a `blobs/<hash>` member, reference by content hash from
  the envelope) -- this needs NO splice-record change and ships in 0.1. Keep
  WHOLE-BLOB VERSIONED binary members as a later enhancement that DEPENDS on
  FORMAT.md section 7 open question (b) (a binary / base64 `insert` form) being
  resolved first.
- Consequences: unchanged binaries are stored once and referenced across versions
  (dedup win immediately); changed binaries are new blobs; ingest never diffs a
  binary member and never aborts the whole ingest on one; the whole-blob path is
  explicitly gated on the spec decision.
- Alternatives rejected: forcing binary through the splice path (impossible under
  INV-4 today); storing a full copy of every binary per version (wasteful,
  defeats dedup).

## ADR-6 Spec-first ordering (INV-1)

- Context: INV-1 makes FORMAT.md the source of truth; code must not define a
  format the spec has not defined.
- Decision: the document-group, envelope-recipe, version-alignment, and
  binary-member rules are written into FORMAT.md FIRST (via
  `format-change-proposal.md`, pasted in and reviewed) BEFORE any office code
  lands. The additions are OPTIONAL and backward-compatible, so old archives
  still conform and group-unaware readers degrade safely.
- Consequences: delivery order is spec -> additive types -> ingest -> repack ->
  CLI -> fixtures/tests (requirements.md section 5); the implementation phase
  begins by landing the spec revision.
- Alternatives rejected: coding the group/envelope shape first and
  back-documenting it -- violates INV-1 and risks code and spec diverging.

## ADR-7 Walker office playback is phase two (view-only, low-fidelity in-engine), dependent on phase one

- Context: the owner asked whether Walker could step through an office document's
  versions in a view-only form, the way it already steps through text versions.
  Office storage (unpack / version / byte-identical repack) is the phase-one
  office-container feature (ADR-1..ADR-6) and lives in `walrus-fmt`; the Walker
  playback view is a SEPARATE, later Walker-stream feature. The requirements and
  design for it are in `office-container/walker-playback.md` (FR-WP prefix).
- Decision: Walker office playback is PHASE TWO and BLOCKED ON PHASE ONE. It
  starts only after the phase-one office surface -- `Archive::extract_office(group,
  Version)` (FR-OC6), `groups()`/`group()` + `DocumentGroup` + `OfficeKind`
  (FR-OC3), and the version-alignment table (FR-OC4) -- lands and is signalled
  API-ready, mirroring the WALRUS -> WALKER "API READY" pattern in
  `requirements-walrus.md` section 6. The view is VIEW-ONLY (not WYSIWYG, not
  editing, not pixel-faithful layout). The renderer is LOW-FIDELITY IN-ENGINE: it
  lowers the office document's inner XML to the EXISTING shared `Block`/`Inline`
  model (`walker-viewer` design sections 2.3/3) painted by the existing egui
  painter, reusing the viewer rather than adding a new layout path. It binds to
  the EXISTING, UNCHANGED `player.rs` state machine (the player steps a
  document-version index; the office view renders whatever that index
  reconstructs via `extract_office`). v1 targets TEXT documents (ODT / DOCX);
  spreadsheets and presentations are deferred.
- Consequences: `walrus-fmt` stays LEAN and GUI-free (INV-2) -- the library just
  reconstructs and returns office bytes; WALKER owns the XML -> `Block` lowering,
  so no office/GUI/rendering type is pushed into `walrus-fmt`. New rendering code
  (`view/office.rs`, `view/odf.rs`, `view/ooxml.rs`, and an extended
  `view/selector.rs`) is Walker-side only, each under 400 non-test lines. Office
  XML is untrusted: structure is parsed only, images render as labelled
  placeholders (never decoded or fetched), macros/scripts are never run --
  consistent with the viewer's untrusted-content rule. The pure XML -> `Vec<Block>`
  lowering is the primary test surface (unit-tested without egui); headless
  `egui_kittest` covers that stepping the player swaps the rendered version.
- Alternatives rejected: a HIGH-FIDELITY EXTERNAL CONVERTER (LibreOffice headless
  `soffice --convert-to`) -- a heavy external runtime, too slow per version for
  step-through playback, at odds with the lean posture. An EMBEDDED WEBVIEW /
  FULL LAYOUT ENGINE -- rejected by reference to the already-locked `walker-viewer`
  design section 2.4 webview decision (untrusted script execution, remote fetches,
  heavy platform-specific runtime, does not compose with egui). Exposing a
  STRUCTURED office model from `walrus-fmt` for Walker to render -- rejected
  because it enlarges the lean embeddable surface with a rendering-shaped type
  only Walker needs, eroding the INV-2 boundary.
- Authority: owner's phase-two directive -- "part of the docs would be enhancing
  walker to be able to render odt documents in a view only format so that we can
  step through them as they change ... it will be dependant on the first part
  working first." The recommendation agrees: view-only low-fidelity in-engine,
  blocked on phase one. This ADR appends to ADR-1..ADR-6 and alters none of them.
