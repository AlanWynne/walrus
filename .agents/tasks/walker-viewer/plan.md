# Implementation Plan -- Walker CSV Viewer + walrus-fmt Export Backends

Source of truth: `design.md` (REVIEW-APPROVED) in this directory. This plan
sequences that design; it does not re-decide architecture. All work happens in
the worktree `c:\workspace\Walrus\.worktrees\csv-viewer` (cwd for every command).
Relative paths resolve against the parent workspace, so always use the worktree
absolute path or run with that cwd.

TDD is mandatory (testing steering): for each acceptance criterion write the
FAILING test first, run `cargo test` to watch it fail (red), implement the
minimum to pass (green), refactor green. Never skip the red step.

Hard invariants a reviewer WILL check (keep true at every step):

- `walrus-fmt` stays GUI-free and application-free. `cargo build -p walrus-fmt`
  (no features) compiles the always-present export metadata surface and links
  NONE of pulldown-cmark / scraper / csv / genpdf / docx-rs. Prove with
  `cargo tree -p walrus-fmt`. No egui/eframe ever in walrus-fmt.
- FORMAT.md is unchanged.
- Every `.rs` file is UNDER 400 non-test lines (exclude the `#[cfg(test)]`
  module). `richtext.rs` is the split seam for html/markdown.
- Strict ASCII in `.rs`; ASCII-only markdown (`--`, `->`, `<->`, `<=`, `>=`;
  no em dashes, curly quotes, or unicode arrows).
- No `unwrap()` / `expect()` in walrus-fmt library code: use `thiserror` +
  `crate::Result`. walker is a binary (`anyhow` `.context`; `expect("why")`
  tolerable). Tests use `expect("why")`.
- Untrusted content: HTML/Markdown render a safe subset only; never execute
  scripts or fetch remote resources. No webview, no `ff-*`, no command line.

Verified starting state (do not re-discover):

- Workspace is already on egui/eframe 0.36 and `rust-version = "1.95"` (the
  egui-0.36 upgrade task landed; see git log `3ec1712`). The MSRV knob is the
  workspace `[workspace.package]` `rust-version`; walrus-fmt inherits it.
- `crates/walrus/src/lib.rs` gates the WHOLE `export` module behind
  `#[cfg(any(feature = "export-*"))]`; `ExportFormat` / `ExportUnavailable` live
  inside `export.rs`; current stub fns return `Vec<u8>` (`to_markdown`,
  `to_html`, `to_json`); there is NO `to_csv`.
- Grep across the workspace confirms NO caller of `export::to_*` exists outside
  `export.rs` itself -- the breaking signature change touches no production call
  site.
- `crates/walrus/Cargo.toml` declares the `export-*` features with EMPTY dep
  lists (no optional deps yet). `serde_json` is already a non-optional dep.
- `crates/walker/Cargo.toml` has eframe 0.36, egui 0.36, rfd 0.14, and
  `egui_kittest 0.36` in dev-deps. It does NOT yet have egui_extras,
  pulldown-cmark, or csv. walker depends on walrus-fmt with
  `features = ["export-all"]`.
- `crates/walker/src/app.rs` already uses the eframe 0.36 `logic`/`ui` split and
  `egui::Panel::top`/`bottom`. It still has the old `View` enum (Player/Csv) and
  the fixed CSV tab to remove.
- `crates/walker/tests/harness.rs` is the egui_kittest pattern to copy for new
  rendered-widget tests.
- `examples/walrus-story.wal` exists (document.mime `text/plain`) -- the section
  8 integration fixture.
- `manifest().document` is a `DocumentInfo { encoding, mime, name }` (public
  fields). walker reaches mime/name via `player`/`WalrusFile::manifest()`.

Local toolchain is 1.98.1 (satisfies 1.95). `cargo-gate` wraps fmt + clippy +
build + nextest; `cargo test` / `cargo clippy -- -D warnings` / `cargo fmt` are
the direct commands.

---

## Part A -- walrus-fmt export restructure + real backends (design 7, 9)

