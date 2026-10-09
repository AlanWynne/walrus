# Walker Viewer -- Technical Design

Status: REVISED after design review (see "Review responses" at the end). Scope:
HEAVY feature. All paths below are inside the worktree
`c:\workspace\Walrus\.worktrees\walker-viewer`.

This design turns `walker` from a plain-text player into a content-aware viewer
with per-type renderers, playback speed control, egui built-in theming, an
FFWB-style chrome (minus the command line), and a per-version export menu backed
by new feature-gated export backends in `walrus-fmt`. It is written to be
implemented without further architectural decisions; the technology stack named
here is LOCKED once approved.

---

## 0. GUI stack version (single stated constant)

egui minor = **0.36**. This one constant applies UNIFORMLY to every egui-side
crate so they cannot drift:

| Crate          | Exact version | Notes                                              |
|----------------|---------------|----------------------------------------------------|
| `egui`         | `0.36`        | bumped from 0.28 in the egui-0.36 upgrade task     |
| `eframe`       | `0.36`        | bumped from 0.28                                   |
| `egui_extras`  | `0.36`        | ONLY if a renderer needs it; `default-features = false`, `features = ["syntax_highlighting"]`, NO `table` feature. NOT added by the upgrade task (walker's current source does not use it); the CSV-viewer task adds it when the Code/CSV renderers land. |
| `egui_kittest` | `0.36`        | walker `[dev-dependencies]`; the headless GUI test harness |

`egui_kittest = "0.36"` IS now the headless GUI test harness (reversing the old
"no egui_kittest" trade). It entered the ecosystem with egui 0.29, so moving the
stack to 0.36 unlocks it. The default (egui) harness drives a `Ui` closure, which
is enough for walker's widget-level assertions; the `eframe` feature is only
needed for a full-app harness and is NOT enabled.

`catppuccin-egui` is DROPPED entirely. Theming uses egui's built-in theming
(`egui::Context::set_visuals` / the light-dark presets); a flavored theme picker
is out of scope for the upgrade task and, if wanted later, is built on egui's own
`Visuals` rather than a third-party crate. This removes the former tension where
a catppuccin `egui2x` feature had to track the egui minor.

MSRV: egui 0.36 raised the required Rust to **1.95**. The workspace
`rust-version` in `[workspace.package]` is set to `1.95` (walrus-fmt, walx, and
walker all inherit it via `rust-version.workspace = true`). The edition stays
`2021`. The local toolchain (1.98.1) satisfies this.

### Scope note: what the egui-0.36 upgrade task resolves vs what is deferred

The egui-0.36 upgrade + egui_kittest-enablement task resolves ONLY the GUI-stack
findings from `design-review.json`: HIGH findings 1 and 2 and MEDIUM finding 4
(the egui-version / egui_kittest / egui_extras / catppuccin mismatch), via the
single egui-minor constant above. It also bumps the egui 0.28 -> 0.36 API in
`app.rs` (the eframe 0.36 `App::update` -> `App::logic` + `App::ui` split, and the
`TopBottomPanel`/`CentralPanel` -> `egui::Panel::top`/`bottom` with
`show(ui, ..)` container change) and adds one real headless `egui_kittest`
harness test.

The export-related findings (3, 5, 6, 7, 8) and the CSV / per-file line-budget
nits (9, 10) are NOT resolved by the upgrade task. They belong to the later
CSV-viewer implementation task, which also lands the content-aware renderers,
the export backends, `egui_extras`, and the chrome. Sections 1-7 below describe
that later task and are retained unchanged except where section 0's stack
constant supersedes an old egui-0.28 pin.

---

## 1. Overview

`walker` already owns an `eframe::App` (`WalkerApp`) that opens a `.wal` / `.wbl`
file, drives a pure `Player` state machine, and renders the current version into
a read-only `TextEdit`. The format engine (`walrus-fmt`, package `walrus-fmt`,
crate dir `crates/walrus`) exposes `WalrusFile::reconstruct(Version) -> Result<Vec<u8>>`
and `WalrusFile::manifest()` (carrying `document.mime` and `document.name`).

The change set is two-sided and respects the sacred dependency boundary
(project-conventions invariant 2):

- `walrus-fmt` gains real export backends behind the EXISTING `export-*` cargo
  features. It stays GUI-free: no egui/eframe/catppuccin ever enters its graph.
- `walker` gains the viewer renderers, the speed UI, the theme picker (egui
  built-in `Visuals`), the FFWB-style chrome, and the export UI. All egui-side
  crates live only here.

Two builds matter, and the design is verified against both (expanding the former
single "zero export crates" claim, which was only true for a direct no-feature
build):

1. `cargo build -p walrus-fmt` (no features) -- the EMBEDDABLE BOUNDARY proof.
   MUST compile the always-present export metadata surface (`ExportFormat`,
   `ExportUnavailable`, `offered_formats`, `to_original`; section 7.1) and MUST
   link NONE of `pulldown-cmark` / `scraper` / `csv` / `genpdf` / `docx-rs`.
2. `cargo build` (whole workspace) -- the SHIPPED build. It builds `walker`,
   which depends on `walrus-fmt` with `features = ["export-all"]` (verified in
   `crates/walker/Cargo.toml`), so ALL heavy export crates ARE in the graph and
   ARE subject to the MSRV gate (section 7.3). For the shipped product the heavy
   export crates are NOT optional; the MSRV gate in 7.3 is a gate on THIS default
   workspace build, not an opt-in extra.

The guiding principles are: FORMAT.md is the source of truth (no format change
is needed by this work); every `.rs` file stays under 400 non-test lines; plain
ASCII only in source and docs; TDD (a failing test precedes each behaviour); and
rendered content is UNTRUSTED (we render only implemented constructs, never
execute scripts or fetch remote resources).

---

## 2. Content-aware viewer (walker)

### 2.1 Renderer selection

The renderer is chosen from `manifest().document.mime`, with an extension
fallback (derived from `manifest().document.name`) when the MIME is generic or
unknown. "Generic" means `application/octet-stream`, an empty string, or a
`text/plain` that the extension can refine (so a `report.md` stored with a lazy
`text/plain` mime still renders as Markdown). A dedicated pure function owns this
mapping so it is unit-testable without egui:

```
fn select_renderer(mime: &str, name: &str) -> Renderer
```

`Renderer` is a data-carrying enum (make illegal states unrepresentable,
rust-standards):

```
enum Renderer {
    Html,
    Markdown,
    Csv,
    Code(CodeLang),   // JSON, TOML, or a source language
    Text,             // plain monospace fallback
}
```

Resolution order:

1. Match the normalised MIME (lowercased, parameters after `;` stripped) against
   the table below. If it maps to anything other than `Text`, use it.
2. Otherwise (generic/unknown/`text/plain`), look at the lowercased extension of
   `document.name` and apply the extension column.
3. If neither resolves, `Renderer::Text`.

MIME -> renderer table (primary):

| MIME                                 | Renderer           |
|--------------------------------------|--------------------|
| `text/html`, `application/xhtml+xml` | `Html`             |
| `text/markdown`, `text/x-markdown`   | `Markdown`         |
| `text/csv`                           | `Csv`              |
| `application/json`                   | `Code(Json)`       |
| `application/toml`, `text/toml`      | `Code(Toml)`       |
| `application/xml`, `text/xml`        | `Code(Xml)`        |
| `text/plain`                         | `Text` (refinable) |
| anything else                        | `Text`             |

Extension fallback table (used only in step 2):

| Extension                                   | Renderer           |
|---------------------------------------------|--------------------|
| `.html`, `.htm`, `.xhtml`                   | `Html`             |
| `.md`, `.markdown`                          | `Markdown`         |
| `.csv`                                      | `Csv`              |
| `.json`                                     | `Code(Json)`       |
| `.toml`                                     | `Code(Toml)`       |
| `.java`                                     | `Code(Java)`       |
| `.c`, `.h`                                  | `Code(C)`          |
| `.cpp`, `.cc`, `.cxx`, `.hpp`               | `Code(Cpp)`        |
| `.py`                                       | `Code(Python)`     |
| `.rs`                                       | `Code(Rust)`       |
| `.js`, `.mjs`                               | `Code(JavaScript)` |
| `.ts`                                       | `Code(TypeScript)` |
| `.xml`                                      | `Code(Xml)`        |
| `.sh`, `.bash`                              | `Code(Shell)`      |
| `.txt` or unrecognised                      | `Text`             |

`CodeLang` carries the token string that `egui_extras::syntax_highlighting`
expects. The CSV-viewer task pins `egui_extras = "0.36"` (section 0) and MUST
confirm the exact `highlight` / `CodeTheme` surface against that pinned 0.36
crate before relying on it (per design-review finding 2, do not take a
version-sensitive signature on faith). The 0.36 `syntax_highlighting` surface is
shaped roughly as:

```
// egui_extras::syntax_highlighting, egui_extras 0.36 -- CONFIRM exact arity on pin
pub struct CodeTheme { /* ctors: CodeTheme::dark(..) / CodeTheme::light(..) / from_style(style) */ }
pub fn highlight(ctx, style, theme: &CodeTheme, code: &str, language: &str) -> egui::text::LayoutJob;
```

The `language` hint is a
short token like `"rs"`, `"json"`, `"toml"`, `"java"`, `"c"`, `"cpp"`, `"py"`,
`"js"`, `"ts"`, `"xml"`, `"sh"`. A hint the highlighter does not know degrades
gracefully to monospace (its own behaviour), satisfying "fall back to plain
monospace if a language cannot be mapped" without special-casing.

### 2.2 Renderers

- HTML (`text/html`) -- an ACCEPTABLE SUBSET rendered NATIVELY in egui. DECIDED:
  no webview, no browser engine (see section 2.4). Supported constructs:
  headings `h1..h6`, `p` paragraphs, `strong`/`b` (bold), `em`/`i` (italic),
  inline `code`, `pre`/`code` blocks (monospace), `ul`/`ol` lists (nested),
  `a` links (shown as styled text with the href in a tooltip; NOT navigated),
  `blockquote`, `table`/`tr`/`td`/`th` (via `egui_extras::TableBuilder`), `hr`
  horizontal rules, and `img` rendered as a labelled placeholder box (never
  fetched). All other tags are skipped (their text content is still shown).
  Entities are decoded for display. Content is UNTRUSTED: scripts, styles, event
  handlers, iframes, and remote resources are never executed or fetched.

- Markdown (`text/markdown`) -- parsed with `pulldown-cmark` into the SAME native
  egui layout path as HTML where practical (headings, emphasis, code, lists,
  block quotes, tables, rules, links-as-text, image placeholders). The HTML and
  Markdown renderers share one internal `Block`/`Inline` model and one egui
  painter so the two feed a single layout path (section 2.3).

- CSV (`text/csv`) -- a column grid viewer, used ONLY when the document is
  actually CSV. Parsed with the `csv` crate into rows; rendered as an aligned,
  scrollable grid via `egui_extras::TableBuilder` (TableBuilder is unconditional
  in `egui_extras`; there is NO separate `table` feature). This replaces the
  current always-present "CSV viewer" tab (which the user called out as out of
  place): a non-CSV document never shows the CSV view.

- Code + structured text (`Code(..)`) -- highlighted with
  `egui_extras::syntax_highlighting` (`CodeTheme` + `highlight`). JSON is
  pretty-printed (parse with `serde_json`, re-serialise with
  `to_string_pretty`; if it does not parse, show the raw bytes) BEFORE
  highlighting. TOML is highlighted as-is. Source languages
  (`.java .c .cpp .h .py .rs .js .ts .xml .sh`) use the matching language hint.
  Unknown hint -> monospace.

- Plain text (`text/plain` or unrecognised) -- read-only monospace, the current
  behaviour, kept as the universal fallback.

### 2.3 Shared layout path

HTML and Markdown both lower to a small internal document model owned by
`view/richtext.rs`:

```
enum Inline { Text(String), Bold(..), Italic(..), Code(String), Link { text, href } }
enum Block  { Heading(u8, Vec<Inline>), Paragraph(Vec<Inline>), CodeBlock(String),
              List { ordered: bool, items: Vec<Vec<Block>> }, Quote(Vec<Block>),
              Table { head: Vec<..>, rows: Vec<..> }, Rule, ImagePlaceholder(String) }
```

`view/html.rs` lowers the subset of HTML to `Vec<Block>`; `view/markdown.rs`
lowers `pulldown-cmark` events to the same `Vec<Block>`; `view/richtext.rs`
paints `Vec<Block>` with egui widgets. The lowering functions are PURE (bytes ->
`Vec<Block>`), so the block model is unit-testable without a running egui context
-- this is what substitutes for the (unavailable) `egui_kittest` harness.

### 2.4 Webview decision (locked)

DECIDED: NO embedded webview. The user asked whether embedding a webview is a
good fit; the recommendation and locked decision is NO, because: (a) a webview
(wry/tao or CEF) is a heavy native dependency with a platform-specific runtime
(WebView2 on Windows, WebKitGTK on Linux), at odds with a lean egui viewer;
(b) it would execute arbitrary scripts and fetch remote resources from UNTRUSTED
stored documents, directly violating the untrusted-content rule; (c) it does not
compose with egui's single-surface immediate-mode rendering or theming. The
native acceptable-HTML subset gives safe, themed, testable rendering sufficient
for viewing stored document versions.

### 2.5 Deferred (do NOT implement now)

Future work: COBOL (`.cob`) and other custom sublime-syntax languages; a full
`syntect` upgrade (the `egui_extras` highlighter is enough for now); and syntax
highlighting INSIDE Markdown/HTML code fences (fences render as plain monospace
code blocks for now).

---

## 3. Walker module decomposition

Every file stays under 400 non-test lines; each new file TARGETS well under that
(roughly 150-250 lines each). `richtext.rs` is the explicit split seam: if
`html.rs` or `markdown.rs` approaches 400 lines, move the inline-vs-block lowering
halves into sibling files that still feed the one `richtext.rs` block model.

| File                   | Responsibility                                                        |
|------------------------|-----------------------------------------------------------------------|
| `src/app.rs`           | `WalkerApp` state + eframe `update`; wires panels, delegates drawing. Trimmed (target < ~250 lines) by moving chrome/transport/export out. |
| `src/chrome.rs`        | FFWB-style chrome: top menu bar + toolbar, bottom status bar (section 5). |
| `src/transport.rs`     | Transport controls row + the speed control UI (section 4); calls `Player`. |
| `src/theme.rs`         | Theme-mode enum (built on egui `Visuals`), the picker combo box, and `apply` (section 6).  |
| `src/export_ui.rs`     | Export menu: offered-format list per document type, `rfd` save dialog, calls `walrus_fmt::export` (section 7). |
| `src/view/mod.rs`      | Re-exports; dispatch to the right view. No logic beyond selection re-export. |
| `src/view/selector.rs` | `select_renderer(mime, name)` pure mapping + `CodeLang` (section 2.1). |
| `src/view/richtext.rs` | Shared `Block`/`Inline` model + egui painter (section 2.3). SPLIT SEAM.  |
| `src/view/html.rs`     | HTML-subset lowering to `Vec<Block>` (untrusted-safe).                |
| `src/view/markdown.rs` | Markdown (pulldown-cmark) lowering to `Vec<Block>`.                   |
| `src/view/csv.rs`      | CSV parse + column grid.                                              |
| `src/view/code.rs`     | Code/JSON/TOML highlighting via egui_extras; JSON pretty-print.       |
| `src/view/text.rs`     | Plain monospace fallback.                                            |
| `src/player.rs`        | UNCHANGED state machine; speed via its existing `set_step_interval` (section 4). |

`view/mod.rs` and `view/selector.rs` are deliberately separate so the pure
selection logic has no egui dependency and tests can call it directly.

---

## 4. Playback speed (walker)

The `Player` already holds `step_interval: Duration` and the (currently
`#[allow(dead_code)]`) `set_step_interval`, clamped to a 16 ms minimum. This work
WIRES a UI control to it and REMOVES the `dead_code` allow once called. The
player loop/stop/wrap/direction state machine and its three existing unit tests
stay intact (no behaviour change to `advance_one`/`tick`).

UI (in `transport.rs`): a speed control expressed as a MULTIPLIER against a base
interval of 400 ms (the current default), DECIDED as a preset stepper plus a fine
slider:

- A segmented preset: `0.25x, 0.5x, 1x, 2x, 4x` (1x = 400 ms/step).
- A fine slider `0.25x..=4x` for in-between values.

The conversion is a PURE free function so it is unit-testable without egui:

```
// transport.rs
pub fn interval_for_multiplier(mult: f64) -> Duration {
    Duration::from_secs_f64(0.400 / mult)
}
```

A thin wrapper `set_speed_multiplier(player, mult)` calls
`player.set_step_interval(interval_for_multiplier(mult))`; the player clamps to
>= 16 ms, so 4x = 100 ms is safe and even 25x clamps rather than busy-loops.
Multiplier (not raw ms) is chosen because it reads naturally ("twice as fast")
and keeps one base constant to tune. The current multiplier is held in
`WalkerApp` so widget state persists across frames and files.

Testing (new, pure):

- `transport.rs`: `interval_for_multiplier` returns 400 ms at 1x, 100 ms at 4x,
  1600 ms at 0.25x (exact arithmetic, no GUI).
- `player.rs`: `set_step_interval` lower-bound clamp (sub-16 ms -> 16 ms); and a
  fixed interval advances the expected index count over a known elapsed sum (e.g.
  1000 ms at 200 ms/step advances 5, consistent with `tick`'s
  `while accumulated >= step_interval` loop). The three existing loop/wrap tests
  are UNCHANGED.

The widget-to-player wiring (slider drag -> model interval) CAN now be asserted by
the headless `egui_kittest` harness (section 0); the pure
`interval_for_multiplier` + `set_step_interval` arithmetic is additionally
unit-tested directly. Only pixel-exact cadence stays a manual visual check
(section 8).

---

## 5. FFWB-style chrome minus command line (walker)

The look and feel is inherited from FileForgeWorkbench's desktop shell, which
lays out `TopBottomPanel::top` for the menu bar, a central panel for content, and
`TopBottomPanel::bottom` for the status bar. `walker` replicates this NATIVELY --
there is NO `ff-*` dependency and none is added (invariant 2). FFWB is read for
LAYOUT INSPIRATION ONLY.

Layout owned by `chrome.rs`:

- Top: a menu bar (`egui::menu::bar`) with `File` (Open..., Export..., Quit),
  `View` (wrap/no-wrap and a raw/rendered toggle; the renderer itself is
  automatic), and `Theme` (the flavor picker, section 6). Directly beneath it, a
  thin toolbar row with the open button and the theme combo for quick access,
  mirroring FFWB's chrome density.
- Center: a SINGLE content surface (NOT the old fixed Player/CSV tab pair)
  showing the selected renderer's output for the current version. The seek slider
  and transport live in a bottom-of-center strip (via `transport.rs`).
- Bottom: a status bar (`TopBottomPanel::bottom`) with non-interactive labels:
  file name, variant (`wal`/`wbl`), current index / total versions, detected
  renderer, document MIME, and any error. Mirrors FFWB's status-bar segments
  (plain labels, not focusable), minus FFWB-specific content.

EXPLICITLY ABSENT: the FFWB command line (its `Command ===>` input and
`render_command_line` panel). `walker` has NO command-line input bar. The
existing `View` enum (`Player`/`Csv`) in `app.rs` is removed; the view is derived
per-document by `select_renderer`, so the stale "CSV viewer" tab disappears.

---

## 6. Theming (walker)

Theming uses egui's OWN built-in theming (DECIDED: `catppuccin-egui` is dropped,
section 0; no third-party theme crate). The picker is option B -- a small theme
PICKER, NOT a full style editor. No extra dependency is added to
`crates/walker/Cargo.toml`; egui 0.36 already ships light/dark `Visuals` presets
and `ThemePreference` support.

