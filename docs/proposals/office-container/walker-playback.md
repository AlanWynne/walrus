# Walker office playback -- view-only, step-through versions (PHASE TWO)

Status: PLANNING (phase two). BLOCKED ON PHASE ONE. This is a Walker-stream
feature: it lives entirely in `crates/walker` and consumes the `walrus-fmt`
office surface produced by the phase-one office-container stream
(`office-container/requirements.md`, `office-container/design.md`). No phase-two
code starts until phase one lands its office reconstruct/extract surface and
signals it API-ready (section 3).

ASCII only (`->`, `<->`, `<=`, `>=`, `--`) per the documentation steering rule.

This document blends requirements and design for ONE bounded Walker feature:
rendering an ODF/OOXML office document VIEW-ONLY and stepping through its stored
versions with the EXISTING player, so the user can WATCH the document change the
same way Walker already steps through text versions. It extends -- never breaks
-- the existing Walker viewer design (`walker-viewer/design.md`) and the Walker
stream requirements (`requirements-walker.md`, FR-K prefix). It introduces a new
FR prefix `FR-WP` (Walker Playback), distinct from the office library's `FR-OC`
and from Walker's existing `FR-K`.

---

## 1. Goal and non-goals

### 1.1 Goal

Let Walker OPEN an archive that holds an office document group (a `.odt`,
`.docx`, etc., unpacked by phase one into a `DocumentGroup`), SELECT that logical
document in the navigator, and STEP THROUGH its document versions with the
existing transport/player. At each document version the content pane shows a
LOW-FIDELITY, in-engine rendering of the document's text structure, so the user
watches paragraphs, headings, and tables appear and change as playback advances.
This is the office analogue of the text playback Walker already does: the player
steps an index, the view renders whatever that index reconstructs.

### 1.2 Non-goals (explicit)

- NOT WYSIWYG. The rendering is a readable, structural approximation, not a
  faithful reproduction of Writer/Word page layout.
- NOT pixel-faithful layout. Exact fonts, margins, columns, page breaks, floats,
  and precise table/line metrics are OUT OF SCOPE. Pixel-perfect Word/Writer
  layout is far-future only and is NOT attempted here.
- NOT editing. The office view is strictly read-only, like every other Walker
  renderer.
- NOT a new export path. Byte-level fidelity of the OUTPUT office file is phase
  one's concern (FR-OC6 byte-identical repack); phase two never repacks for
  display and asserts nothing about output bytes (section 10).
- NOT a change to the format or to `walrus-fmt`'s office logic. Phase two adds
  only Walker-side rendering (section 8, INV-2).

---

## 2. Dependency on phase one (BLOCKING CONTRACT)

Phase-two office playback is one-directional: Walker depends on `walrus-fmt`,
NEVER the reverse. It is BLOCKED until the specific phase-one capability below
lands and is signalled API-ready, mirroring the WALRUS -> WALKER "API READY"
signal pattern in `requirements-walrus.md` section 6 (the dated block that
un-blocks a dependent stream). No phase-two work -- not the renderer, not the
selector extension, not the harness tests -- starts before that signal.

### 2.1 The specific phase-one capability depended on

Named against the real identifiers in `office-container/requirements.md`
(the public surface in its section 1) and `office-container/design.md`:

- `Archive::extract_office(group: &str, v: Version) -> Result<Vec<u8>>`
  (requirements.md section 1; FR-OC6 "byte-identical repack round-trip"). This
  reconstructs each member at the version-aligned index, reconstructs the
  envelope, and returns the office file bytes for document version `v`.
- The read-only grouping accessors, ALWAYS-COMPILED plain-serde surface:
  `Archive::groups(&self) -> impl Iterator<Item = &DocumentGroup>` and
  `Archive::group(&self, sel: &str) -> Option<&DocumentGroup>`
  (requirements.md section 1; FR-OC3 "document-group manifest concept").
- The `DocumentGroup` type and `OfficeKind { Odf, Ooxml }` enum
  (requirements.md section 1), carrying `id`, `name`, `kind`, `member_ids`,
  `envelope_id`, and the `version_map` version-alignment table.
- The VERSION-ALIGNMENT capability (FR-OC4 "version-alignment rule"; design.md
  section 5 "per-group document-version table"): the pure mapping that turns a
  requested document `Version` into the per-member index set, so that
  `extract_office(group, Version::Index(k))` yields the office bytes for the k-th
  document version. This is the capability that makes "office document at version
  k" meaningful; phase-two playback relies on it entirely and reimplements none
  of it.