- [ ] 1. Make `pub mod export;` UNGATED and split the metadata surface out of the
      gated code. In `crates/walrus/src/lib.rs` remove the `#[cfg(any(feature =
      "export-*"))]` attribute on `pub mod export;`. Turn `export.rs` into a
      directory module `export/mod.rs` holding the ALWAYS-compiled surface:
      `ExportFormat` (keep `#[non_exhaustive]`, add an `Original` variant),
      `ExportUnavailable`, and re-exports; move the current gated stub bodies out
      to their own files in step 3. Keep the module compiling with no features.
      Files: crates/walrus/src/lib.rs, crates/walrus/src/export.rs ->
      crates/walrus/src/export/mod.rs
      Verify: `cargo build -p walrus-fmt` (no features) compiles; `cargo build
      -p walrus-fmt --features export-all` compiles.

- [ ] 2. TDD the always-compiled metadata surface. First write failing tests in
      `export/mod.rs` `#[cfg(test)]`: `offered_formats("text/csv")` includes
      `Csv`; a prose mime (`text/plain`) OMITS `Csv`; `Original` and `PlainText`
      are always present; markdown/plain sources add `Markdown`. These tests MUST
      compile and pass with NO export feature (the boundary proof). Then
      implement `offered_formats(mime: &str) -> Vec<ExportFormat>` and
      `to_original(doc: &[u8]) -> Vec<u8>` (verbatim passthrough) in
      `export/original.rs` (to_original) and `export/mod.rs` (offered_formats),
      both ungated.
      Files: crates/walrus/src/export/mod.rs, crates/walrus/src/export/original.rs
      Verify: `cargo test -p walrus-fmt` (default, no features) -- the new
      offered_formats/to_original tests pass; `cargo tree -p walrus-fmt` lists
      none of pulldown-cmark/scraper/csv/genpdf/docx-rs.

- [ ] 3. Add the export crate dependencies and feature wiring at the EXACT
      versions from design 7.2. In `crates/walrus/Cargo.toml` add optional deps
      `pulldown-cmark = "0.11"`, `scraper = "0.20"`, `csv = "1.3"`,
      `genpdf = "0.2"`, `docx-rs = "0.4"` and wire the features:
      `export-html = ["dep:pulldown-cmark", "dep:scraper"]`,
      `export-md = ["dep:pulldown-cmark"]`, `export-csv = ["dep:csv"]`,
      `export-json = []`, `export-pdf = ["dep:genpdf"]`,
      `export-docx = ["dep:docx-rs"]`, `export-all` = all six.
      Files: crates/walrus/Cargo.toml
      Verify: `cargo build -p walrus-fmt --features export-all` resolves and
      compiles (crate selection is proven here before any backend code uses it).

- [ ] 4. MSRV gate on walrus-fmt with export-all (design 7.3). After step 3,
      confirm the shipped-build toolchain requirement. Run the build on the MSRV
      pin: `cargo +1.95.0 build -p walrus-fmt --features export-all` if a 1.95
      toolchain is installed, else record that the local 1.98.1 build succeeds
      and note the gate was not run on an exact-MSRV toolchain. If ANY export
      crate requires > 1.95, raise the WORKSPACE `rust-version` in
      `[workspace.package]` to the lowest satisfying value, add a comment naming
      the forcing crate on that line, and record it in ROADMAP.md. If `genpdf
      0.2` fails to resolve or forces MSRV past ~1.80, switch PDF to
      `printpdf = "0.7"` (design 7.3 step 4). Keep `docx-rs` regardless and
      document its MSRV if it forces a bump (design 7.3 step 5); never hand-roll
      OOXML. Do NOT add a per-package rust-version override.
      Files: Cargo.toml (only if a bump is required), ROADMAP.md,
      crates/walrus/Cargo.toml (comment only if PDF backend swapped)
      Verify: `cargo build --features walrus-fmt/export-all` for the workspace
      succeeds on the pinned MSRV (or the local toolchain with the caveat
      recorded). If a crate's real MSRV conflicts with design assumptions,
      SURFACE it (send_message warning) before diverging.