`theme.rs` defines:

```
enum ThemeMode { System, Light, Dark }   // default Dark (FFWB default-dark posture)
```

mirroring FFWB's default-dark posture. Applied on change by setting the egui
theme preference, e.g. `ctx.set_theme(mode.to_preference())` where
`to_preference` maps `System`/`Light`/`Dark` to
`egui::ThemePreference::{System, Light, Dark}` (egui then resolves the concrete
`Visuals`). The picker is a single `egui::ComboBox` among the three modes, placed
in the `Theme` menu and the toolbar. No per-token style editor.

`ThemeMode` and `to_preference` are pure data, so the mode state transition is
unit-tested directly (section 8); only the resulting pixel colours stay manual.
If a richer curated palette is wanted later, build it from egui `Visuals` rather
than reintroducing a third-party theme crate.

---

## 7. Export API shape (walrus-fmt)

Export operates on the ALREADY-RECONSTRUCTED bytes of a selected/stopped version
(`WalrusFile::reconstruct(Version) -> Result<Vec<u8>>`) plus the manifest's
`document.name` and `document.mime`.

### 7.1 Module restructuring and function shapes

The `export` module is RESTRUCTURED so the metadata surface is always compiled
while the heavy backends stay feature-gated. Current state (verified):
`crates/walrus/src/lib.rs` gates the WHOLE module
(`#[cfg(any(feature = "export-html", ...))] pub mod export;`), and `ExportFormat`
/ `ExportUnavailable` live INSIDE `export.rs`. With no export feature, none of
them compile -- so `offered_formats`/`to_original` could not exist. The fix is
exact:

