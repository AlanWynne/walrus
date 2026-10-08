# Walrus

Walrus is a small, embeddable toolkit for a pair of self-describing document
formats that carry their own edit history:

- **`.wal`** -- `original (base) + forward log = current`
- **`.wbl`** -- `current (base) - reverse log = original`

Both are a compressed tar bundle of a base document plus an ordered change log,
so the whole history of a file travels inside one artefact. You can reconstruct
any version, step forward and backward through the versions, and "play" them
back like an animation.

The project is three pieces:

| Crate         | Kind    | What it is                                                       |
|---------------|---------|------------------------------------------------------------------|
| `walrus-fmt`  | library | The format + reconstruct-to-version engine. No GUI, no deps on apps. Embed this. |
| `walx`        | CLI     | Decompress, inspect, extract a version, convert `.wal <-> .wbl`.  |
| `walker`      | GUI     | egui viewer/player: browse, auto-play forward/backward with loop and speed, export, CSV column viewer. |

The format itself is specified language-agnostically in [`FORMAT.md`](FORMAT.md)
so it can be implemented anywhere; `walrus-fmt` is the reference implementation.

## The walrus demo

The mascot is also the canonical test fixture. `examples/walrus.wal` is a text
file containing an ASCII walrus, with each change-log record nudging him one
column across the screen. Play it forward in `walker` and the walrus walks (or
swims) to the right; play it backward and he returns; enable loop and he paces.
It exercises the forward/reverse log, the player, loop, and speed control in one
file, and doubles as a deterministic playback test.

## Why two variants

- `.wal` makes saving cheap: new edits append to the forward log, the original
  base never moves.
- `.wbl` makes sharing safe: the base IS the current correct text, so even a
  naive reader that extracts only the base sees the right content.

They are inter-convertible; `walx` performs the conversion.

## Using the library

```toml
[dependencies]
walrus-fmt = { git = "https://github.com/OWNER/walrus" }
```

```rust
use walrus_fmt::{WalrusFile, Version};

let doc = WalrusFile::open("report.wbl")?;      // decompress + read manifest
let current = doc.reconstruct(Version::Head)?;  // bytes of a chosen version
let original = doc.reconstruct(Version::Index(0))?;
```

Export formats (PDF, DOCX, HTML, Markdown, CSV, JSON, plain text) are behind
optional cargo features so a playback-only embedder stays lean:

```toml
walrus-fmt = { git = "...", features = ["export-html", "export-md"] }
```

## Relationship to FileForgeWorkbench (FFWB)

Walrus is a standalone project. FFWB is simply one consumer: when FFWB adds
`.wal`/`.wbl` support (tracked as CR-NR-107 in that project), it will depend on
the `walrus-fmt` crate rather than reimplement the format. One source of truth.

## Naming note (crates.io)

The crates.io name `walrus` is already taken by an unrelated WebAssembly crate,
so the library package is published as **`walrus-fmt`**. The repository, the GUI
binary (`walker`), and the mascot keep the name "Walrus".

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Status

Early scaffold. The format in `FORMAT.md` is a 0.1 draft and may change before
1.0. See each crate's `TODO` notes for what is stubbed vs implemented.