- [ ] 5. TDD `to_csv` (net-new, feature `export-csv`). Write a failing test in
      `export/csv.rs` `#[cfg(test)]` under `#[cfg(feature = "export-csv")]`:
      round-trip a small CSV (rows with a header) through `to_csv` and assert the
      output parses back to the same rows (ragged rows preserved). Implement
      `to_csv(doc: &[u8]) -> crate::Result<Vec<u8>>` using the `csv` crate;
      return `Err` on parse failure (never panic).
      Files: crates/walrus/src/export/csv.rs
      Verify: `cargo test -p walrus-fmt --features export-csv to_csv` passes.

- [ ] 6. TDD + implement the text-ish backends with the BREAKING signature change
      to `crate::Result<Vec<u8>>`. `to_html(doc, mime: &str)` (feature
      export-html): write a failing test asserting the output has a `<!doctype
      html>`, a balanced wrapper, and HTML-escapes `<`, `>`, `&`; when `mime` is
      `text/markdown`, render via pulldown-cmark to the safe subset (no scripts,
      no remote fetch). `to_markdown(doc)` (feature export-md): passthrough for a
      markdown source, fenced block for other text; test the fenced-block shape.
      `to_json(doc)` (feature export-json): wrap text in a JSON object via
      serde_json pretty; test the output parses as valid JSON and is forgiving on
      non-JSON input. Each moves to its own file; `scraper` is used for HTML text
      extraction behind export-html.
      Files: crates/walrus/src/export/html.rs,
      crates/walrus/src/export/markdown.rs, crates/walrus/src/export/json.rs
      Verify: `cargo test -p walrus-fmt --features export-html,export-md,export-json`
      passes the new backend tests.

- [ ] 7. TDD + implement the heavy binary backends. `to_pdf(doc, mime: &str)`
      (feature export-pdf): extract text (via scraper for HTML sources, else
      lossy UTF-8), lay out with genpdf (or printpdf if swapped in step 4); test
      the output is non-empty and starts with the `%PDF-` magic bytes.
      `to_docx(doc, mime: &str)` (feature export-docx): build a `.docx` with
      docx-rs from the extracted text; test the output is a zip whose directory
      contains `word/document.xml` (and `[Content_Types].xml`). All return
      `crate::Result<Vec<u8>>`, build fully in memory, never panic.
      Files: crates/walrus/src/export/pdf.rs, crates/walrus/src/export/docx.rs
      Verify: `cargo test -p walrus-fmt --features export-pdf,export-docx` passes.