- In `lib.rs`, make `pub mod export;` UNGATED (remove the `#[cfg(any(...))]`).
- Inside the `export/` module, ALWAYS compile: `ExportFormat`,
  `ExportUnavailable`, `offered_formats(mime) -> Vec<ExportFormat>`, and
  `to_original(doc) -> Vec<u8>`.
- GATE ONLY the heavy backend fns/modules with their `#[cfg(feature = "export-*")]`.

Result: `cargo build -p walrus-fmt` (no features) compiles the metadata surface
and links ZERO export crates (boundary proof, section 1 build (1)).

To keep every file under 400 lines, `export.rs` becomes a directory:

```
export/mod.rs      // ExportFormat, ExportUnavailable, offered_formats, re-exports -- ALWAYS compiled
export/original.rs // to_original -- ALWAYS compiled (no feature, no dep)
export/html.rs     // to_html      (cfg export-html)
export/markdown.rs // to_markdown  (cfg export-md)
export/csv.rs      // to_csv       (cfg export-csv)
export/json.rs     // to_json      (cfg export-json)
export/pdf.rs      // to_pdf       (cfg export-pdf)
export/docx.rs     // to_docx      (cfg export-docx)
```

Signatures (library returns `Result`, never panics -- rust-standards):

```
pub fn to_original(doc: &[u8]) -> Vec<u8>                 // verbatim passthrough; ALWAYS built
#[cfg(feature = "export-html")] pub fn to_html(doc: &[u8], mime: &str) -> crate::Result<Vec<u8>>
#[cfg(feature = "export-md")]   pub fn to_markdown(doc: &[u8]) -> crate::Result<Vec<u8>>
#[cfg(feature = "export-csv")]  pub fn to_csv(doc: &[u8]) -> crate::Result<Vec<u8>>
#[cfg(feature = "export-json")] pub fn to_json(doc: &[u8]) -> crate::Result<Vec<u8>>
#[cfg(feature = "export-pdf")]  pub fn to_pdf(doc: &[u8], mime: &str) -> crate::Result<Vec<u8>>
#[cfg(feature = "export-docx")] pub fn to_docx(doc: &[u8], mime: &str) -> crate::Result<Vec<u8>>
```

