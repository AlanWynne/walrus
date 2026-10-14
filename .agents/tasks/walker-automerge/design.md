# Walker Phase 2 design -- Automerge support (FR-K11)

Status: DESIGN (Phase 2, Phase A step 2), REVISION 3 after a second design
review (`design-review.md` / `design-review.json`, verdict CHANGES_REQUESTED,
1 HIGH + 2 NIT -- all localised to section 7.4's byte-fetch-site count and a
one-line note in section 1.2). Revision 2 cleared the earlier 3 HIGH + 3 MEDIUM
findings. Scoped to the `walker` egui GUI crate only. Implements the
requirements in `requirements.md` (8 ACs, INV-2 feature gating,
structured-vs-text, the additive `VersionId::Heads` extension). Grounded in the
SHIPPED Phase-1 code read for this design: `source/mod.rs` (the trait and the
two in-memory sources), `source/walrus.rs` (the adapter pattern this mirrors),
`player.rs`, `transport.rs`, `app.rs`, `app_mutate.rs`, `chrome.rs`,
`view/present.rs`, `view/selector.rs`, `history_view.rs`, and
`walker/Cargo.toml`. The automerge 0.12.0 API is CONFIRMED against the extracted
crate source (`.gate/automerge-api-0.12.md`), not assumed.

ASCII only (`--`, `->`, `<->`, `<=`) per the documentation steering rule;
code sketches use ASCII `// === Name ===` separators.

## Changes since revision 2 (review responses are at the end of this document)

- Section 7.4 and the overview: the byte-fetch-site count is corrected from
  "four" to the THREE sites that actually exist in the shipped
  `crates/walker/src/app.rs`, now cited by line number -- `app.rs:156`
  (`refresh_bytes`), `app.rs:245` (`logic`, inside `if version_changed`), and
  `app.rs:366` (`ui` CentralPanel, inside `if changed`). The fictitious fourth
  "duplicated transport-controls match" item is deleted. The collapse into a
  single `refresh_bytes(&mut self)` is unchanged; it now replaces three sites,
  not four (review-2 finding 1 HIGH, finding 3 NIT).
- Section 1.2: a one-line note records that `source/walrus.rs:168` `file_info`
  constructs `VersionId::Index(0)` internally; it is an internal walrus-only
  construction, unaffected by the new variant, and left as-is (review-2
  finding 2 NIT).

## Changes since revision 1

- Section 1.2: the `VersionId` match-site enumeration is re-derived from the
  shipped code, with the real `app.rs:378` line quoted and the `app_mutate.rs:161`
  site corrected to the EXPORT path over `&dyn HistorySource` (finding 1).
- Sections 1.2 and 7.2: `source.version_at(active, player.index())` is the
  SINGLE specified mechanism for producing the viewer/export `VersionId`; the
  "indexed by `versions()`" wording is deleted (finding 2).
- Section 6: every automerge API name is now CONFIRMED against the pinned
  0.12.0 source; reconstruct reads IN PLACE via the `ReadDoc` `*_at` methods
  (there is no view/fork/snapshot object), and the timestamp is unix SECONDS
  with NO division (findings 3 and 8).
- Section 3.3 + new section 8.1: the structured status-bar label is wired via a
  defaulted trait method `status_note` plus a new `StatusInfo.note` field
  (finding 4).
- Sections 7.3 and 7.4: the status bar and export now read `file_name`/`mime`/
  `document_name` from the active source via `content_hint`/`file_info`, not
  from `player.manifest()`, and the three `player.current_bytes()` byte-fetch
  sites are enumerated and collapsed into one `refresh_bytes(&mut self)`
  (findings 5 and 6).
- Section 6: the `#[path]` precedent is cited accurately as a NEW module alias
  (finding 7).

## Overview

Phase 1 shipped Walker coded to the format-agnostic `HistorySource` trait with
`WalrusSource` as the only implementation. Phase 2 adds a SECOND, read-only
implementation, `AutomergeSource`, in a new feature-gated module
`crates/walker/src/source/automerge.rs`, mirroring the `source/walrus.rs`
adapter pattern. The adapter wraps a loaded `automerge::Automerge` document,
exposes it as a single-`FileRef` source, linearises the change DAG into the
Phase-1 index space (0 = current, higher = older), and reconstructs each version
either as raw text bytes (single-text-root case) or as pretty JSON (structured
case). A magic-byte sniff in the open path selects `AutomergeSource` vs
`WalrusSource`. `automerge` 0.12.0 enters the build ONLY behind a new walker
cargo feature named `automerge`; with the feature off the adapter, its magic
sniff arm, and the dependency compile out entirely and Walker is byte-for-byte
the Phase-1 GUI.

The one genuinely cross-cutting cost is a boxed-trait-object read path in the
app shell and the player, because today the `Player` owns a concrete
`walrus_fmt::WalrusFile` and the central viewer reconstructs through the trait
using a `VersionId::Index`. An Automerge document has no `WalrusFile`, so Phase 2
must make reconstruction and playback flow through the trait for BOTH backends.
This is anticipated by the Phase-1 code -- `app.rs` already documents "When a
second, read-only backend arrives, reintroduce a boxed trait object for reads"
-- and is specified concretely in section 7.

## Technology stack (locked once approved)

- Language/edition: Rust, workspace edition/MSRV (workspace rust-version 1.95;
  automerge 0.12.0 MSRV is 1.90.0, satisfied).
- New dependency: `automerge = { version = "0.12.0", optional = true }` in
  `crates/walker/Cargo.toml`, pinned EXACTLY to 0.12.0 (MIT, latest stable,
  checksum `2fb8491309...` recorded in `.gate/automerge-0.12.json`). Default
  features; none of automerge's optional features (`wasm`, `utf16-indexing`,
  `utf8-indexing`, `slow_path_assertions`) are enabled.
- New cargo feature: `[features] automerge = ["dep:automerge"]` (walker only).
  No `default` entry -- the feature is OFF by default so the lean Phase-1 build
  is the default build.
- JSON serialisation: the existing `serde_json` dependency (already in
  `walker/Cargo.toml`) for the structured-case tree-to-JSON rendering. No new
  JSON dependency. NOTE (confirmed section 6): automerge's own `AutoSerde` reads
  CURRENT state only and cannot serialise a past version, so the adapter does
  its own `*_at` walk into a `serde_json::Value`.
- Rendering: the existing `view` renderers; structured docs reuse the existing
  `application/json -> Renderer::Code(CodeLang::Json)` path (confirmed in
  `view/selector.rs`). No new renderer variant is introduced.
- Tests: the existing `egui_kittest` dev-dependency for headless harness checks;
  `tempfile` is NOT needed because Automerge documents load from in-memory bytes
  (no on-disk fixture round-trip required for the adapter unit tests).

## 1. VersionId::Heads -- the additive variant and the full ripple

### 1.1 The variant and the head newtype