How Walker asks for "office document at version k", stated in terms of what the
phase-one docs actually define: Walker does NOT reconstruct individual member
parts and does NOT repack. It calls `extract_office(group_name, Version::Index(k))`
and receives the fully reassembled office ZIP bytes for document version k; the
member-at-aligned-index reconstruction, the envelope replay, and the repack all
happen INSIDE phase one (design.md section 7 "byte-identical repack procedure").
Walker then UNPACKS those returned ZIP bytes Walker-side only to read the inner
XML parts it wants to render (section 7). This keeps the version-alignment logic
in exactly one place (phase one) and keeps Walker a pure consumer of office
bytes, identical in spirit to how the text viewer consumes
`reconstruct(file, Version::Index(k))` bytes today.

### 2.2 The blocking contract (mirrors requirements-walrus.md section 6)

- Until `office-container` posts an "API READY" note (branch + tip, in the style
  of requirements-walrus.md section 6's dated "API READY -- 2026-10-11" block)
  stating that `extract_office` / `groups` / `group` / `DocumentGroup` /
  `OfficeKind` are implemented, reviewed, and green, phase-two Walker office
  playback stays BLOCKED. The office library surface is the single source of the
  office read API; Walker MUST NOT guess it or stub a parallel one.
- The dependency is a one-way build edge: `walker` depends on
  `walrus-fmt` with the `office` feature enabled (section 8.2); `walrus-fmt`
  gains NOTHING from Walker (INV-2). If a phase-two need requires an office API
  shape not in `office-container/requirements.md` section 1, raise it to the
  office stream rather than inventing it in Walker (mirrors requirements-walker.md
  section 6's "signal back" rule).
- The office library build must enable its `office` feature for Walker (design.md
  section 0/8 "Cargo feature `office`"); the always-compiled grouping types mean
  `groups()`/`group()` exist even in a default build, but `extract_office`
  requires `features = ["office"]`.

---

## 3. API-ready gate (what un-blocks phase two)

Phase two un-blocks when the office stream signals, in the requirements-walrus.md
section 6 style, that ALL of the following are implemented, reviewed, and green:

1. `extract_office(group, v)` returns byte-complete office bytes for any resolved
   document `Version` (FR-OC6 AC1/AC2/AC3, including an older version via the
   version-alignment table).
2. `groups()` / `group(sel)` resolve a group by logical name or id and expose its
   `name`, `kind`, and `member_ids` (FR-OC3 AC3).
3. The `office` cargo feature on `walrus-fmt` builds and `extract_office` is
   reachable with it enabled (FR-OC8).

Until that note exists, Walker builds only the phase-two parts that need NO office
API: the pure ODF/OOXML XML -> `Vec<Block>` lowering functions (section 6) and
their unit tests, which operate on raw XML byte inputs supplied by the test, not
on `walrus-fmt`. The office RENDERER WIRING and the player-stepping harness test
(which need `extract_office`) wait for the signal. This mirrors
requirements-walker.md section 1's INDEPENDENT-vs-BLOCKED split.

---

## 4. Binding to the existing player (player.rs is UNCHANGED)

The office view binds to the EXISTING player state machine in
`crates/walker/src/player.rs` with NO change to that file. `player.rs` is
content-agnostic: it owns a `usize` `index` in `0..=len`, advances it by
direction/loop/speed (`advance_one`, `tick`, `step_forward`, `step_backward`,
`seek`), and exposes `current_bytes()` which reconstructs via
`Version::Index(self.index)`. It renders NOTHING itself; the GUI reads the index
and renders whatever that index reconstructs. This is confirmed by reading
`player.rs`: the loop/stop/wrap/direction/speed logic is pure arithmetic over
`index`/`len`, and the only reconstruct call is `current_bytes()`.

Phase-two office playback REUSES this unchanged:

- For an office document, the player's `index` is the DOCUMENT-VERSION index of
  the selected group (0 = current, higher = older), exactly as `index` is the
  per-file version index for a text file. `len` is the group's document-version
  count.
- The office content view does NOT call `player.current_bytes()` (that path
  reconstructs a single tracked file). Instead, when bound to an office group, the
  view resolves the current office bytes by calling
  `archive.extract_office(group_name, Version::Index(player.index()))` (section
  2.1). The player still owns and steps the index; only the "turn this index into
  bytes" call differs by content kind -- which is already how a content-aware
  viewer is structured (the viewer chooses a renderer; here it also chooses the
  reconstruct call).
- Because the player is content-agnostic, loop/wrap/direction/speed (the existing
  three player unit tests and `interval_for_multiplier`) apply to office playback
  with zero change. Stepping the player swaps the rendered document version; this
  is asserted by a headless harness test (section 10).

Within the Walker `HistorySource` seam (requirements-walker.md FR-K1), an office
group is modelled as a `FileRef` whose `versions()` are the group's document
versions and whose `reconstruct(version)` returns the office bytes via
`extract_office`. This keeps office playback inside the same seam the text player
already uses; the player never learns it is playing an office document.

---

## 5. Chosen renderer: LOW-FIDELITY IN-ENGINE (reuse the shared Block model)

DECIDED (see section 9): a LOW-FIDELITY IN-ENGINE renderer. Walker parses the
office document's inner XML STRUCTURE and LOWERS it to the viewer's EXISTING
shared `Block`/`Inline` model (`walker-viewer/design.md` sections 2.3 and 3,
`view/richtext.rs`), which the existing egui painter already renders. The office
renderer REUSES that painter; it adds a new lowering front-end, not a new layout
path.

### 5.1 Lowering target (reuse, do not reinvent)

The existing shared model (walker-viewer design section 2.3) is:

```
enum Inline { Text(String), Bold(..), Italic(..), Code(String), Link { text, href } }
enum Block  { Heading(u8, Vec<Inline>), Paragraph(Vec<Inline>), CodeBlock(String),
              List { ordered: bool, items: Vec<Vec<Block>> }, Quote(Vec<Block>),
              Table { head: Vec<..>, rows: Vec<..> }, Rule, ImagePlaceholder(String) }
```

The office lowering produces `Vec<Block>` from the document's main XML part(s),
exactly as `view/html.rs` and `view/markdown.rs` lower HTML and Markdown today.
No new `Block`/`Inline` variants are required for v1: office headings map to
`Block::Heading`, paragraphs to `Block::Paragraph` with `Inline` runs for bold /
italic / code-styled spans, lists to `Block::List`, tables to `Block::Table`,
embedded images to `Block::ImagePlaceholder` (section 7). If a v1 construct has no
clean `Block` home it degrades to a `Block::Paragraph` of its text content (the
same graceful-degradation posture the HTML subset uses).

### 5.2 Document kinds handled at this fidelity

- v1 (REQUIRED): text documents -- ODT (ODF text, `content.xml` with
  `office:text`) and DOCX (OOXML word-processing, `word/document.xml` with
  `w:body`). These map cleanly to the paragraph/heading/list/table block model
  and are RECOMMENDED first because their structure is closest to the shared
  model.
- Lower-priority / DEFERRED: spreadsheets (`.ods` / `.xlsx`) and presentations
  (`.odp` / `.pptx`). Their layout (a grid of sheets/cells; a deck of positioned
  slides) is further from the linear block model: a spreadsheet maps most
  naturally to the existing CSV grid path (walker-viewer design section 2.2, the
  `csv` renderer / `view/csv.rs`) rather than the block model, and a presentation
  needs per-slide framing with no current analogue. These are RISKS/OPEN
  QUESTIONS (section 11), explicitly NOT in v1. The selector (section 5.3) routes
  only text-document office groups to the office renderer in v1; a spreadsheet or
  presentation group is shown with a labelled "office kind not yet rendered"
  placeholder block rather than a wrong rendering.

---

## 6. Renderer selection (extend the existing selector, do not bypass it)

Walker already has a pure `select_renderer(mime: &str, name: &str) -> Renderer`
(`walker-viewer/design.md` section 2.1, `view/selector.rs`). Phase two EXTENDS
this enum and table rather than adding a parallel dispatch path.

### 6.1 Detecting an office document version

The viewer learns a selected document is an office group from the PHASE-ONE
MANIFEST GROUP, not by sniffing bytes:

- When the navigator selection is a `DocumentGroup` (via `archive.group(sel)`),
  the viewer reads the group's `kind` (`OfficeKind::Odf` / `Ooxml`) and `name`
  (its logical document name, e.g. `my-file.odt`) from the phase-one manifest
  group (design.md section 3). The group's logical mime/name is the authority for
  routing -- the office document's own container mime (e.g.
  `application/vnd.oasis.opendocument.text`,
  `application/vnd.openxmlformats-officedocument.wordprocessingml.document`) is
  derived from `kind` + member inspection, not guessed from the extension alone.
- A non-group selection (an ordinary tracked file) continues through the existing
  mime/name resolution unchanged.

### 6.2 Extending the selector enum/table

Add ONE data-carrying variant to the existing `Renderer` enum, keeping illegal
states unrepresentable (rust-standards):

```
enum Renderer {
    Html,
    Markdown,
    Csv,
    Code(CodeLang),
    Office(OfficeDoc),   // NEW: carries which office kind to lower
    Text,
}

enum OfficeDoc { OdfText, OoxmlText }   // v1 kinds; spreadsheet/presentation deferred
```

Selection remains a PURE function so it stays unit-testable without egui:

- A new pure helper `select_office_renderer(kind: OfficeKind, name: &str) ->
  Renderer` maps a group's `kind` + logical name to `Renderer::Office(..)` for a
  v1 text document, or to a deferred-placeholder marker for a spreadsheet /
  presentation (section 5.2). This sits ALONGSIDE `select_renderer`; the viewer
  calls it when the selection is a group and falls back to the existing
  `select_renderer(mime, name)` for a non-group file. The office container mimes
  are added to the EXISTING MIME table as a documented extension so the mapping
  lives in one place:

| Office container MIME (from `kind` + members)                                | Renderer                 |
|------------------------------------------------------------------------------|--------------------------|
| `application/vnd.oasis.opendocument.text`                                    | `Office(OdfText)`        |
| `application/vnd.openxmlformats-officedocument.wordprocessingml.document`    | `Office(OoxmlText)`      |
| ODF spreadsheet/presentation, OOXML spreadsheet/presentation                 | deferred placeholder (v1)|

The selector NEVER bypasses the existing dispatch: `Renderer::Office(..)` flows
through the same `view/mod.rs` selection re-export that the other renderers use
(walker-viewer design section 3).

---

## 7. Untrusted-content safety (consistent with the viewer rule)

Office XML and its members are UNTRUSTED data, exactly like the HTML the viewer
already treats as untrusted (walker-viewer design sections 2.2, 2.4). The office
renderer obeys the same locked rule:

- It PARSES STRUCTURE ONLY. It reads the office XML parts to extract text runs,
  headings, lists, and tables; it never executes anything the document contains.
- Embedded images and other binary members are rendered as LABELLED PLACEHOLDERS
  (`Block::ImagePlaceholder`), never decoded, never displayed as real images in
  v1, and NEVER fetched from a remote URL. (Decoding to a real raster is a
  possible later enhancement with its own untrusted-image review; v1 does not do
  it.)
- Macros / scripts / embedded objects (ODF `Basic` macros, OOXML VBA
  `vbaProject.bin`, OLE objects) are NEVER run and NEVER interpreted. They are
  ignored for rendering (optionally surfaced as a labelled placeholder noting an
  embedded object is present).
- No remote resource referenced by the document (external image href, linked
  template, web-extension URL) is EVER fetched. This matches the viewer's
  no-remote-fetch rule and the locked NO-webview decision (walker-viewer design
  section 2.4).

Because Walker unpacks the ZIP bytes returned by `extract_office` only to read
inner parts, the unpack is a read-only structural parse: a malformed or hostile
office ZIP yields a typed render error (section 9.4 / error handling below), not
a panic and not code execution.

---

## 8. Module decomposition and the dependency boundary (INV-2)

### 8.1 New Walker files (GUI-side only, each < 400 non-test lines)

All new files live under `crates/walker/src/` and are Walker-side only; none
touch `walrus-fmt`. Bodies are NOT written here (planning only); responsibilities:

| File                     | Responsibility                                                                                   |
|--------------------------|--------------------------------------------------------------------------------------------------|
| `src/view/office.rs`     | Office view entry: given office ZIP bytes + `OfficeDoc` kind, unpack the ZIP read-only, pick the main part, call the kind's lowering, return `Vec<Block>` to the shared painter. Dispatch + glue only; no layout logic. |
| `src/view/odf.rs`        | PURE ODF-text lowering: `content.xml` (`office:text`) bytes -> `Vec<Block>`. Headings/paragraphs/spans/lists/tables -> the shared model. No egui. |
| `src/view/ooxml.rs`      | PURE OOXML word-processing lowering: `word/document.xml` (`w:body`) bytes -> `Vec<Block>`. Same target model. No egui. |
| `src/view/selector.rs`   | EXTENDED (existing file): add the `Office(OfficeDoc)` variant, the office MIME rows, and the pure `select_office_renderer` helper (section 6). Stays pure, unit-tested without egui. |

If `odf.rs` or `ooxml.rs` approaches 400 lines, split the inline-run lowering out
into a sibling that still feeds the shared `Block` model, mirroring the
`richtext.rs` split-seam rule in walker-viewer design section 3. The shared
`Block`/`Inline` model and the egui painter in `view/richtext.rs` are REUSED
unchanged (no new painter).

A small ZIP read is needed Walker-side to open the bytes returned by
`extract_office` and pull out the main XML part. This uses a lean, pure-Rust ZIP
reader as a WALKER dependency (candidate: the same `zip` crate family phase one
pins in design.md section 8, confirmed against the workspace MSRV 1.95 at
implementation time). It is a Walker-side dependency ONLY; it is NOT pushed into
`walrus-fmt`.

### 8.2 Dependency boundary (INV-2) -- RECOMMENDED split

RECOMMENDED and chosen: the phase-one library reconstructs the inner XML
parts/bytes (it already must, to repack) and RETURNS office bytes via
`extract_office`; WALKER owns the XML -> `Block` lowering. Justification:

- It keeps `walrus-fmt` LEAN and GUI-free (INV-2, the sacred boundary): the
  library gains no document-model type, no `Block`/`Inline`, no egui, and nothing
  that a playback-only or FFWB CR-NR-107 embedder would have to pay for. The
  library's office concern stays "unpack, version, repack bytes" (office-container
  design.md), nothing about rendering.