BREAKING CHANGE to the existing stubs: today `to_markdown`/`to_html`/`to_json`
return `Vec<u8>` (verified in `export.rs`); the design changes them to return
`crate::Result<Vec<u8>>` and adds a `mime: &str` parameter to `to_html` (and to
the new `to_pdf`/`to_docx`). `to_csv` is NET-NEW (no stub exists today). Call
sites and any doc-tests must be updated to the `Result` form. CALLER CHECK:
`grep` for `export::to_` across the workspace found NO current callers (only the
module definition in `export.rs`, this design, and the review docs), so the
breaking change touches no production call site -- only the stubs' own bodies and
any doc-tests added with them. `crate::Result` is the already-exported
`walrus_fmt::Result` alias (`pub use error::{Result, WalrusError}` in `lib.rs`).

ORIGINAL export writes the reconstructed bytes VERBATIM; the caller names the
file from `manifest().document.name` (a stored `report.html` is written back as
`report.html`, byte-for-byte). `to_original` is NOT feature-gated: it is a
passthrough with no dependency and must always be available. The `.wal` is a tar
bundle whose stored document carries its original name; "Original" simply selects
a version and writes those bytes under that name.

### 7.2 Crate choices, EXACT versions, feature gating

| Format   | Feature       | Crate(s) + EXACT version                               | MSRV          |
|----------|---------------|--------------------------------------------------------|---------------|
| Original | (none)        | -- (verbatim passthrough)                               | 1.75          |
| HTML     | `export-html` | `pulldown-cmark = "0.11"` (md->html); `scraper = "0.20"` (HTML text extraction) | 1.75 / see 7.3 |
| Markdown | `export-md`   | `pulldown-cmark = "0.11"`                               | 1.75 / see 7.3 |
| CSV      | `export-csv`  | `csv = "1.3"`                                           | 1.75          |
| JSON     | `export-json` | `serde_json` (already a non-optional dep)              | 1.75          |
| PDF      | `export-pdf`  | `genpdf = "0.2"` PREFERRED; `printpdf = "0.7"` fallback | see 7.3       |
| DOCX     | `export-docx` | `docx-rs = "0.4"`                                       | see 7.3       |