`VersionId` gains a second variant. The hash type crossing the trait boundary
is a PLAIN walker-side newtype, NOT an `automerge` type, so the trait and every
consumer stay free of the `automerge` dependency (INV-2) and compile with the
feature off:

```rust
// === VersionId (source/mod.rs) ======================================
/// A plain walker-side Automerge change-hash: the 32-byte hash rendered as a
/// lowercase hex string. Carries NO `automerge` type, so `source/mod.rs` and
/// every GUI consumer stay dependency-free (INV-2). The automerge adapter is
/// the only code that converts between this and `automerge::ChangeHash`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HeadId(String);

impl HeadId {
    pub fn new(hex: impl Into<String>) -> Self { Self(hex.into()) }
    pub fn as_str(&self) -> &str { &self.0 }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VersionId {
    /// A linear version index where 0 is CURRENT and higher is OLDER.
    Index(usize),
    /// A set of Automerge heads identifying a point in the change DAG.
    /// Produced ONLY by `AutomergeSource`; walrus/in-memory sources never
    /// produce it. The vec is the frontier (usually one head, more at a merge).
    Heads(Vec<HeadId>),
}
```

Decision: a newtype over `String` (lowercase hex) rather than over `[u8; 32]`.
Reasoning: the hex string is directly displayable in the status bar and history
report with no formatting layer, equality/ordering are trivial and
deterministic, and it keeps `source/mod.rs` free of any byte-layout assumption
about automerge's hash. The adapter converts `automerge::ChangeHash -> HeadId`
via the change's hex rendering (`ChangeHash: Display` -> lowercase hex,
confirmed section 6) and back via `str::parse::<ChangeHash>()`, in the one gated
module.

Why `HeadId` is additive and churn-free: `VersionId::Index` and all its Phase-1
semantics are untouched; every existing `WalrusSource`, `InMemorySource`, and
`FakeSource` path keeps matching `Index` exactly as before. The requirements
name the variant `Heads(Vec<ChangeHash>)`; this design substitutes the plain
`HeadId` for `ChangeHash` precisely to honour INV-2 (no `automerge` type across
the boundary) -- a deliberate, called-out refinement of FR-K11.3, not a silent
divergence.

### 1.2 Full enumeration of the match / construction sites (read from the code)

Every site that constructs or destructures `VersionId` today, re-derived by
reading the shipped files, and how each handles the new variant. The irrefutable
`let VersionId::Index(index) = v;` bindings are the ones that STOP compiling when
a second variant exists; each is listed with its fix.

Production (`src/`):

1. `source/mod.rs` -- the enum definition. Add `Heads` (section 1.1).
2. `source/mod.rs` `InMemorySource::versions` (mod.rs ~285) -- builds `Index`
   only. No change (an in-memory source never produces `Heads`).
3. `source/mod.rs` `InMemorySource::reconstruct` (mod.rs:294) --
   `let VersionId::Index(index) = v;` is no longer irrefutable. Change to a
   `match` whose `Heads` arm returns
   `Err(anyhow!("InMemorySource has no Automerge heads"))`. Documented as
   unreachable in normal use (this source never emits `Heads`).
4. `source/mod.rs` `#[cfg(test)] FakeSource::reconstruct` (mod.rs:453) -- same
   irrefutable `let`; same `match` fix, `Heads` arm errors.
5. `source/mod.rs` `#[cfg(test)] FakeSource::metadata` (mod.rs:461) -- same
   irrefutable `let`; `match` with a `Heads` arm returning the empty
   `VersionMeta`.
6. `source/walrus.rs` `WalrusSource::reconstruct` (walrus.rs:128) --
   `let VersionId::Index(index) = v;`. Change to a `match`; `Heads` arm returns
   `Err(anyhow!("WalrusSource addresses versions by index, not heads"))`.
   WalrusSource never produces `Heads`, so this arm is defensive only.
7. `source/walrus.rs` `WalrusSource::metadata` (walrus.rs:137) --
   `let VersionId::Index(index) = v;`. `match`; `Heads` arm returns an index-less
   default `VersionMeta` (the same empty meta the stale-ref path returns).

These five irrefutable-`let` sites (items 3-7) are exactly the ones the review
confirmed. The two construction sites below are where the review found the
revision-1 enumeration wrong; both are corrected here against the shipped lines:

8. `app.rs:378` central viewer -- the SHIPPED line is EXACTLY:

   ```rust
   let version = VersionId::Index(self.player.as_ref().map_or(0, Player::index));
   ```

   This CONSTRUCTS the version handed to `view::render(ui, source, active,
   &version)`. After the player-indirection change (section 7) this single line
   becomes:

   ```rust
   let version = active
       .as_ref()
       .zip(self.source())
       .map(|(f, s)| s.version_at(f, self.player.as_ref().map_or(0, Player::index)))
       .unwrap_or(VersionId::Index(0));
   ```

   i.e. the version is produced by `source.version_at(active, player.index())`
   (section 7.2), NOT by indexing `versions()`. For a walrus/in-memory source
   `version_at` returns `Index(index)` (the default), so this line is
   behaviour-identical to today; for an Automerge source it returns the `Heads`
   for that linear point. This is the ONLY per-frame version construction in the
   viewer path.

9. `app_mutate.rs:161` `handle_export_action` -- the SHIPPED site is an EXPORT
   path (NOT a post-mutate refresh), and it reconstructs through
   `self.source()`, i.e. the format-agnostic `&dyn HistorySource` returned by
   `app.rs:188 fn source()`, NOT the concrete `WalrusSource`. The shipped line
   is:

   ```rust
   let bytes = match source.reconstruct(file, &VersionId::Index(0)) { ... };
   ```

   After Phase 2 the open source behind `self.source()` MAY be an
   `AutomergeSource` (the navigator single/batch export operates on whatever
   source is open), so a bare `VersionId::Index(0)` would hit
   `AutomergeSource::reconstruct` with an `Index`, not a `Heads`. FIX: route the
   export through `version_at` for consistency with the viewer, so an Automerge
   export reconstructs at the current heads:

   ```rust
   let version = source.version_at(file, 0);   // Index(0) for walrus; current Heads for automerge
   let bytes = match source.reconstruct(file, &version) { ... };
   ```

   This makes `version_at(active, 0)` the single "current version" expression
   everywhere, so `AutomergeSource::reconstruct` is only ever called with a
   `Heads` on the normal paths. As defence in depth, `AutomergeSource::reconstruct`
   still ACCEPTS a bare `Index` by mapping it through its linearised head table
   if in range (section 8), and AC3's export test asserts the exported bytes of
   an Automerge document at the current version equal the current text/JSON.

Consumers that take `&VersionId` but do NOT destructure it -- NO change, they
forward it through the trait:

10. `view/present.rs` `render(.., version: &VersionId)` -- forwards `version` to
    `source.reconstruct`. No match; confirmed by the review. No change.
