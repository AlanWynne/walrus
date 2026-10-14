# Design review -- Walker office playback (PHASE TWO) + ADR-7

Reviewer: design-review subagent (fresh read, no authoring context).
Deliverables under review:
- `docs/proposals/office-container/walker-playback.md`
- ADR-7 appended to `docs/proposals/office-container/decisions.md`

Source docs read to verify claims:
- `docs/proposals/office-container/requirements.md` (FR-OC1..FR-OC10, section 1 API surface)
- `docs/proposals/office-container/design.md` (sections 3, 4, 5, 7, 8 -- group schema,
  envelope, version-alignment, repack, module/feature plan)
- `.agents/tasks/walker-viewer/design.md` (sections 0, 2.1, 2.3, 2.4, 3, 5)
- `.agents/tasks/requirements-walker.md` (FR-K1 HistorySource trait, FR-K3)
- `.agents/tasks/requirements-walrus.md` section 6 (the "API READY" signal block)
- `crates/walker/src/player.rs` (content-agnostic player)

Verdict: APPROVED (no HIGH, no MEDIUM findings; LOW findings do not block).

---

## Findings

### 1. LOW -- `HistorySource` binding is described with method call shapes the trait does not have

Location: `walker-playback.md` section 4, final paragraph ("an office group is
modelled as a `FileRef` whose `versions()` are the group's document versions and
whose `reconstruct(version)` returns the office bytes").

Problem: verified against `requirements-walker.md` FR-K1, the real trait is:

```
trait HistorySource {
    fn versions(&self, f: &FileRef) -> Vec<VersionId>;
    fn reconstruct(&self, f: &FileRef, v: &VersionId) -> Result<Vec<u8>>;
    ...
}
```

`versions()` and `reconstruct()` are methods on `HistorySource` that TAKE a
`&FileRef`, not methods ON `FileRef`. The doc's phrasing ("a `FileRef` whose
`reconstruct(version)` ...") reads as if `FileRef` carried those methods. The
intent (model the office group behind the same seam) is correct and the
`VersionId::Index` mapping is sound; only the phrasing is loose.

Concrete fix: reword to the trait's real shape, e.g. "the office group is exposed
through the `HistorySource` seam as a `FileRef`; the source's
`versions(&group_ref)` returns the group's document versions and
`reconstruct(&group_ref, VersionId::Index(k))` returns the office bytes via
`extract_office`." Note the trait uses `VersionId` (walker-side), distinct from
`walrus_fmt::Version`; `WalrusSource` maps between them. FR-WP1 AC3's phrasing
("a `FileRef` whose `versions()` length") carries the same slip and should match.

### 2. LOW -- Section 4 states the office view does NOT call `player.current_bytes()` while section on the seam routes reconstruction through `HistorySource`

Location: `walker-playback.md` section 4, bullet 2 ("The office content view does
NOT call `player.current_bytes()` ... the view resolves the current office bytes
by calling `archive.extract_office(...)`") versus the final paragraph routing the
group through the `HistorySource` seam.

Problem: the two descriptions use two different reconstruction entry points -- a
direct `archive.extract_office(...)` call in the content view, and the
`HistorySource::reconstruct` seam. Both end at `extract_office`, so there is no
behavioural contradiction, but the document should state ONE path as normative so
the implementer does not wire both. The seam path is the one consistent with
FR-K1/FR-K8 ("renderers reference ONLY the trait, not `walrus_fmt` types"); a
direct `archive.extract_office` call from the view would reintroduce a
`walrus_fmt` dependency the seam exists to isolate.

Concrete fix: make the seam the single normative path -- the office view obtains
bytes from `HistorySource::reconstruct(&group_ref, VersionId::Index(player.index()))`,
and `WalrusSource` is the one place that calls `archive.extract_office`. Keep the
"player still owns and steps the index" point; drop the implication that the view
itself calls `archive.extract_office`.

### 3. LOW -- `current_bytes()` characterised as reconstructing "a single tracked file"

Location: `walker-playback.md` section 4, bullet 2.

Problem: verified against `player.rs`, `current_bytes()` is
`self.file.reconstruct(Version::Index(self.index))` over a `WalrusFile` (one
opened file), which is accurate for today's single-file player. In the multi-file
`HistorySource` world (FR-K2/FR-K8) reconstruction already goes through
`HistorySource::reconstruct(&FileRef, &VersionId)`, so leaning on
`player.current_bytes()` as the contrast point is slightly out of date. Minor; the
core claim (the player is content-agnostic and owns only the index) is correct and
is confirmed by `player.rs` (pure arithmetic over `index`/`len`, single
reconstruct call).

Concrete fix: contrast against the seam's `reconstruct` rather than the legacy
`Player::current_bytes`, consistent with finding 2.

---

## Criteria assessment

(a) Phase-one dependency is a REAL blocking contract naming ACTUAL identifiers:
PASS. Section 2.1 / 2.2 / 3 and ADR-7 name `extract_office(group, Version) ->
Result<Vec<u8>>` (requirements.md section 1 + FR-OC6), `groups()` / `group(sel)`
(section 1 + FR-OC3), `DocumentGroup`, `OfficeKind { Odf, Ooxml }` (section 1),
and the version-alignment table (FR-OC4 / design.md section 5). Every identifier
matches the phase-one docs verbatim. No invented API name. `Archive::extract_office`
return type `Result<Vec<u8>>` matches requirements.md section 1. No HIGH.

(b) Consistency with walker-viewer LOCKED decisions: PASS. No webview (section 7
and section 12 Option 3 reject it by reference to walker-viewer section 2.4 --
verified that section exists and says exactly that). Untrusted-content handling
(section 7) mirrors walker-viewer sections 2.2/2.4. Shared `Block`/`Inline` reuse
(section 5.1) quotes the real model from walker-viewer section 2.3 accurately.
egui 0.36 is implied via reuse of the existing painter; the doc does not restate
the pin, which is fine since it adds no new egui crate. 400 non-test line rule
stated in section 8.1 and ADR-7. The MSRV 1.95 reference (section 8.1) matches
design.md section 0 / walker-viewer section 0.

(c) INV-2 respected: PASS. Section 8.2 explicitly RECOMMENDS and CHOOSES
library-returns-bytes / Walker-owns-XML->Block lowering, and REJECTS exposing a
structured office model from `walrus-fmt`, with the INV-2 justification. The
Walker-side `zip` read is scoped as a Walker-only dependency, not pushed into
`walrus-fmt`. Consistent with design.md section 8's library-side `zip` gating
(the two `zip` uses are independent, one per crate, which is acceptable).

(d) Fidelity decision explicit with all three alternatives: PASS. Section 12 and
ADR-7 record Option 1 (low-fidelity in-engine) CHOSEN, Option 2 (external
converter / LibreOffice headless) REJECTED with reasons, Option 3 (webview / full
layout engine) REJECTED by reference to walker-viewer section 2.4. WYSIWYG and
pixel-faithful layout are explicitly OUT OF SCOPE (section 1.2).

(e) ACs testable and TDD-phrased: PASS. FR-WP2/FR-WP3/FR-WP4/FR-WP5 are pure
XML->`Vec<Block>` / selector units marked INDEPENDENT, test-first, no egui, no
`walrus-fmt`. FR-WP1/FR-WP6 use headless `egui_kittest` with bounded frames and
model/block-structure assertions (not pixels/frame counts), matching the testing
steering GUI rule. MANUAL items (section 10) are limited to pixel-exact appearance
and the `rfd` OS dialog, each with a stated WHY. The pure lowering is named as the
PRIMARY testable surface.

(f) ASCII-only Markdown: PASS. `grep` for non-ASCII (excluding the allowed
box-drawing range U+2500-U+257F) found NO matches in either `walker-playback.md`
or `decisions.md`. The docs use ASCII `->`, `<->`, `<=`, `>=`, `--` throughout.

(g) ADR-7 appended correctly: PASS. ADR-1..ADR-6 are unchanged (verified by full
read). ADR-7 follows the same Context / Decision / Consequences / Alternatives
rejected / Authority structure, is placed after ADR-6, and explicitly states "This
ADR appends to ADR-1..ADR-6 and alters none of them."

---

## Verified assumptions

- `Archive::extract_office(group: &str, v: Version) -> Result<Vec<u8>>` exists in
  phase-one requirements.md section 1 and is tied to FR-OC6. CONFIRMED.
- `groups()` / `group(sel)` read accessors and the `DocumentGroup` /
  `OfficeKind { Odf, Ooxml }` types exist in requirements.md section 1 and are
  ALWAYS-COMPILED plain serde data (requirements.md "Boundary note"; design.md
  section 3/8). CONFIRMED.
- The version-alignment table is FR-OC4 and design.md section 5's "per-group
  document-version table" (`version_map`). The walker-playback description of it
  (document version k -> per-member index set, index 0 = current) matches design.md
  section 5. CONFIRMED.
- The byte-identical repack procedure is design.md section 7 (referenced as
  "design.md section 7 byte-identical repack procedure"). CONFIRMED the section
  exists and matches.
- walker-viewer section 2.4 is the LOCKED "Webview decision (locked)". CONFIRMED.
- walker-viewer section 2.1 is `select_renderer(mime, name)` as a pure function
  and `Renderer` is a data-carrying enum. CONFIRMED; the proposed `Office(OfficeDoc)`
  variant and `select_office_renderer` helper extend it consistently.
- walker-viewer section 2.3 shared `Block`/`Inline` model is quoted accurately
  (Heading/Paragraph/CodeBlock/List/Quote/Table/Rule/ImagePlaceholder;
  Text/Bold/Italic/Code/Link). CONFIRMED.
- egui/eframe 0.36, egui_kittest 0.36, MSRV 1.95 are the locked stack
  (walker-viewer section 0; design.md section 0). CONFIRMED.
- `player.rs` is content-agnostic: owns a `usize index` in `0..=len`, pure
  loop/wrap/direction/speed arithmetic (`advance_one`, `tick`, `step_forward`,
  `step_backward`, `seek`), single reconstruct call via `current_bytes()` ->
  `Version::Index(self.index)`. CONFIRMED by reading the source.
- requirements-walrus.md section 6 carries the "API READY -- 2026-10-11" dated
  block style the doc mirrors. CONFIRMED.
- FR-K1 defines the `HistorySource` trait with `versions(&self, &FileRef)` and
  `reconstruct(&self, &FileRef, &VersionId)`. CONFIRMED (and is the basis for
  finding 1/2/3).
- ADR-1..ADR-6 are intact and unchanged. CONFIRMED by full read of decisions.md.

## Unverified / wrong assumptions

- WRONG (minor): walker-playback section 4 implies `FileRef` carries
  `versions()` / `reconstruct(version)` methods and that the office view calls
  `archive.extract_office` directly. The real trait places those methods on
  `HistorySource` taking `&FileRef`, and FR-K8 keeps renderers off `walrus_fmt`
  types. See findings 1 and 2 (both LOW; the end state -- reconstruct via the seam
  which calls `extract_office` -- is correct).
- UNVERIFIED (acknowledged by the doc itself): that `DocumentGroup.kind` + member
  inspection suffice to distinguish text vs spreadsheet vs presentation for mime
  routing. The doc flags this as an open question in section 11 ("OFFICE CONTAINER
  MIME AUTHORITY") to confirm at API-ready time, which is the correct handling --
  not a defect.
- UNVERIFIED (correctly deferred): the Walker-side `zip` crate choice is left to
  "the same `zip` crate family phase one pins in design.md section 8, confirmed
  against MSRV 1.95 at implementation time." design.md section 0/8 pins
  `zip = "2"` for the library; deferring the exact Walker-side pin to
  implementation time matches the project's pin-at-impl convention and is not a
  gap.