- The lowering is a presentation concern, and presentation is Walker-exclusive
  (project-conventions INV-2; requirements-walker.md "export/rendering is
  Walker-side"). Putting a rendering-oriented structured model in `walrus-fmt`
  would blur that boundary and tempt other consumers to depend on a Walker-shaped
  type.
- The lowering functions are pure and testable in Walker without a running egui
  context, matching how the existing HTML/Markdown lowering is tested
  (walker-viewer design section 2.3).

REJECTED alternative: the library exposes a structured office DOCUMENT MODEL
(parsed paragraphs/runs) that Walker renders directly. Rejected because it pushes
office-structure semantics and a rendering-shaped type into `walrus-fmt`,
enlarging the lean embeddable surface for a benefit (shared parsing) that only
Walker needs, and risks the boundary erosion INV-2 exists to prevent. Phase one
already unpacks parts internally for repack; EXPOSING raw part bytes (which
`extract_office` effectively does, as a whole office file Walker re-opens) is
enough. No office/GUI dependency is pushed into `walrus-fmt` by this design.

---

## 9. Functional requirements (FR-WP) with testable acceptance criteria

Prefix `FR-WP` (Walker Playback); it does not collide with office `FR-OC` or
Walker `FR-K`. Each AC is TDD-phrased: write the failing test first, watch it
fail, then implement to green (testing steering). The PRIMARY testable surface is
the pure XML -> `Vec<Block>` lowering (section 10).

### FR-WP1 Office group is selectable and binds to the player -- BLOCKED on section 3
Selecting an office `DocumentGroup` in the navigator binds the existing player to
that group's document-version timeline (section 4), with `len` = the group's
document-version count and index 0 = current.
- AC1 (headless harness) Selecting a fixture office group sets the player `len` to
  the group's document-version count and leaves `index` at 0 (current).
- AC2 (headless harness) `step older` increases the document-version index and
  `step newer` decreases it, reusing the existing index-0-is-current transport
  semantics (requirements-walker.md FR-K3); no player.rs change.
- AC3 (unit) The office group is exposed through the `HistorySource` seam as a
  `FileRef` whose `versions()` length equals the group document-version count
  (tested against a fake source, no `walrus-fmt` needed).

### FR-WP2 ODF-text lowering (`content.xml` -> Vec<Block>) -- INDEPENDENT (pure)
A pure function lowers ODF text `content.xml` bytes to `Vec<Block>` in the shared
model (section 5.1), handling headings, paragraphs, bold/italic/code runs, lists,
and tables.
- AC1 A `content.xml` with `text:h` of outline levels 1 and 2 lowers to
  `Block::Heading(1, ..)` and `Block::Heading(2, ..)` with the heading text.
- AC2 A `text:p` with a `text:span` styled bold lowers to a `Block::Paragraph`
  whose runs include `Inline::Bold(..)` for the styled span and `Inline::Text`
  for the rest.
- AC3 A `text:list` lowers to `Block::List { ordered, items }` with one item per
  `text:list-item`; nested lists lower to nested `Block::List`.
- AC4 A `table:table` lowers to `Block::Table` with the expected row/column cell
  text.
- AC5 An unrecognised ODF element degrades to a `Block::Paragraph` of its text
  content (no panic, no dropped text) -- graceful degradation.
- AC6 All of FR-WP2 is driven test-first with raw `content.xml` byte inputs under
  `tests/fixtures/`; no egui context and no `walrus-fmt` call.

### FR-WP3 OOXML word-processing lowering (`word/document.xml` -> Vec<Block>) -- INDEPENDENT (pure)
A pure function lowers OOXML `word/document.xml` bytes to `Vec<Block>` in the same
shared model, handling the `w:body` structure.
- AC1 A `w:p` with `w:pStyle` naming a heading lowers to `Block::Heading(level,
  ..)` at the mapped level.
- AC2 A `w:p` containing a `w:r`/`w:rPr` bold run lowers to a `Block::Paragraph`
  with `Inline::Bold(..)` for that run.
- AC3 A `w:tbl` lowers to `Block::Table` with the expected cell text.
- AC4 A numbered/bulleted paragraph (`w:numPr`) lowers to a `Block::List` item of
  the correct `ordered` sense.
- AC5 An unrecognised OOXML element degrades to a `Block::Paragraph` of its text
  content (no panic, no dropped text).
- AC6 Driven test-first with raw `word/document.xml` byte inputs under
  `tests/fixtures/`; no egui, no `walrus-fmt`.

### FR-WP4 Image and binary members render as labelled placeholders -- INDEPENDENT (pure)
Embedded images and binary members lower to `Block::ImagePlaceholder` and are
never decoded or fetched (section 7).
- AC1 A document referencing an embedded image lowers that reference to a
  `Block::ImagePlaceholder` carrying a label (e.g. the member path), with no image
  bytes decoded.
- AC2 (unit) The lowering performs no file/network I/O for a placeholder (verified
  by construction: the lowering takes only in-memory XML bytes and returns Blocks;
  there is no I/O call to assert against).

### FR-WP5 Selector routes an office text group to the office renderer -- INDEPENDENT (pure)
`select_office_renderer(kind, name)` maps an ODF/OOXML TEXT group to
`Renderer::Office(OdfText | OoxmlText)` and a spreadsheet/presentation group to
the deferred placeholder; a non-group selection falls back to the existing
`select_renderer` (section 6).
- AC1 `OfficeKind::Odf` + a `.odt` name resolves to `Renderer::Office(OdfText)`.
- AC2 `OfficeKind::Ooxml` + a `.docx` name resolves to `Renderer::Office(OoxmlText)`.
- AC3 An ODF/OOXML spreadsheet or presentation group resolves to the deferred
  placeholder, NOT to a text office renderer (v1 scope guard).
- AC4 The function is pure and unit-tested without egui.

### FR-WP6 Stepping the player swaps the rendered document version -- BLOCKED on section 3
With an office group bound, advancing the player one document version re-renders
the content pane from the newly reconstructed office version.
- AC1 (headless harness) With a two-document-version office fixture, the rendered
  `Vec<Block>` at index 0 differs from the rendered `Vec<Block>` at index 1 in the
  expected way (e.g. a paragraph added in the newer version is present at index 0
  and absent at index 1), proving the player index drives the rendered version via
  `extract_office`.
- AC2 (headless harness) Playing forward then backward returns to the original
  rendered block structure at index 0 (reuses the player loop/wrap logic
  unchanged).
- AC3 The office bytes feeding the render come from
  `archive.extract_office(group, Version::Index(player.index()))`; no member
  reconstruction or repack happens Walker-side (section 2.1).

### FR-WP7 Untrusted office content is safe -- INDEPENDENT (pure) + harness
The renderer never executes macros/scripts, never fetches remote resources, and
renders images as placeholders (section 7).
- AC1 (unit) A fixture document carrying an embedded macro/script member lowers to
  Blocks with NO execution and no placeholder that implies execution; the macro
  member is ignored or shown as a labelled "embedded object" placeholder.
- AC2 (unit) A document referencing a remote image URL lowers to a
  `Block::ImagePlaceholder`; no network call is made (by construction, section
  7 / FR-WP4 AC2).
- AC3 A malformed/hostile office ZIP returned to the office view yields a typed
  render error surfaced in the status bar, never a panic (section 9.4).

### 9.4 Error handling (concrete, per operation)

- `extract_office` returns `Err`: office library failure (bad group, version out
  of range, non-reproducible member). RECOVERABLE at the UI level -- the content
  pane shows an error message and the status bar reports it (reusing the existing
  Walker error surface, walker-viewer design section 5 status bar); playback does
  not crash and the user can seek elsewhere. Logged at the Walker app level with
  `anyhow` context (`walker` is a binary; rust-standards app-code rule).
- ZIP unpack of the returned office bytes fails (malformed container): typed
  Walker-side render error -> content pane shows "cannot read office document",
  status bar notes it. RECOVERABLE; not fatal to the app.
- Main part missing (`content.xml` / `word/document.xml` not found): render a
  single `Block::Paragraph` error note ("office main part not found"); not fatal.
- XML parse error inside the main part: lower what parsed, append a trailing
  `Block::Paragraph` noting truncation; never panic, never drop silently with
  `let _` (rust-standards). Partial render is acceptable for a view-only tool.
- The lowering functions themselves return `Result<Vec<Block>, _>` (or lower on a
  best-effort basis returning `Vec<Block>` with an embedded error block) so a
  parse failure is data, not a panic; the choice (Result vs best-effort block) is
  fixed at implementation as best-effort-with-error-block to match the
  graceful-degradation ACs (FR-WP2 AC5 / FR-WP3 AC5).

### 9.5 Input validation (per external input)

- Office bytes from `extract_office`: treated as UNTRUSTED (section 7). No size
  cap is imposed by this design beyond what the ZIP reader enforces; a RISK on
  very large documents during fast playback is noted (section 11). Required: the
  bytes must be a readable ZIP; on failure, the error path above.
- Group selector string (`group(sel)`): optional/typed by the navigator; an
  unresolved selector yields no office view (fall back to the normal file path).
- XML element/attribute values: parsed structurally; unknown elements degrade to
  text (FR-WP2/3 AC5). No value is interpreted as a path, URL to fetch, or code.

---

## 10. Testing strategy (per testing steering)

- PRIMARY (pure, no egui, test-first): the ODF/OOXML XML -> `Vec<Block>` lowering
  functions (`view/odf.rs`, `view/ooxml.rs`) and the pure selector helper
  (`view/selector.rs`). These are unit-tested with raw XML byte fixtures under
  `tests/fixtures/`, asserting the produced `Vec<Block>` structure (headings,
  paragraph runs, lists, tables, image placeholders, graceful degradation). This
  is the bulk of the behaviour and needs no running egui context, matching the
  existing HTML/Markdown lowering tests (walker-viewer design section 2.3).
- HEADLESS egui_kittest (deterministic, bounded frames): the view wiring and that
  stepping the player SWAPS the rendered version (FR-WP1, FR-WP6). The harness
  runs a fixed, bounded number of frames, asserts on model state / rendered block
  structure (not pixels, not frame counts), per the testing steering's GUI rule.
  The office group is driven through the `HistorySource` seam with a fixture
  archive; the assertion is on the rendered `Vec<Block>` or the model index, not
  on appearance.
- WHAT PHASE TWO ASSERTS vs NOT: phase two asserts the RENDERED BLOCK STRUCTURE
  matches the reconstructed XML for a given document version. It does NOT assert
  byte-level fidelity of any OUTPUT office file -- that is phase one's
  byte-identical repack concern (FR-OC6), out of scope here (section 1.2).
- MANUAL (only where genuinely non-headless, each with a stated why): pixel-exact
  appearance of the rendered office page (a GPU/pixel concern egui_kittest does
  not assert -- WHY: visual fidelity is subjective and pixel comparison is brittle
  across platforms); the `rfd` native dialog if an office group is ever opened via
  OS file picker (WHY: OS-native dialog is not headless-drivable, matching
  requirements-walker.md NFR-3). No functional behaviour is left manual.
- Fixtures: small, captured ODT/DOCX inner XML parts (and tiny two-version office
  archives for the harness) live under `tests/fixtures/`; tests are deterministic
  (no HashMap iteration order, no system time, no files outside `tests/fixtures/`),
  per NFR / testing steering.

---

## 11. Risks and open questions

- SPREADSHEET / PRESENTATION FIDELITY: `.ods`/`.xlsx` and `.odp`/`.pptx` map
  poorly to the linear block model. Open: route spreadsheets to the existing CSV
  grid path (section 5.2) vs a dedicated sheet view; presentations likely need a
  per-slide framing with no current analogue. DEFERRED past v1; v1 shows a labelled
  placeholder for these kinds.
- LARGE-DOCUMENT RENDER PERFORMANCE DURING FAST PLAYBACK: at high speed the player
  may request a new office version every few frames; each request is an
  `extract_office` call plus a ZIP unpack plus a full lowering. Open: whether to
  cache the lowered `Vec<Block>` per document-version index, debounce rendering
  during fast playback, or cap document size. Not solved in v1; noted so the
  harness performance assumptions stay bounded.
- HOW MUCH ODF/OOXML STRUCTURE TO LOWER IN v1: the minimal useful set is
  headings/paragraphs/runs/lists/tables/image-placeholders (section 5.1). Open:
  whether footnotes, comments, tracked-changes markup, fields, and section breaks
  are worth lowering, or left to degrade to text. v1 lowers the minimal set and
  degrades the rest (FR-WP2/3 AC5).
- WHETHER IMAGE PLACEHOLDERS SUFFICE: v1 renders images as labelled placeholders
  (section 7, FR-WP4). Open: whether later decoding to real rasters (with a
  dedicated untrusted-image review) is wanted; it is NOT in v1.
- OFFICE CONTAINER MIME AUTHORITY: deriving the container mime from `kind` +
  member inspection (section 6.1) assumes the phase-one group records enough to
  distinguish text vs spreadsheet vs presentation. Open: confirm at API-ready time
  that `DocumentGroup.kind` + members are sufficient, else request a finer kind
  from the office stream (section 2.2 "signal back" rule).

---

## 12. Alternatives / decision record

Three fidelity options were evaluated for how Walker renders an office document
version. The chosen option is recorded here and as ADR-7 in
`office-container/decisions.md`.

### Option 1 -- LOW-FIDELITY IN-ENGINE (RECOMMENDED / CHOSEN)

Parse the office XML structure Walker-side and lower it to the EXISTING shared
`Block`/`Inline` model, painted by the existing egui painter (sections 5-6).

- Pros: reuses the viewer's painter and untrusted-safe posture; pure, testable
  lowering (the bulk of the work needs no egui); keeps `walrus-fmt` lean (Walker
  owns lowering, library just returns bytes, INV-2); fast enough for stepping;
  composes with the existing content-aware viewer and selector.
- Cons: not WYSIWYG; approximate layout; spreadsheets/presentations deferred.
- CHOSEN because it matches the view-only, step-to-watch goal (section 1) at low
  cost and respects every locked viewer decision and the dependency boundary.

### Option 2 -- HIGH-FIDELITY VIA EXTERNAL CONVERTER (REJECTED)

Shell out to LibreOffice headless (`soffice --convert-to pdf|html`) per document
version and display the converted result.

- REJECTED: a heavy external runtime (a full LibreOffice install) is at odds with
  Walker's lean posture and the project's lean-embeddable ethos; conversion is
  slow (hundreds of ms to seconds per version), which breaks step-through
  playback where versions change every few frames; it adds an external process
  and filesystem round-trip per step; and it is an operational dependency the
  rest of the toolkit does not have. Not acceptable for an interactive player.

### Option 3 -- EMBEDDED WEBVIEW / FULL LAYOUT ENGINE (REJECTED)

Convert each version to HTML and render it in an embedded webview (or embed a
full CSS layout engine) for near-WYSIWYG output.

- REJECTED by reference to the ALREADY-LOCKED walker-viewer design section 2.4
  "Webview decision (locked)": a webview is a heavy native dependency with a
  platform-specific runtime (WebView2 / WebKitGTK); it would execute arbitrary
  scripts and fetch remote resources from UNTRUSTED stored documents, directly
  violating the untrusted-content rule; and it does not compose with egui's
  single-surface immediate-mode rendering. The same reasoning that rejected a
  webview for HTML rejects it for office documents. A full in-process layout
  engine carries the same weight without the script risk and is still far beyond
  the view-only goal.

---

## 13. Review responses

(placeholder for the design-review loop -- findings and their resolutions are
recorded here on the revision iteration.)