11. `source/mod.rs` trait method signatures (`reconstruct`, `metadata`) -- take
    `&VersionId`; unchanged.
12. `source/walrus.rs:168` `WalrusSource::file_info` CONSTRUCTS
    `VersionId::Index(0)` internally to size the current version. This is an
    internal walrus-only construction of the EXISTING variant, which never
    breaks when a second variant is added, and it stays `Index(0)` for a walrus
    source -- so it is unaffected by `Heads` and is left as-is. Listed for
    completeness so the enumeration of "every site that constructs or
    destructures `VersionId` today" is exhaustive.

Tests (`tests/` + in-file `#[cfg(test)]`) that construct `VersionId::Index(..)`
literally -- `present.rs`, `navigator.rs`, `walrus_source.rs`, `file_controls.rs`,
`import.rs`, and the `source/mod.rs` unit tests. These keep compiling unchanged
because adding an enum variant does not break construction of the existing one;
they continue to exercise the `Index` path and satisfy AC5 ("all existing
`Index(usize)` behaviour and tests pass unchanged"). New `Heads` behaviour gets
NEW tests in the automerge module (section 9).

Navigator (`src/navigator/**`) and `history_view.rs` -- the review confirmed by
grep that NEITHER references `VersionId`, `manifest()`, `WalrusFile`, or
`player.`. `history_view` consumes `HistoryEntry` (a `usize index`, not a
`VersionId`). So both are unaffected by the variant. Section 4 covers how
Automerge history maps onto `HistoryEntry`.

Net ripple: 1 enum edit + 1 new newtype, 5 irrefutable-`let`-to-`match`
conversions (items 3-7), two construction-site edits that both become
`version_at(active, index)` (items 8-9), and the player-indirection change
(section 7). Nothing in the renderers, selector, transport arithmetic,
navigator, or the history formatter changes for the variant itself.

## 2. One Automerge document -> the multi-file trait

`AutomergeSource` exposes EXACTLY ONE `FileRef`: `FileRef::new(0, name)`, where
`name` is derived from the opened path's file name (the adapter is constructed
with the path, same as `WalrusSource::open`). The trait is multi-file, so:

- `files()` returns `vec![FileRef::new(0, name)]`.
- Every other method validates `f.id() == 0`; a foreign ref yields the same
  benign defaults the trait documents (empty `versions`, zeroed `FileInfo`,
  empty `history`, and `reconstruct` returns an error). This mirrors
  WalrusSource's stale-ref handling exactly.

Decision (pre-approved default): one `FileRef` for the whole document rather
than one `FileRef` per top-level map key. Reasoning: the requirement is a single
addressable "document with history"; per-key fan-out would multiply the version
model (each key has its own sub-history) and is explicitly out of scope. The
single-FileRef model keeps the navigator listing honest (one entry) and the
player timeline singular.

## 3. Structured-vs-text detection, reconstruct, and content_hint

### 3.1 Detection (computed once at open)

After `Automerge::load`, the adapter inspects the document ROOT
(`automerge::ROOT`, the root map object) ONCE at the CURRENT heads and caches a
`Shape`:

```rust
// === Shape (source/automerge.rs) ====================================
enum Shape {
    /// The document is a single text object addressable as the "content":
    /// the root map has exactly one key whose value is a Text object.
    /// Reconstruct yields that text's bytes.
    Text { key: String, name: String, mime: String },
    /// Anything else (multiple keys, nested maps/lists, scalars): reconstruct
    /// serialises the whole tree to pretty JSON.
    Json,
}
```

Detection rule (deterministic, no clock/map-order dependence):

- The root of an automerge document is always a Map (`ROOT`); there is no
  "root is itself Text" case in the data model (confirmed: `ROOT` is a Map and
  values hang off keys). So the Text case is precisely: the root Map has EXACTLY
  one key (`doc.keys(ROOT)` yields one key) AND `doc.get(ROOT, key)` is
  `Value::Object(ObjType::Text)` (checked via `doc.object_type(obj) ==
  ObjType::Text`). The single-key `name`/`mime` are derived from the key name
  via the existing `view::select_renderer` inputs (e.g. a key `"README.md"`
  yields a markdown mime by extension); when the key gives no useful extension,
  mime defaults to `text/plain` and the name defaults to the document (path)
  name.
- Json otherwise (zero keys, more than one key, or the single key is not Text).

Detection is done at the CURRENT heads only and the resulting `Shape` is fixed
for the source's lifetime, so the renderer selection is stable across playback
(a version where the key did not yet exist still renders through the text/JSON
renderer chosen at open -- see the empty-content rule in section 8).

### 3.2 reconstruct(f, v)

Reconstruction reads IN PLACE on the loaded `Automerge` via the `ReadDoc` `*_at`
methods at the version's heads -- there is no snapshot/fork object in 0.12.0
(confirmed section 6):

```
reconstruct(f, v):
  if f.id() != 0 -> Err("no such file: {name}")
  heads = heads_for_version(v)              // section 7.2; Vec<automerge::ChangeHash>
  match self.shape:
    Text{key} ->
      obj = doc.get_at(ROOT, key, &heads)?  // -> Option<(Value, ExId)>
      match obj:
        Some((Value::Object(ObjType::Text), id)) -> doc.text_at(id, &heads)? .into_bytes()
        _                                         -> Vec::new()   // key absent at these heads
    Json ->
      value = walk_map(doc, ROOT, &heads)   // -> serde_json::Value (section 3.2.1)
      serde_json::to_vec_pretty(&value)?    // bytes
```

#### 3.2.1 The JSON walk (pure, deterministic)

A recursive map of automerge values at `heads` to `serde_json::Value`, using the
CONFIRMED `*_at` reads (`keys_at`, `get_at`, `length_at`, `list_range_at`/index
loop, `text_at`) and the CONFIRMED `Value`/`ScalarValue` enums (section 6):

- `ObjType::Map | Table` -> `serde_json::Value::Object`, keys emitted in SORTED
  order (collect `keys_at`, sort, then `get_at` each) so output is byte-identical
  for identical documents regardless of automerge's internal iteration order
  (asserted by a test).
- `ObjType::List` -> `Array`, elements `0..length_at(obj, heads)` via `get_at`.
- `ObjType::Text` -> `String` via `text_at`.
- `ScalarValue`: `Str -> String`, `Int/Uint/F64 -> Number`,
  `Counter(i64)/Timestamp(i64) -> Number`, `Boolean -> Bool`, `Null -> Null`,
  `Bytes -> Array of u8 numbers` (documented choice: lossless and renderer-safe),
  `Unknown { type_code, .. } -> Null` with the key annotated once in a module
  comment (an unknown scalar is rare and not round-trippable to JSON).

This walk is pure (no clock, no map-order dependence) and unit-tested on
in-memory documents.

### 3.3 content_hint(f) and the structured label

