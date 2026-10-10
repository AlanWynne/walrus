# Walrus roadmap / build state

This scaffold establishes the structure, the format spec, and the reader +
reconstruct engine. The following is the honest state of each piece so the next
session can pick up without re-discovering it.

## Done (compiles, tested at unit level)

- `walrus-fmt` crate structure: `error`, `manifest`, `record`, `version`,
  `reconstruct`, `container` (read path), `export` (feature-gated stubs).
- Container READER: zstd decompress + tar member extraction + manifest/log parse.
- Reconstruct engine: replay forward splices to any index, with bounds checks.
- Version addressing: index / base / head / relative / checkpoint-label.
  Unit tests cover stepping, clamping, label lookup, out-of-range.
- `walx` CLI: `info` and `extract` subcommands over the reader.
- `walker` egui app: open dialog, transport (step/play/pause/direction/loop/
  seek), read-only version display, playback state machine (`player.rs`) with
  unit-tested loop/stop/wrap logic.
- GUI test approach decided and wired: egui/eframe/egui_kittest bumped to 0.36;
  `egui_kittest 0.36` is the headless GUI harness (walker `[dev-dependencies]`).
  A real harness test (`crates/walker/tests/harness.rs`) drives a transport
  button and asserts player state. This raised the workspace MSRV to 1.95
  (egui 0.36 forcing crate).
- Export module restructured so the metadata surface is ALWAYS compiled while
  heavy backends stay feature-gated. `pub mod export` is ungated; `ExportFormat`
  (now with an `Original` variant), `ExportUnavailable`, `offered_formats`, and
  `to_original` compile with NO export feature and link zero export crates
  (dependency-boundary proof). `export.rs` split into `export/{mod,original,
  html,markdown,csv,json,pdf,docx}.rs`. Real backends behind `export-*`:
  HTML/Markdown (`pulldown-cmark 0.11` + `scraper 0.20`), CSV (`csv 1.3`),
  JSON (`serde_json`), PDF (`printpdf 0.7`), DOCX (`docx-rs 0.4`). The text
  backends return `crate::Result<Vec<u8>>`; `to_html`/`to_pdf`/`to_docx` take a
  `mime: &str`; `to_csv` is net-new. PDF uses `printpdf` not `genpdf`: genpdf 0.2
  loads a TrueType font family from font FILES on disk at runtime, which a
  GUI-free in-memory backend cannot satisfy without bundling font binaries, so
  printpdf's standard-14 built-in Helvetica was chosen (design 7.2/7.3). No new
  MSRV bump beyond the existing 1.95 (egui 0.36); the export crates build on it.
- Container WRITER (`write.rs`): serialise manifest + base + log into zstd(tar).
  Public API: `WalrusFile::new_wal`, `to_bytes`, `write`. Round-trip unit test
  confirms every version index reconstructs byte-identically after write+read.
- Example `.wal` generators and committed fixtures:
  - `generate_story.rs` -> `examples/walrus-story.wal` (narrated title card).
  - `generate_walk.rs` -> `examples/walrus.wal` (the walking-walrus mascot demo;
    each forward record shifts him one column, self-verified).

## Stubbed / TODO (intentionally not implemented in the scaffold)

1. (DONE -- see "Done" above) Container WRITER: `write.rs` serialises manifest +
   base + log into zstd(tar); `WalrusFile::new_wal` / `to_bytes` / `write`.
2. gzip outer codec (reader currently zstd-only; magic sniffing is in place).
3. `.wal <-> .wbl` conversion (`walx convert`): invert splices, re-base.
4. Timestamp version addressing (`--at T`), per FORMAT.md section 5.
5. (DONE -- see "Done" above) Export backends restructured and implemented.
6. CSV column viewer in `walker` (`View::Csv`): parse + aligned sortable grid.
7. (DONE -- see "Done" above) The walking-walrus `walrus.wal` demo fixture:
   `generate_walk.rs` produces it and `examples/walrus.wal` is committed.
8. (DONE -- see "Done" above) GUI test approach: egui_kittest 0.36 is the chosen
   headless harness (mirrors the FFWB convention), enabled by the egui 0.36
   upgrade.

## Build

The GUI stack is egui/eframe 0.36 (with egui_kittest 0.36 as a walker
dev-dependency). egui 0.36 raised the workspace MSRV to 1.95 (`rust-version` in
`[workspace.package]`); edition stays 2021. Verify with:

```sh
cargo build
cargo test -p walrus-fmt
cargo test -p walker
cargo run -p walker
```

If egui/eframe have moved on, bump the versions in `crates/walker/Cargo.toml`.

## Dependency boundary (keep this true)

`walrus-fmt` must stay GUI-free and free of any `ff-*` / application dependency
so third parties (and FFWB) can embed it. GUI and export-heavy deps live only in
`walker` / behind `walrus-fmt` optional features.