Each `Cargo.toml` dependency is `optional = true`, pulled in by its feature:

```toml
[dependencies]
pulldown-cmark = { version = "0.11", optional = true }
scraper        = { version = "0.20", optional = true }
csv            = { version = "1.3",  optional = true }
genpdf         = { version = "0.2",  optional = true }
docx-rs        = { version = "0.4",  optional = true }

[features]
export-html = ["dep:pulldown-cmark", "dep:scraper"]
export-md   = ["dep:pulldown-cmark"]
export-csv  = ["dep:csv"]
export-json = []                      # serde_json is already non-optional
export-pdf  = ["dep:genpdf"]
export-docx = ["dep:docx-rs"]
export-all  = ["export-html", "export-md", "export-csv", "export-json", "export-pdf", "export-docx"]
```

Backend behaviour:

- `to_html`: if the source is Markdown (`mime` is `text/markdown`), render via
  `pulldown-cmark` to an HTML fragment wrapped in a minimal `<!doctype html>`
  document using the ACCEPTABLE SUBSET (no scripts, no remote links rewritten).
  If the source is already HTML, pass it through wrapped/escaped. The subset
  matches the viewer's subset so what is shown is what is exported.
- `scraper` extracts plain text from HTML for the Plain-Text and PDF/DOCX flows
  (strip tags, keep text and block breaks). It is behind `export-html` because
  that feature needs HTML parsing.
- `to_markdown`: passthrough for a markdown source; for other text it emits the
  text in a fenced block.
- `to_csv`: only offered when `offered_formats` says the document is tabular
  (CSV in, CSV out round-trip via the `csv` crate). Not offered for prose.
- `to_json`: wrap text + minimal metadata in a JSON object via `serde_json`
  pretty.
- `to_pdf`: lay the extracted text out with `genpdf` (default font, flow
  pagination). CSS-perfect PDF explicitly NOT required. Output must be a
  non-empty PDF beginning with `%PDF-` magic bytes.
- `to_docx`: build a `.docx` (zip of `word/document.xml` + parts) from the
  extracted text via `docx-rs`. Output must be a valid zip containing the
  expected parts.

### 7.3 MSRV resolution (explicit, do NOT silently break)

The WORKSPACE pins `rust-version = "1.75"` in `[workspace.package]` (verified);
`walrus-fmt` inherits it via `rust-version.workspace = true`. The real knob is
therefore the WORKSPACE value, not a per-package override. RISK crates are
`genpdf`, `docx-rs`, and `scraper` (its `selectors`/`html5ever` tree has
historically needed newer toolchains). As noted in section 1, the SHIPPED build
(`cargo build`) always enables `export-all` via `walker`, so this gate applies to
the DEFAULT workspace build, not an opt-in extra.

DECISION procedure (locked):

1. At implementation time, after adding each export crate at the version above,
   verify with `cargo +1.75.0 build -p walrus-fmt --features export-all` (or
   `cargo msrv`).
2. If every crate builds on 1.75, keep `rust-version = "1.75"` unchanged.
3. If ANY enabled export crate requires > 1.75, raise the WORKSPACE
   `rust-version` (in `[workspace.package]`, which `walrus-fmt` inherits) to the
   LOWEST value that satisfies all enabled crates. Record the forcing crate in
   the workspace `Cargo.toml` (a comment on the `rust-version` line) and in
   `ROADMAP.md`. Do NOT add a per-package `rust-version` override on `walrus-fmt`
   -- that would break the `rust-version.workspace = true` inheritance.
