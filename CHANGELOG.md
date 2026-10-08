# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Initial workspace scaffold with three crates: `walrus-fmt` (library), `walx`
  (CLI), and `walker` (egui GUI).
- `FORMAT.md` 0.1 draft: the language-agnostic `.wal` / `.wbl` container spec.
- Container reader (zstd decompress, tar member extraction, manifest/log parse).
- Reconstruct engine: replay forward splices to any version index.
- Version addressing: index / base / head / relative / checkpoint-label.
- `walx` CLI with `info` and `extract` subcommands.
- `walker` egui viewer with transport (step/play/pause/direction/loop/seek) and
  a unit-tested playback state machine.

### Stubbed / TODO
See `ROADMAP.md` for the honest done-vs-stubbed state. Notably: the container
writer, gzip codec support, `.wal <-> .wbl` conversion, timestamp addressing,
the export backends, the CSV viewer, and the `walrus.wal` demo fixture.

[Unreleased]: https://github.com/OWNER/walrus/commits/main