- [ ] 8. Confirm the full library export surface and the boundary proof together.
      Ensure `export/mod.rs` re-exports every backend under its cfg and that no
      split file exceeds 400 non-test lines.
      Files: crates/walrus/src/export/*.rs
      Verify: `cargo test -p walrus-fmt --features export-all` all pass;
      `cargo test -p walrus-fmt` (no features) still passes the boundary tests;
      `cargo tree -p walrus-fmt` (no features) links none of the heavy crates;
      `cargo clippy -p walrus-fmt --all-features -- -D warnings` is clean.

## Part B -- walker viewer tree (design 2, 3, 5, 6)

- [ ] 9. Add walker viewer dependencies at the design 9 versions. In
      `crates/walker/Cargo.toml` add `egui_extras = { version = "0.36",
      default-features = false, features = ["syntax_highlighting"] }` (NO `table`
      feature), `pulldown-cmark = "0.11"`, and `csv = "1.3"`. Before relying on
      syntax highlighting, CONFIRM the exact egui_extras 0.36
      `syntax_highlighting::highlight(..)` / `CodeTheme` signature against the
      pinned crate (read the vendored source under the Cargo registry or
      `cargo doc -p egui_extras --open`); record the confirmed arity in a comment
      in `view/code.rs`. If the real API differs from design 2.1, SURFACE it
      (send_message warning) rather than diverging silently.
      Files: crates/walker/Cargo.toml
      Verify: `cargo build -p walker` compiles with the new deps present.

- [ ] 10. TDD + implement `select_renderer` (pure, no egui) in
      `src/view/selector.rs` with the `Renderer` and `CodeLang` enums. Write
      failing tests FIRST covering every row of design 2.1: each MIME (including
      `application/xml` and `text/xml` -> `Code(Xml)`), each extension fallback,
      the `text/plain` + `.md` refinement to Markdown, and unknown -> `Text`. One
      sentence-named assertion per scenario. Then implement
      `select_renderer(mime, name) -> Renderer`. Add `src/view/mod.rs` as a
      re-export/dispatch module (no logic beyond selection re-export).
      Files: crates/walker/src/view/selector.rs, crates/walker/src/view/mod.rs,
      crates/walker/src/lib.rs (add `pub mod view;`)
      Verify: `cargo test -p walker select_renderer` -- all mapping tests pass.

- [ ] 11. TDD + implement the shared Block/Inline model and the HTML/Markdown
      lowering (design 2.3). First write failing PURE tests: an HTML fixture with
      headings, bold/italic, a list, a table, a link, a `<script>` tag, and an
      `<img>` lowers to the expected `Vec<Block>` with the script DROPPED and the
      image a placeholder (asserts the untrusted rule); an equivalent Markdown
      fixture lowers to the same block shapes. Implement `Block`/`Inline` +
      painter in `src/view/richtext.rs` (the SPLIT SEAM), HTML-subset lowering in
      `src/view/html.rs`, and pulldown-cmark lowering in `src/view/markdown.rs`.
      The lowering fns are pure (bytes -> `Vec<Block>`); keep each file under 400
      non-test lines, moving inline-vs-block halves into richtext.rs if html.rs or
      markdown.rs approaches the limit.
      Files: crates/walker/src/view/richtext.rs, crates/walker/src/view/html.rs,
      crates/walker/src/view/markdown.rs
      Verify: `cargo test -p walker` -- the html/markdown lowering tests pass,
      asserting script-dropped and img-placeholder.

- [ ] 12. Implement the CSV grid, Code highlighting, and Text fallback views.
      `src/view/csv.rs`: parse with the `csv` crate, render an aligned scrollable
      grid via `egui_extras::TableBuilder` (TableBuilder is unconditional; no
      `table` feature). `src/view/code.rs`: JSON pretty-print via serde_json
      (raw on parse failure), highlight via egui_extras syntax_highlighting using
      the confirmed signature, unknown language -> monospace. `src/view/text.rs`:
      read-only monospace fallback. Pure helpers (JSON pretty-print, CSV parse to
      rows) get unit tests; the grid/highlight painting is covered by the harness
      test in step 15 or stays MANUAL with a stated WHY where it cannot be driven
      headlessly.
      Files: crates/walker/src/view/csv.rs, crates/walker/src/view/code.rs,
      crates/walker/src/view/text.rs
      Verify: `cargo test -p walker` passes; `cargo build -p walker` compiles.

- [ ] 13. TDD + implement the transport speed control (design 4). Write failing
      tests FIRST: `interval_for_multiplier` returns 400 ms at 1x, 100 ms at 4x,
      1600 ms at 0.25x; `Player::set_step_interval` clamps a sub-16 ms interval to
      16 ms (remove the `#[allow(dead_code)]` once called). Implement
      `interval_for_multiplier(mult: f64) -> Duration` and
      `set_speed_multiplier(player, mult)` in `src/transport.rs`, plus the
      transport controls row (preset stepper 0.25x..4x + fine slider) moved out of
      app.rs. The player loop/stop/wrap/direction state machine and its three
      existing tests stay UNCHANGED.
      Files: crates/walker/src/transport.rs, crates/walker/src/player.rs (drop the
      dead_code allow on set_step_interval), crates/walker/src/lib.rs
      (`pub mod transport;`)
      Verify: `cargo test -p walker interval_for_multiplier` and the clamp test
      pass; the three existing player tests still pass.

- [ ] 14. TDD + implement theming (design 6) and the chrome/export UI shells.
      `src/theme.rs`: `ThemeMode { System, Light, Dark }` (default Dark) with a
      pure `to_preference()` mapping to `egui::ThemePreference`; unit-test the
      pure mode->preference transition (colours stay manual). `src/chrome.rs`:
      menu bar (File/View/Theme) + toolbar + bottom status bar, NO command line.
      `src/export_ui.rs`: list `walrus_fmt::offered_formats(mime)`, open the `rfd`
      save dialog with the matching extension (Original uses `document.name`),
      call the matching `walrus_fmt::export::to_*`, surface `ExportUnavailable`
      and errors to the status bar. Remove the old `View` enum and fixed CSV tab
      from `src/app.rs`; derive the renderer per-document via `select_renderer`
      and keep app.rs under ~250 lines.
      Files: crates/walker/src/theme.rs, crates/walker/src/chrome.rs,
      crates/walker/src/transport.rs, crates/walker/src/export_ui.rs,
      crates/walker/src/app.rs, crates/walker/src/lib.rs (module declarations)
      Verify: `cargo test -p walker theme` passes; `cargo build -p walker`
      compiles; `cargo run -p walker` is a MANUAL launch (long-running GUI; do not
      block an automated step on it).

- [ ] 15. Extend the egui_kittest harness for a rendered viewer behaviour
      (design 8). Following the `tests/harness.rs` pattern, add a deterministic
      headless test for a viewer widget (e.g. the speed slider drives the model
      interval, or the CSV grid renders expected cell labels): fixed bounded
      frames, assert widget ids/labels/model state, never pixels or frame counts,
      never loop-until-timeout. Reserve MANUAL only for pixel-exact appearance and
      the rfd native dialog, each with a stated WHY comment.
      Files: crates/walker/tests/harness.rs (or a new tests/ file per feature area)
      Verify: `cargo test -p walker --test harness` passes.

## Part C -- integration round-trip + verification gate (design 8)

- [ ] 16. Add the section 8 integration round-trip test. Open
      `examples/walrus-story.wal` (document.mime `text/plain`), confirm
      `select_renderer` picks Text/Code, reconstruct a chosen version, and export
      it to EACH offered format (`Original`, `PlainText`, `Html`, `Markdown`,
      `Pdf`, `Docx`), asserting each backend's well-formed-output check (doctype +
      escape for HTML, valid JSON, `%PDF-` prefix, zip-with-`word/document.xml`
      for docx, byte-identical for Original) and that Original reproduces the
      stored bytes under `document.name`. Use `tempfile::TempDir` for on-disk
      assertions. Place it as a walker integration test (walker enables
      export-all) or a walrus-fmt integration test under
      `crates/walrus/tests/`; add `tempfile` to the owning crate dev-deps.
      Files: crates/walker/tests/integration.rs (or
      crates/walrus/tests/export_round_trip.rs), owning crate Cargo.toml dev-deps
      Verify: `cargo test` runs the round-trip and it passes.

- [ ] 17. Full verification gate and ROADMAP update. Run the whole gate and fix
      any failure before considering the work complete.
      Files: ROADMAP.md (move "CSV column viewer", "Export backends (pdf/docx)"
      from Stubbed/TODO to Done; record any MSRV bump + forcing crate)
      Verify, all must pass:
        - `cargo build -p walrus-fmt` (no features) compiles; `cargo tree -p
          walrus-fmt` links none of pulldown-cmark/scraper/csv/genpdf/docx-rs.
        - `cargo build` (whole workspace, walker's export-all) compiles.
        - `cargo test` (whole workspace) passes.
        - `cargo clippy --workspace --all-targets -- -D warnings` is clean
          (and `--all-features` on walrus-fmt).
        - `cargo fmt -- --check` passes.
        - No `.rs` file exceeds 400 non-test lines; strict ASCII in `.rs`;
          ASCII-only markdown.

Notes / assumptions:

- The egui_extras 0.36 `syntax_highlighting` arity is confirmed against the
  pinned crate in step 9 before use (design 2.1 flags it as version-sensitive);
  if it differs, surface via send_message warning.
- The MSRV gate (step 4) uses the local 1.98.1 toolchain if an exact 1.95
  toolchain is unavailable, recording that caveat; any required bump raises only
  the workspace value and names the forcing crate.
- The breaking export signature change touches no production caller (grep-
  confirmed); only the stub bodies and any doc-tests move to the Result form.