- Text case: `content_hint` returns the detected `(mime, name)` so the EXISTING
  selector picks markdown/csv/html/code/text exactly as for a walrus file (AC7
  text half).
- Json case: `content_hint` returns `("application/json", "<name>.json")` so the
  selector resolves to `Renderer::Code(CodeLang::Json)` -- the existing JSON
  code view (AC7 structured half).

For the AC7 "clearly labelled as such in the status bar" requirement, the
adapter reports its structured state through a NEW defaulted trait method
`status_note` (section 8.1), NOT a concrete-only `is_structured()` the shell
cannot see. `AutomergeSource` overrides `status_note` to return
`Some("Automerge (structured JSON view)")` in the Json case and `None` in the
Text case; all other sources inherit the `None` default. The status-bar wiring
is section 8.1.

## 4. History: linearised list first; DAG deferred and LABELLED

`versions(f)` and `history(f)` present a LINEARISED view of the change DAG as
the first cut (pre-approved). The adapter calls `doc.get_changes(&[])` ONCE at
open (confirmed: returns `Vec<Change>` oldest-first in causal/topological
order), and builds the linear sequence of version points plus the per-index head
table (section 7.2):

- Index semantics match Phase 1: index 0 = CURRENT (the document's current heads
  from `doc.get_heads()`), increasing index = OLDER. For a document with `N`
  changes, version `k` is the state after the first `(N - k)` changes, so index
  `N` is the empty/original state and index 0 is current. This mirrors the
  WalrusSource `[0, N]` inclusive range precisely, so the player and transport
  arithmetic (which assume that range) work unchanged (section 7).
- `history(f)` emits one `HistoryEntry` per change in index order. The entry's
  `index` is the version index the change produced; `timestamp` is
  `change.timestamp()` passed THROUGH (unix seconds, NO division -- section 6
  finding 8); `note` is `change.message().map(str::to_owned)`. The
  `SpliceSummary` is a BEST-EFFORT mapping: Automerge changes are not byte
  splices, so the summary records `at = 0`, `deleted_len = 0`,
  `inserted_len = change.len()` (the op count) as a documented approximation.
  This keeps the existing `history_view` Markdown formatter working unchanged
  (it formats only the four `HistoryEntry` fields).

LABELLED LIMITATION (stated, not silently dropped): the change DAG's branching
and merge structure is NOT represented in this first cut. When the history is
non-linear (concurrent branches that merged), the linearisation presents
automerge's own topological order and the history view shows a flat list. This
matches requirements "Out of scope: a full Automerge DAG-branch visualisation"
and design-note open question 3. The adapter records this in a module-doc
comment and the history view is unaffected. The `VersionId::Heads` variant is
deliberately head-SET-valued (`Vec<HeadId>`) so a later DAG-aware iteration can
address merge frontiers without another `VersionId` change -- the data model is
DAG-ready even though the first-cut UI is linear.

Flagged decision for the owner: IF a reviewer judges that a flat linearisation
of a genuinely branched document is misleading enough to need at least a
"merged branches present" badge in this delivery (rather than a pure deferral),
that is a scope increase beyond the pre-approved labelled-deferral and should be
decided explicitly. The design's default, per the pre-approved direction, is the
labelled deferral with no badge.

## 5. Magic-byte sniff in the open path

Open today (`app.rs::open_file`) hard-codes `WalrusFile::open` then
`WalrusSource::open`. Phase 2 introduces a sniff step. The file's leading bytes
decide the backend; the extension is a hint only (consistent with FORMAT.md
section 2 and FR-K11.1):

```
// === open sniff (app.rs) =============================================
read first 4 bytes of the chosen path:
  starts_with [0x85, 0x6F, 0x4A, 0x83]            -> Automerge
  starts_with [0x28, 0xB5, 0x2F, 0xFD] (zstd)     -> Walrus
  starts_with [0x1F, 0x8B]            (gzip)       -> Walrus
  else                                            -> Walrus (let WalrusSource
                                                     surface a precise error)
```

The sniff is a small pure helper `fn sniff_backend(head: &[u8]) -> Backend`
(unit-tested with byte literals, no file I/O). When the `automerge` feature is
OFF, the `Automerge` arm is `#[cfg(feature = "automerge")]`-gated; a file that
sniffs as Automerge in a feature-off build yields a clear status message
("Automerge support is not built into this Walker") rather than mis-opening it as
a walrus file. The native open dialog's filter adds `"automerge"`/`"amrg"` as
hint extensions only when the feature is on; sniffing (not the filter) still
decides.

## 6. Dependency boundary (INV-2) and the CONFIRMED 0.12.0 APIs

`automerge` is declared `optional = true` and reachable only via the walker
feature `automerge` (`dep:automerge`). It is NEVER added to `walrus-fmt` or
`walx`. AC6 is verified by `cargo tree -p walker` showing no `automerge` with
the feature off, and the whole adapter module is `#[cfg(feature = "automerge")]`:

```rust
// === source/mod.rs ===================================================
#[cfg(feature = "automerge")]
#[path = "automerge.rs"]
mod automerge_src;              // module ALIAS: Rust name `automerge_src`,
                                // file `source/automerge.rs`.
#[cfg(feature = "automerge")]
pub use automerge_src::AutomergeSource;
```

NOTE on the `#[path]` alias (review finding 7): the Rust module is named
`automerge_src` so the module path does not collide with the `automerge` CRATE
name, while the file stays `source/automerge.rs`. This NAME/FILE mismatch is a
NEW alias in this crate -- `source/walrus.rs`'s `#[path]` children
(`walrus_import.rs`, `walrus_history.rs`, `walrus_mutate.rs`) use `#[path]` but
their module names MATCH their file names, so they are the precedent for using
`#[path]`, not for aliasing. The alias is valid Rust and compiles; it is called
out here as new rather than claimed to match an existing alias.

### CONFIRMED automerge 0.12.0 API (verified against the extracted crate source)

Full details and source line numbers are in `.gate/automerge-api-0.12.md`. The
names below are quoted from `automerge-0.12.0/src/`, not inferred; the earlier
"candidate / if / OR" hedges are removed.

- Load: `automerge::Automerge::load(data: &[u8]) -> Result<Automerge,
  AutomergeError>` (`automerge.rs:834`).
- Current heads: `doc.get_heads() -> Vec<ChangeHash>` (`automerge.rs:1506`),
  sorted frontier. This is `VersionId::Heads` at index 0.
- Change list: `doc.get_changes(have_deps: &[ChangeHash]) -> Vec<Change>`
  (`automerge.rs:1512`). Returns `Vec<Change>` DIRECTLY (NOT a `Result`).
  `get_changes(&[])` is the full history, oldest-first.