4. PDF fallback: if `genpdf 0.2` forces an MSRV above an acceptable bound (post
   ~1.80) or fails to resolve, switch PDF to `printpdf = "0.7"` (text/flow
   pagination is sufficient).
5. DOCX: DOCX has NO lightweight alternative. If `docx-rs 0.4` forces an MSRV
   above ~1.80, KEEP `docx-rs` and accept + DOCUMENT its MSRV. Do NOT attempt a
   hand-rolled OOXML writer: the only archive crate in the graph is `tar`, which
   cannot produce the ZIP container a `.docx` requires.

The design does NOT silently break MSRV: the build gate makes any required bump
explicit, reviewed, and recorded. The only runtime-discovered input is the exact
MSRV number each crate reports, surfaced deterministically by the command above.

### 7.4 Walker export UI (export_ui.rs)

`offered_formats(mime: &str) -> Vec<ExportFormat>` lives in `walrus-fmt` and is
ALWAYS compiled (section 7.1), needing NO export feature -- it is pure data. It
returns the formats appropriate to the current document, always including
`PlainText`, `Html`, `Pdf`, `Docx`, plus `Original`, adding `Markdown` for
markdown/plain sources and `Csv` ONLY for `text/csv`. The walker export menu
lists exactly these, opens the `rfd` save dialog (already a walker dependency)
with the matching extension pre-filled (Original uses `document.name`), calls the
matching `walrus_fmt::export::to_*`, and writes the bytes. A disabled/absent
feature surfaces as `ExportUnavailable` -> a status-bar error (walker builds
`walrus-fmt` with `export-all`, so all backends are present in the shipped
binary).

Error handling for export (concrete, per operation):

- Reconstruct failure (bad index / corrupt log): fatal to the export action,
  recoverable to the app; the caller receives `walrus_fmt::Result::Err`, shown in
  the status bar, no file written. Logged at the app layer via the status string.
- Backend conversion failure (invalid UTF-8 for a text target, PDF/DOCX builder
  error): returned as `Err`, shown in status bar, no partial file written (build
  fully in memory, then write once).
- File write failure (`rfd` returned a path but the write failed): `std::io::Error`
  surfaced as a status-bar error; recoverable (retry/another path). Logged via
  the status string.
- User cancels the `rfd` dialog: no-op, no error.
- Feature not compiled: `ExportUnavailable` with the format name; status-bar
  message naming which cargo feature to enable.

Validation of external input (stored document bytes are external/untrusted):

- Text targets (`PlainText`, `Markdown`, `Html`, `Json`): bytes decoded with
  `String::from_utf8_lossy` (lossy, never panics; declared encoding is utf-8 per
  manifest). No size limit imposed by the library; export is synchronous on
  single documents, not streams.
- CSV: parsed by the `csv` crate; a parse error returns `Err` (shown), never
  panics. Ragged rows preserved as-is (round-trip).
- JSON pretty-print: if the bytes are not valid JSON, `to_json` wraps the raw
  text rather than failing (keeps the forgiving contract).

Invariant ownership: the "Original export is byte-identical to the stored
document" invariant is OWNED by `walrus-fmt` (`to_original` + the caller using
`document.name`), because only the library knows the stored bytes and the
manifest name; the GUI must not re-encode on the Original path. The "offered
formats match the document type" invariant is OWNED by
`walrus_fmt::offered_formats` (pure, tested) so the GUI and any other embedder
agree.

---

## 8. Verification plan

Follow TDD (testing steering): write the failing test first for each criterion.
With egui 0.36 (section 0), a headless `egui_kittest` harness IS available, so
rendered-widget behaviour (transport controls, seek, the CSV grid) is driven by
a headless harness rather than deferred to manual verification. The upgrade task
already proves the harness end to end with one passing test
(`crates/walker/tests/harness.rs`): it builds a `Harness` around a "step >"
transport button plus a version label backed by a real `player::Player`, clicks
the button through the accessibility tree, runs a FIXED, bounded number of
frames, and asserts the player index advanced and the label re-rendered -- no
pixels, no frame-count assertions, no loop-until-timeout. The CSV-viewer task
extends this pattern to the renderers it adds. Pure model logic
(`select_renderer`, `interval_for_multiplier`, `Flavor`/visuals transitions, HTML
/MD lowering) still gets its own pure unit tests; only things that genuinely
cannot be driven headlessly stay manual (named below with reasons).

Pure unit tests (no GUI, in the owning crate):

- `select_renderer` mapping (`view/selector.rs`): every MIME row (including
  `application/xml`/`text/xml` -> `Code(Xml)`), every extension fallback, the
  `text/plain`+`.md` refinement, and the unknown -> `Text` default. One assertion
  per scenario, sentence-named.
- HTML-subset lowering (`view/html.rs`): a fixture with headings, bold/italic,
  lists, a table, a link, a script tag, and an `img` lowers to the expected
  `Vec<Block>` with the script DROPPED and the image a placeholder (asserts the
  untrusted-content rule directly).
- Markdown lowering (`view/markdown.rs`): an equivalent fixture lowers to the
  same block shapes (shared `pulldown-cmark = "0.11"` event model, section 9).
- Speed (`transport.rs` + `player.rs`): `interval_for_multiplier` arithmetic
  (400 ms at 1x, 100 ms at 4x, 1600 ms at 0.25x); `set_step_interval` clamps
  below 16 ms; a fixed interval advances the expected index count over a known
  elapsed sum. The three existing loop/wrap tests stay unchanged.
- Theme (if a picker is added in the CSV-viewer task): built on egui's own
  `Visuals` (catppuccin-egui is dropped, section 0). The theme-selection state
  transition is a pure function (selected mode -> `egui::Visuals`), unit-tested
  without pixels; the resulting colours stay manual.
