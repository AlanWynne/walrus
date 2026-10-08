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
5. Export backends: html/md/json implemented as simple stubs; csv real impl;
   pdf/docx need a chosen crate (feature-gated so they stay optional).
6. CSV column viewer in `walker` (`View::Csv`): parse + aligned sortable grid.
7. (DONE -- see "Done" above) The walking-walrus `walrus.wal` demo fixture:
   `generate_walk.rs` produces it and `examples/walrus.wal` is committed.
8. Decide the GUI test approach (egui_kittest mirrors the FFWB convention).

## Build

First build will resolve egui/eframe 0.28 and the zstd/tar crates. Verify with:

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