- Read-only state AT a version: the `ReadDoc` `*_at` methods, each taking
  `heads: &[ChangeHash]`, called IN PLACE on the loaded `Automerge` (confirmed:
  there is NO view/fork/isolate/snapshot object in 0.12.0): `object_type`,
  `get_at`, `keys_at`, `length_at`, `list_range_at`, `map_range_at`, `text_at`
  (`read.rs`). This is the heart of `reconstruct` (section 3.2) and it is cheap
  (no document clone).
- `Change` accessors (`change.rs`): `hash() -> ChangeHash`,
  `message() -> Option<&str>`, `timestamp() -> i64`, `actor_id() -> &ActorId`,
  `deps() -> &[ChangeHash]`, `len() -> usize`.
- Timestamp UNIT (resolves finding 8): `Change::timestamp()` is unix SECONDS
  (`CommitOptions.time` doc: "The unix timestamp (in seconds) of the commit").
  Walker's `VersionMeta`/`HistoryEntry` timestamps are also epoch seconds, so
  the adapter passes the value THROUGH with NO `/1000`.
- Author: `change.actor_id().to_hex_string() -> String` (`types.rs`); used for
  `VersionMeta.author`.
- Hash hex round-trip (`types.rs:690`): `ChangeHash(pub [u8; 32])`;
  `impl Display` -> lowercase hex; `impl FromStr` (`type Err =
  ParseChangeHashError`). So `HeadId::new(hash.to_string())` and
  `head.as_str().parse::<ChangeHash>()`. The adapter imports `std::str::FromStr`.
- JSON at a version: `automerge::AutoSerde` serialises CURRENT state only (no
  `_at`) and `hydrate::Value` is NOT `Serialize`, so the adapter's own `*_at`
  walk (section 3.2.1) is REQUIRED, not optional. Values map via the CONFIRMED
  `Value` / `ScalarValue` enums (`value.rs:10, :519`).

Adapter imports (all behind `#[cfg(feature = "automerge")]`):
`use automerge::{Automerge, ReadDoc, ChangeHash, ObjType, Value, ScalarValue,
ObjId, ROOT};` plus `use std::str::FromStr;`.

## 7. The player indirection (the one cross-cutting change)

### 7.1 The problem, read from the code

`player.rs::Player` owns a concrete `walrus_fmt::WalrusFile` and reconstructs via
`self.file.reconstruct(Version::Index(..))` in `current_bytes()`. `app.rs` builds
the player from `WalrusFile::open`, drives the status bar from
`player.manifest()`, and passes a `VersionId::Index` to `view::render`. An
Automerge document has no `WalrusFile`, so the player cannot own one for the
Automerge case. The Phase-1 code anticipates this (the `source` field doc
comment).

### 7.2 The chosen design: a position-only player over the trait

Decision: make the player hold a POSITION (index + len) and the direction/speed
state machine ONLY, and reconstruct through the active `&dyn HistorySource`
rather than owning a `WalrusFile`. The pure stepping/loop/wrap/direction
arithmetic in `player.rs` (and ALL its tests, which already use a detached
`Mini` state machine and small in-memory `WalrusFile`s) is UNCHANGED. What
changes is where `len` and the bytes come from:

- `Player::new` takes `len: usize` (the far index) instead of a `WalrusFile`.
  `len` for a source `s` and file `f` is `s.versions(f).len() - 1` (the inclusive
  far index), matching the existing `[0, len]` seek range. The three
  `player_with_two_records()` / `WalrusFile::new_wal` test constructors change to
  `Player::new(2)` etc. (pure index inputs); the assertions on index arithmetic
  are untouched.
- `current_bytes()` and `manifest()`/`variant()` move OUT of `Player`. The
  byte-fetch moves to the app shell (section 7.4); the variant/name/mime come
  from the active source (section 7.3). `Player` keeps `index()`, `len()`,
  `is_empty()`, `seek`, `step_older/newer`, `tick`, `play/pause`,
  direction/loop/speed exactly as today.

Mapping the player index to a `VersionId` (the one place that differs per
backend) is owned by the source via a NEW defaulted trait method -- this is the
SINGLE mechanism, replacing any notion of indexing `versions()` per frame:

```rust
// === HistorySource (source/mod.rs) ===================================
/// The version addressing value for the linear position `index` (0 = current,
/// higher = older) of file `f`. Walrus/in-memory sources return
/// `VersionId::Index(index)`; `AutomergeSource` returns the `VersionId::Heads`
/// for that linear point. Default returns `Index(index)` so existing sources
/// need no change. `versions(f)` is used only to compute `len` at open; the
/// per-frame version ALWAYS comes from this method.
fn version_at(&self, _f: &FileRef, index: usize) -> VersionId {
    VersionId::Index(index)
}
```

`WalrusSource`, `InMemorySource`, and `FakeSource` inherit the default
(`Index(index)`) -- NO change to them, preserving AC5/AC8 (the review confirmed
these sources build `Index` only in `versions`, so the default is behaviour-
preserving). `AutomergeSource` overrides it: it holds the linearised
`Vec<Vec<HeadId>>` built at open (index 0 = current heads from `get_heads()`,
index k = the heads after the first `N-k` changes, computed by walking the
oldest-first `get_changes(&[])` list), and returns
`VersionId::Heads(self.linear_heads[index].clone())`, clamping an out-of-range
index to the far end. `reconstruct` converts those `HeadId`s back to
`Vec<ChangeHash>` and reads at those heads (section 3.2).

Decision rationale vs the alternative (keep `Player<WalrusFile>` and add a
parallel `AutomergePlayer`): a parallel player would duplicate the entire
step/loop/wrap state machine and the transport wiring, contradicting the
Phase-1 "one trait, no GUI rework" premise and the DRY/testability guidance.
Routing the player through the trait is the smaller, better-tested change: the
pure arithmetic stays put and only the byte-fetch indirection moves up to the
shell that already owns the source.

### 7.3 App-shell impact: the source split, status bar, variant, export

- `WalkerApp` keeps a boxed read source `Option<Box<dyn HistorySource>>` for the
  viewer/player path, PLUS the existing concrete `Option<WalrusSource>` for the
  FR-K5/FR-K6 WRITE path (import/mutate), which Automerge does not have. On open:
  a walrus file populates both (the concrete one, and a boxed `WalrusSource`
  reopened from the same path for reads); an automerge file populates only the
  boxed read source and leaves the concrete `WalrusSource` `None` (the write
  controls are inert for Automerge, which is correct -- Phase 2 is read-only).
  This matches the exact guidance in the Phase-1 `source` field doc comment. The
  `source()` accessor returns `&dyn HistorySource` from the boxed read source.
- `open_file` builds the player from `len = source.versions(active).len() - 1`
  of the active file, not from `WalrusFile::len()`.
- Status bar `file_name` / `mime` (finding 5): today `app.rs` builds `StatusInfo`
  from `player.manifest().files.first()` (a `WalrusFile`-only path). Since the
  player no longer owns a `WalrusFile`, these now come from the ACTIVE SOURCE via
  the trait: `let (mime, name) = source.content_hint(active);` and `let variant =
  self.variant_str();`. `index`/`total` still come from the player
  (`player.index()` / `player.len()`), which are format-agnostic. The structured
  `note` comes from `source.status_note(active)` (section 8.1).