- Export backends (`walrus-fmt`, feature-gated test modules): `to_html` emits
  well-formed HTML (doctype present, balanced wrapper, input escaped); `to_csv`
  round-trips rows; `to_json` is valid JSON; `to_pdf` output is non-empty and
  starts with `%PDF-`; `to_docx` output is a zip whose directory contains
  `word/document.xml` (+ `[Content_Types].xml`); `to_original` returns the input
  bytes UNCHANGED and a caller test confirms the chosen filename equals
  `document.name`.
- `offered_formats` (`walrus-fmt`, ALWAYS-compiled test): `text/csv` includes
  `Csv`; prose MIME omits `Csv`; `Original` and `PlainText` always present. This
  test compiles and passes with NO export feature enabled (boundary proof).

Boundary build checks (CI/manual commands, section 1):

- `cargo build -p walrus-fmt` (no features) MUST compile and MUST NOT link
  `pulldown-cmark` / `scraper` / `csv` / `genpdf` / `docx-rs` (e.g. assert via
  `cargo tree -p walrus-fmt -i <crate>` returning not-found).
- `cargo build` (whole workspace, hence walker's `export-all`) MUST build all
  backends; this is the build the 7.3 MSRV gate must pass.

MANUAL verification (stated WHY -- these genuinely cannot be driven by a headless
`egui_kittest` harness; widget-interaction behaviour that CAN be driven headlessly
is now a harness test, not manual):

- Pixel-exact appearance: the built-in-theme colours and that rendered richtext
  (HTML/Markdown) and the CSV grid LOOK correct -- paint output is not
  deterministically assertable without a GPU/screenshot.
- `rfd` native open/save dialog -- a real OS-native dialog that cannot be driven
  by a headless accesskit harness.

Integration round-trip (`walrus-fmt` + walker integration test): open
`examples/walrus-story.wal` (its `document.mime` is `text/plain`), confirm
`select_renderer` picks the Text/Code path, reconstruct a chosen version, and
export that version to EACH offered format (`Original`, `PlainText`, `Html`,
`Markdown`, `Pdf`, `Docx`), asserting each backend's well-formed-output check
above and that `Original` reproduces the stored bytes under the stored filename.
Use `tempfile::TempDir` for any on-disk assertion (testing steering).

---

## 9. Files touched (summary)

walrus-fmt (`crates/walrus`):

- `Cargo.toml`: add optional deps + `dep:` feature wiring (section 7.2); bump the
  WORKSPACE `rust-version` ONLY if the 7.3 build gate requires it (recorded with
  the forcing crate).
- `src/lib.rs`: make `pub mod export;` UNGATED (remove the `#[cfg(any(...))]`);
  the metadata surface compiles with no features (section 7.1).
- `src/export.rs` -> split into `src/export/mod.rs` (always-compiled
  `ExportFormat`/`ExportUnavailable`/`offered_formats` + re-exports),
  `export/original.rs` (always-compiled `to_original`), and the feature-gated
  `html.rs`/`markdown.rs`/`csv.rs`/`json.rs`/`pdf.rs`/`docx.rs`. Note the breaking
  signature change (section 7.1): existing stub bodies and any doc-tests move to
  `-> crate::Result<Vec<u8>>` with the new `mime` param; `to_csv` is net-new.

walker (`crates/walker`):

- `Cargo.toml`: add `egui_extras = { version = "0.36", default-features = false, features = ["syntax_highlighting"] }`
  (no `table` feature -- `TableBuilder` is unconditional in egui_extras);
  `pulldown-cmark = "0.11"` and `csv = "1.3"` (SAME minors as walrus-fmt in 7.2,
  so the viewer's Markdown/CSV behaviour matches the `to_html` exporter -- "what
  is shown is what is exported"). NO catppuccin-egui (dropped; egui built-in
  theming). `egui_kittest = "0.36"` is already a `[dev-dependencies]` entry added
  by the upgrade task.
- `src/app.rs`: remove the `View` enum and the fixed CSV tab; delegate to the new
  modules; hold `Flavor` and the speed multiplier.
- Add `src/chrome.rs`, `src/transport.rs`, `src/theme.rs`, `src/export_ui.rs`,
  and the `src/view/` tree (section 3). Each new file targets well under 400
  non-test lines; `richtext.rs` is the split seam if `html.rs`/`markdown.rs` grow.

ROADMAP.md: move "CSV column viewer", "Export backends (pdf/docx)", and the GUI
test-approach decision from Stubbed/TODO to Done as each lands (the GUI test
approach is a headless `egui_kittest` harness, enabled by the egui-0.36 upgrade,
backed by pure-logic tests); record any MSRV bump and its forcing crate (egui
0.36 already forced 1.75 -> 1.95).

---

## 10. Locked decisions (summary)

0. egui minor = 0.36, applied uniformly to `egui`, `eframe`, `egui_extras`, and
   `egui_kittest`. `egui_kittest = "0.36"` IS the headless GUI harness; GUI
   behaviour is verified by a headless harness test plus pure-logic tests, with a
   short manual list for pixel-exact appearance and the `rfd` dialog. egui 0.36
   raised the workspace MSRV to 1.95. catppuccin-egui is DROPPED (egui built-in
   theming).
1. Renderer chosen from MIME with an extension fallback; `Html`, `Markdown`,
   `Csv`, `Code(lang)`, `Text`; CSV only for actual CSV; XML covered in BOTH the
   MIME table (`application/xml`, `text/xml`) and the extension table.
2. HTML/Markdown render natively to a shared `Block` model -- NO webview
   (decided), untrusted content, no scripts/remote fetches.
3. Code/JSON/TOML via `egui_extras = "0.36" syntax_highlighting` (confirm the
   exact `highlight`/`CodeTheme` signature on pin); JSON pretty-printed; unknown
   language -> monospace. COBOL/syntect/fenced-highlighting deferred.
4. Speed = multiplier (0.25x..4x presets + fine slider) over a 400 ms base via
   the pure `interval_for_multiplier`, driving the existing
   `Player::set_step_interval`; player state machine unchanged.
5. Theming via egui's built-in `Visuals` / `ThemePreference` (catppuccin-egui
   DROPPED); System/Light/Dark modes, default Dark; picker only.
6. FFWB-style chrome (menu bar + toolbar + content + transport + status bar),
   NO command line, NO `ff-*` dependency.
7. Export: `pub mod export` UNGATED with an always-compiled metadata surface
   (`ExportFormat`, `ExportUnavailable`, `offered_formats`, `to_original`); heavy
   backends feature-gated: HTML (`pulldown-cmark 0.11` + `scraper 0.20`), Markdown
   (`pulldown-cmark 0.11`), CSV (`csv 1.3`), JSON (`serde_json`), PDF (`genpdf 0.2`,
   `printpdf 0.7` fallback), DOCX (`docx-rs 0.4`). Breaking stub change to
   `-> crate::Result<Vec<u8>>` + `mime` param (no existing callers). MSRV knob is
   the WORKSPACE `rust-version` (1.75); raise it only if a crate forces it, record
   the forcing crate; no hand-rolled OOXML. The 7.3 gate applies to the default
   workspace build (walker enables `export-all`).
8. Verification: pure unit tests for selection/lowering/speed/theme/export;
   two boundary builds (`-p walrus-fmt` no-feature links zero export crates; full
   workspace builds all backends and is the MSRV gate); manual only for the `rfd`
   dialog and pixel-exact appearance/wiring; plus a `walrus-story.wal` round-trip
   to every offered format.

---

## Review responses

All HIGH and MEDIUM findings from `design-review.json` are resolved; the two NITs
are also addressed.

The egui-0.36 upgrade task resolves the GUI-stack findings (1, 2, 4) as follows.
The export findings (3, 5, 6, 7, 8) and the NITs (9, 10) are DEFERRED to the
later CSV-viewer task (scope note under section 0); their original resolutions
below still stand as the plan for that task.

- Finding 1 (HIGH -- egui_kittest has no egui-0.28 release). RESOLVED by bumping
  the whole egui stack to 0.36 (not by dropping the harness). `egui = "0.36"`,
  `eframe = "0.36"`, and `egui_kittest = "0.36"` as a walker dev-dependency IS
  now the headless harness. A real headless harness test
  (`crates/walker/tests/harness.rs`) proves it end to end: it drives a transport
  button and asserts the player index and version label (sections 0, 8). egui
  0.36 raised the workspace MSRV to 1.95 (recorded in `Cargo.toml` and
  `ROADMAP.md`). Every egui-side crate shares the one 0.36 minor (section 0).
- Finding 2 (HIGH -- egui_extras unpinned / assumed API / nonexistent table
  feature). RESOLVED: the single egui-minor constant (section 0) now pins
  `egui_extras = "0.36"` with `default-features = false`,
  `features = ["syntax_highlighting"]` and NO `table` feature, FOR WHEN the
  CSV-viewer task adds the Code/CSV renderers that need it. The upgrade task does
  NOT add `egui_extras` because walker's current source does not use it (so
  nothing is taken on faith against an unpinned version); the exact
  `syntax_highlighting` API is pinned down by the CSV-viewer task against the
  0.36 crate it depends on.
- Finding 3 (MEDIUM -- offered_formats/to_original under gated module). DEFERRED
  to the CSV-viewer task. Plan: section 7.1 specifies the restructure exactly --
  `pub mod export` UNGATED; `ExportFormat`/`ExportUnavailable`/`offered_formats`/
  `to_original` always compiled; only heavy backend fns/modules gated;
  `cargo build -p walrus-fmt` (no features) compiles the metadata surface and
  links zero export crates.
- Finding 4 (MEDIUM -- catppuccin egui28 vs the egui minor). RESOLVED: section 0
  states the egui minor as one constant (0.36) and DROPS catppuccin-egui entirely
  in favour of egui's built-in theming, removing the cross-section tension; the
  one-line statement that the minor applies uniformly to egui, eframe,
  egui_extras, and egui_kittest remains.
- Finding 5 (MEDIUM -- zero-export-crates claim vs walker's export-all). DEFERRED
  to the CSV-viewer task.
  section 1 states the two explicit builds and their meanings; section 7.3 notes
  the MSRV gate applies to the DEFAULT workspace build; section 8 lists both
  build checks.
- Finding 6 (MEDIUM -- breaking stub signature change). DEFERRED to the
  CSV-viewer task. Plan: section 7.1 calls out the breaking change
  (`-> crate::Result<Vec<u8>>`, new `mime` param), marks `to_csv` net-new, and
  records the grep result: NO current callers of `export::to_` exist in the
  workspace.
- Finding 7 (MEDIUM -- MSRV knob + garbled DOCX clause). DEFERRED to the
  CSV-viewer task for the EXPORT-crate MSRV question. Note: the egui-0.36 upgrade
  task has already exercised the same WORKSPACE `rust-version` knob -- egui 0.36
  forced it from 1.75 to 1.95 (recorded in `Cargo.toml` and `ROADMAP.md`), so the
  mechanism in section 7.3 (raise the inherited workspace value, record the
  forcing crate) is proven. The DOCX-specific plan (keep/document `docx-rs`, no
  hand-rolled OOXML) still stands for the later task.
- Finding 8 (MEDIUM -- walker-side pulldown-cmark/csv unpinned). DEFERRED to the
  CSV-viewer task. Plan: section 9 pins the walker-side `pulldown-cmark = "0.11"`
  and `csv = "1.3"` to match section 7.2.
- Finding 9 (NIT -- per-file line budget). DEFERRED to the CSV-viewer task (it
  adds the new files). Plan: section 3's explicit under-400 target and the
  `richtext.rs` split seam. The upgrade task kept every touched `.rs` file well
  under 400 non-test lines (`app.rs` is ~230).
- Finding 10 (NIT -- missing XML MIME rows). DEFERRED to the CSV-viewer task (it
  adds `select_renderer`). Plan: the section 2.1 MIME table includes
  `application/xml`, `text/xml` -> `Code(Xml)`.