- `variant_str` returns `"wal"`/`"wbl"` for a walrus source and `"amrg"` for an
  Automerge source (there is no `.wal`/`.wbl` variant for Automerge), derived
  from a small `enum SourceKind { Walrus(Variant), Automerge }` the app tracks
  alongside the boxed read source, since `Variant` was previously read from
  `player.variant()`.
- Export context (finding 5): `export_context()` today calls
  `document_name_and_mime(player.manifest())`. It now reads `(mime, name) =
  source.content_hint(active)` and `offered = walrus_fmt::export::offered_formats(&mime)`.
  The export MENU still operates on `self.current_bytes` (the reconstructed
  bytes) + the mime, so it works unchanged for the text case; for the structured
  case it exports the JSON bytes with the JSON mime, which is correct and
  consistent with what is shown. No export backend becomes non-optional (INV-2
  preserved).

### 7.4 App-shell impact: the three byte-fetch sites collapse into one (finding 6)

Today bytes are fetched through `player.current_bytes()` (which calls
`WalrusFile::reconstruct`) in exactly THREE app-shell places (confirmed by grep
across the shipped `crates/walker/src/app.rs`), all identical in intent:

1. `app.rs:156` `fn refresh_bytes(&mut self, player: &Player)` -- the open-time
   refresh.
2. `app.rs:245` `eframe::App::logic` -- the `match player.current_bytes()`
   inside the `if version_changed { ... }` tick-refresh block.
3. `app.rs:366` `eframe::App::ui` CentralPanel -- the `match
   player.current_bytes()` inside the single `if changed { ... }` transport-
   changed block (line 365). `transport::controls(..)` returns the `changed`
   bool that drives this one block; it never fetches bytes itself, so there is
   no separate fourth site.

All three are replaced by a SINGLE method that reads the active source at the
player's current index through `version_at` + `reconstruct`:

```rust
// === app.rs ==========================================================
/// Refresh the cached current-version bytes from the ACTIVE SOURCE at the
/// player's current index. One path for both backends; replaces the three
/// former `player.current_bytes()` call sites (app.rs:156 / 245 / 366). Drops
/// the `&Player` parameter the current `refresh_bytes` takes; the position now
/// comes from `self.player`.
fn refresh_bytes(&mut self) {
    let fetched = match (self.source(), self.navigator.active().cloned(), self.player.as_ref()) {
        (Some(source), Some(active), Some(player)) => {
            let version = source.version_at(&active, player.index());
            source.reconstruct(&active, &version)
        }
        _ => Ok(Vec::new()),
    };
    self.current_bytes = match fetched {
        Ok(bytes) => bytes,
        Err(e) => format!("<reconstruction error: {e}>").into_bytes(),
    };
}
```

Call sites: `open_file` calls `self.refresh_bytes()` after seeking to 0; the
`logic` tick calls it when `player.tick(..)` returns `true`; the `ui` transport
block calls it when `transport::controls(..)` returns `true`. Because there is
now ONE path, an Automerge document and a walrus document refresh through the
exact same code, and there is no residual call to a now-absent
`player.current_bytes()`/`WalrusFile`. This is part of the player-indirection
cost and is why `player.rs` loses `current_bytes`/`manifest`/`variant`.

This player change is the main architectural decision in this design; it is
scoped, anticipated by Phase-1, and keeps every pure player test green.

## 8. Error handling and validation (concrete, per operation)

Application-level (walker binary), so `anyhow` with `.context(..)` per the Rust
standards (library-only `thiserror` does not apply to walker).

- `AutomergeSource::open(path)`:
  - File read failure -> `Err` with context `"reading Automerge document {path}"`.
    Recoverable: the app shell shows it in the status bar (same path as a failed
    walrus open) and leaves no source loaded. Logged at the status-bar level only
    (walker has no file logger).
  - `Automerge::load` failure (corrupt/not-automerge bytes that nonetheless
    passed the 4-byte sniff) -> `Err` with context
    `"loading Automerge document {path}"`. Recoverable, same status-bar surface.
    Fatal to THIS open only, not to the app.
- `reconstruct(f, v)`:
  - Foreign `f` (id != 0) -> `Err("no such file: {name}")`. Recoverable; the
    viewer shows the inline "Reconstruction error" label (existing
    `view/present.rs` path), no panic.
  - `v` is `VersionId::Index(i)` passed to an Automerge source directly (defence
    in depth -- the normal paths use `version_at`, so this is only hit if a
    future caller bypasses it): map `i` through the linearised head table if in
    range (treat it as the linear index), else `Err("version index {i} out of
    range")`. This is the explicit, TESTED fallback the review asked for (AC3
    export test, section 9).
  - `v` is a `Heads` set whose hex does not parse or is not present in this
    document -> `Err("unknown or invalid heads")`. Recoverable; inline error
    label.
  - Shape is `Text` but the text key does not exist at those heads (an old
    version before the key was created) -> return EMPTY bytes (a valid "no
    content yet" state), NOT an error, so playback to the original state shows an
    empty document rather than an error flash.
- `metadata(f, v)` / `file_info(f)` / `history(f)` / `versions(f)` /
  `status_note(f)`: never error (trait contract). A foreign `f` yields the
  documented benign defaults (empty vecs, zeroed `FileInfo`, empty `VersionMeta`,
  `None` note), identical to WalrusSource.
- Feature-off open of an Automerge-sniffed file -> status message "Automerge
  support is not built into this Walker" (no crash, no mis-open). Covered by a
  `#[cfg]`-gated arm in the sniff dispatch.

Input validation of external content:
- The 4 magic bytes are validated by exact prefix match; a file shorter than 4
  bytes falls through to the walrus arm (which errors cleanly).
- Automerge document bytes are untrusted input: the adapter never executes or
  interprets embedded content beyond automerge's own safe decode, and the JSON
  walk produces data only, rendered by the existing sandboxed renderers. Text
  content flows to the same renderers that already treat `.wal` content as
  untrusted (the renderers own the untrusted-content guarantees, per
  `view/present.rs`).

Invariant ownership:
- INV-2 (dependency boundary) is owned by Cargo (feature gating) + the
  `#[cfg(feature)]` module gate; verified by `cargo tree`.
- Index semantics (0 = current, higher = older) are owned by the adapter's
  linearisation (section 4) so the player/transport layer stays oblivious.
- "No `automerge` type crosses the trait" is owned by `HeadId` + the gated
  module; `source/mod.rs` importing no `automerge` symbol is the enforcement.

### 8.1 Status-bar label wiring (finding 4)

AC7 requires a structured Automerge document to be "clearly labelled as such in
the status bar". The shipped `chrome::StatusInfo` has fixed fields
(`file_name`, `variant`, `index`, `total`, `renderer`, `mime`) and
`chrome::status_bar` renders exactly those, with no label field; and
`is_structured()` would be concrete-only, invisible to the format-agnostic
shell. Two concrete changes make the label reachable while keeping the shell
format-agnostic:

1. A new DEFAULTED trait method on `HistorySource`:

   ```rust
   // === source/mod.rs ===============================================
   /// An optional, human-readable note about how `f` is being presented
   /// (e.g. a structured-view caveat), shown in the status bar. Default is
   /// `None`; only sources that transform content away from an original file
   /// override it. Keeps the shell format-agnostic (no downcast needed).
   fn status_note(&self, _f: &FileRef) -> Option<String> { None }
   ```

   `AutomergeSource` overrides it: `Some("Automerge (structured JSON
   view)".into())` in the Json shape, `None` in the Text shape. All other
   sources inherit `None`, so Phase-1 behaviour and tests are unchanged.

2. A new optional field on `StatusInfo` and its rendering:

   ```rust
   // === chrome.rs ===================================================
   pub struct StatusInfo<'a> {
       pub file_name: &'a str,
       pub variant: &'a str,
       pub index: usize,
       pub total: usize,
       pub renderer: &'a str,
       pub mime: &'a str,
       pub note: Option<&'a str>,   // NEW: optional presentation note
   }
   ```

   `status_bar` renders `note` after `mime` when `Some` (a separator + the
   label); when `None` the status bar is visually identical to Phase 1.

`app.rs` sets `note` from `let note = self.source().and_then(|s|
s.status_note(active));` when building `StatusInfo`, so a structured Automerge
document shows the caveat and every walrus document shows nothing new. This is
the only `StatusInfo`/`status_bar` change and it is additive.

## 9. Testability

Pure, unit-testable in `source/automerge.rs` `#[cfg(test)]` (no egui, no disk;
documents are built in memory with the automerge crate's own `AutoCommit`
builder and `save()`d to bytes, then `Automerge::load`ed):

- AC1 sniff: `sniff_backend` returns Automerge for the magic prefix, Walrus for
  zstd/gzip, Walrus for junk and for a <4-byte input -- byte-literal inputs, no
  I/O.
- AC2 versions: a text document built with K changes lists `K+1` versions in
  index order (0 = current).
- AC3 reconstruct: `reconstruct(f, version_at(f, 0))` returns the current text
  bytes; a higher index returns the older text; the empty/original index returns
  empty bytes. PLUS the export path: `reconstruct(f, VersionId::Index(0))`
  (the defence-in-depth bare-`Index` fallback, section 8) returns the SAME
  current bytes as `version_at(f, 0)`, so an Automerge export from the navigator
  is correct (finding 1).
- AC5 additivity: `VersionId::Heads(..)` constructs and round-trips through
  `HeadId` (hex `ChangeHash` -> `HeadId` -> `ChangeHash` is identity); existing
  `Index` tests are untouched (compile-level proof).
- AC7 shape + label: a single-text-root doc reports a text mime via
  `content_hint`, `status_note == None`; a multi-key structured doc reports
  `application/json`, `status_note == Some("Automerge (structured JSON view)")`;
  the JSON walk is byte-stable across two loads of the same document
  (deterministic sorted-key ordering).
- history: `history(f)` yields one entry per change in index order with
  timestamps (unix seconds, NOT divided) and messages mapped; the best-effort
  `SpliceSummary` shape (`at = 0`, `deleted_len = 0`, `inserted_len = op count`)
  is asserted exactly.
- version_at: `AutomergeSource::version_at(f, k)` returns the `Heads` for linear
  point k, and `version_at(f, 0)` equals `Heads(get_heads())`;
  `WalrusSource`/`InMemorySource` inherit `Index(k)` (a tiny test on the
  default).
- status_note default: a `FakeSource`/`InMemorySource` returns `None` from the
  defaulted `status_note` (one assertion), proving the default keeps Phase-1
  sources unlabelled.

Headless `egui_kittest` harness (NFR-3), in `tests/`:

- The transport + viewer drive an `AutomergeSource` (built in-memory) through the
  SAME `transport::controls` + `view::render` path as a walrus file: stepping
  older/newer changes the rendered bytes, proving AC4 (no transport/player code
  is format-aware). A fixed, bounded frame count; assertions on model
  state/widget presence, not pixels or frame counts (per the testing steering).
- A structured-document harness asserts the status bar shows the structured note
  (the `note` field reaches `status_bar`), covering the AC7 wiring at the shell
  level.

Manual-only (stated reason): the `rfd` native open/save dialogs and pixel-exact
rendering of the JSON tree -- neither is headless-drivable (OS dialog / GPU
pixels), identical carve-out to Phase 1.

AC6/AC8 (feature on/off, Phase-1 unchanged) are verified by the scoped gate
run with the feature both ways: `cargo test -p walker` (feature off, default)
and `cargo test -p walker --features automerge`, plus `cargo tree -p walker`
showing no `automerge` in the off build. The full gate is the owner's step.

## 10. Files touched (summary)

New:
- `crates/walker/src/source/automerge.rs` -- the `AutomergeSource` adapter,
  `Shape`, the `*_at` JSON walk, the linearisation + per-index head table,
  `version_at`/`status_note` overrides. Kept under 400 non-test lines (NFR-1); if
  it approaches the cap, the JSON walk and the shape detection split into a
  `#[path]` child (`automerge_shape.rs`) mirroring the walrus child-module
  pattern.

Modified:
- `crates/walker/Cargo.toml` -- optional `automerge = "0.12.0"` dep + `automerge`
  feature.
- `crates/walker/src/source/mod.rs` -- `HeadId` newtype, `VersionId::Heads`
  variant, `version_at` + `status_note` default trait methods, gated
  `AutomergeSource` re-export (aliased module), the two in-memory sources'
  `reconstruct`/`metadata` `let`->`match` fixes (items 3-5).
- `crates/walker/src/source/walrus.rs` -- `reconstruct`/`metadata`
  `let`->`match` fixes (items 6-7; defensive `Heads` arms).
- `crates/walker/src/player.rs` -- `Player::new(len: usize)`; remove
  `current_bytes`/`manifest`/`variant` (moved to the shell / active source);
  pure state-machine arithmetic and its tests unchanged (constructors switch to
  index inputs).
- `crates/walker/src/app.rs` -- the sniff dispatch in `open_file`; the boxed read
  source + concrete write source split with a `SourceKind` tag; player
  construction from `versions()` len; the single `refresh_bytes(&mut self)`
  replacing the three `player.current_bytes()` sites at app.rs:156 / 245 / 366
  (section 7.4);
  `version_at`-driven viewer version at `app.rs:378` (item 8); status bar built
  from `content_hint`/`status_note` of the active source (findings 4, 5);
  `variant_str` "amrg"; `export_context` from `content_hint` (finding 5).
- `crates/walker/src/app_mutate.rs` -- `handle_export_action` (`app_mutate.rs:161`)
  reconstructs via `source.version_at(file, 0)` instead of a bare
  `VersionId::Index(0)` (item 9 / finding 1).
- `crates/walker/src/chrome.rs` -- `StatusInfo.note: Option<&str>` field and its
  rendering in `status_bar` (finding 4).

Unchanged (confirmed): `view/selector.rs`, `view/present.rs`, `view/mod.rs` and
all renderers, `transport.rs`, `history_view.rs`, `navigator/**`.

## Assumptions

- A deterministic in-memory Automerge fixture is built with the automerge
  crate's `AutoCommit` builder in `#[cfg(test)]` (feature-gated), not committed
  as a binary, so the adapter tests have no external-file dependency (testing
  steering: no files outside `tests/fixtures/`).
- The 0.12.0 method spellings in section 6 are CONFIRMED against the extracted
  crate source (`.gate/automerge-api-0.12.md`), not assumed. If a point release
  within 0.12.x were ever to change a name, that is a mechanical swap recorded in
  a module comment; no design decision depends on a spelling.
- `Change::timestamp()` is unix SECONDS (confirmed), so the adapter passes it
  through unchanged into the epoch-seconds `VersionMeta`/`HistoryEntry` fields.

## Responses to the design review findings

Review: `design-review.md` / `design-review.json`, verdict CHANGES_REQUESTED
(3 HIGH, 3 MEDIUM, 2 NIT). All six HIGH/MEDIUM findings are ADDRESSED; both NITs
are addressed.

- Finding 1 (HIGH -- app_mutate.rs/app.rs enumeration wrong): ADDRESSED.
  Section 1.2 item 9 is re-written: `app_mutate.rs:161` is the EXPORT path in
  `handle_export_action` reconstructing through `self.source()` (`&dyn
  HistorySource`, possibly `AutomergeSource`), and is changed to
  `source.version_at(file, 0)`; `AutomergeSource::reconstruct` additionally
  accepts a bare `Index(0)` as the current version as a TESTED fallback (section
  8, AC3 export test in section 9). Item 8 quotes the real `app.rs:378` line.
- Finding 2 (HIGH -- two conflicting version mechanisms): ADDRESSED. The
  "indexed by `versions()`" wording is deleted. `source.version_at(active,
  player.index())` is stated once as the SINGLE mechanism (sections 1.2 item 8
  and 7.2); `versions()` is used only to compute `len` at open; both the viewer
  (item 8) and the export (item 9) route through `version_at`.
- Finding 3 (HIGH -- unverified 0.12.0 API): ADDRESSED. The crate was extracted
  and read; section 6 and `.gate/automerge-api-0.12.md` record the CONFIRMED
  names with source line numbers. The key result: reconstruct reads IN PLACE via
  the `ReadDoc` `*_at(.., heads)` methods -- there is NO view/fork/snapshot
  object, so the earlier "candidate / OR" hedges are gone and reconstruct
  (section 3.2) is implementable as written. `load`, `get_changes(&[])` (returns
  `Vec<Change>`, not `Result`), `get_heads`, the `Change` accessors, and
  `ChangeHash` Display/FromStr hex are all confirmed.
- Finding 4 (MEDIUM -- structured label unwired): ADDRESSED. Section 8.1 adds a
  defaulted `status_note(&self, &FileRef) -> Option<String>` trait method
  (keeping the shell format-agnostic, the review's preferred option b) and a new
  `StatusInfo.note: Option<&str>` field rendered by `status_bar`; `app.rs` wires
  it from the active source.
- Finding 5 (MEDIUM -- player.manifest() gone): ADDRESSED. Section 7.3 specifies
  that `file_name`/`mime` for the status bar and `document_name`/`mime`/`offered`
  for export now come from `source.content_hint(active)` /
  `walrus_fmt::export::offered_formats(&mime)`, replacing
  `player.manifest().files.first()` and `document_name_and_mime(player.manifest())`;
  the variant comes from a `SourceKind` tag the app holds.
- Finding 6 (MEDIUM -- byte-fetch sites): ADDRESSED. Section 7.4 enumerates the
  `player.current_bytes()` sites and collapses them into one
  `fn refresh_bytes(&mut self)` that reconstructs through the active source at
  `version_at(active, player.index())`. (The second review corrected the count
  of these sites from four to the three that actually exist -- app.rs:156 / 245
  / 366 -- see the "Changes since revision 2" note and the review-2 responses
  below.)
- Finding 7 (NIT -- `#[path]` precedent): ADDRESSED. Section 6 now states the
  `automerge_src` -> `automerge.rs` mapping is a NEW module alias and that the
  `walrus_*` children are the precedent for `#[path]`, not for a name/file
  alias.
- Finding 8 (NIT -- timestamp unit): ADDRESSED. Confirmed via
  `CommitOptions.time` ("unix timestamp in seconds") that `Change::timestamp()`
  is SECONDS; the `/1000` is removed everywhere (sections 4, 6, 9, Assumptions),
  and the adapter passes the value through to the epoch-seconds fields.

### Responses to the SECOND review (revision 2 -> revision 3)

Second review: `design-review.md` / `design-review.json`, verdict
CHANGES_REQUESTED (1 HIGH + 2 NIT). All three are ADDRESSED.

- Review-2 finding 1 (HIGH -- section 7.4 enumerated FOUR
  `player.current_bytes()` sites; only THREE exist): ADDRESSED. Verified by grep
  against the shipped `crates/walker/src/app.rs` in this worktree: the only
  `player.current_bytes()` call sites are `app.rs:156` (`refresh_bytes`),
  `app.rs:245` (`logic`, inside `if version_changed`), and `app.rs:366` (`ui`
  CentralPanel, inside the single `if changed` block at line 365). The fictitious
  item (4) -- a claimed second, duplicated match in a separate transport-controls
  branch -- is deleted; `transport::controls(..)` returns the `changed` bool that
  drives the one line-365 block and never fetches bytes. Section 7.4, the
  overview "Changes" note, and the Files-touched summary now say THREE, and the
  single-`refresh_bytes(&mut self)` collapse is unchanged (it replaces three
  sites, dropping the `&Player` parameter).
- Review-2 finding 2 (NIT -- `WalrusSource::file_info` constructs
  `VersionId::Index(0)` and was omitted from section 1.2): ADDRESSED. Section 1.2
  now has item 12 noting `source/walrus.rs:168` `file_info` constructs
  `VersionId::Index(0)` internally, is unaffected by the new variant, and is
  left as-is.
- Review-2 finding 3 (NIT -- section 7.4 lacked the line-level precision of the
  rest of the design): ADDRESSED. Section 7.4 now cites the three sites by line
  number (156 / 245 / 366), matching section 1.2's precision.
