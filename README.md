# Walrus

Walrus is a small, embeddable toolkit for a pair of self-describing document
formats that carry their own edit history:

- **`.wal`** -- `original (base) + forward log = current`
- **`.wbl`** -- `current (base) - reverse log = original`

Each is a compressed tar bundle holding MANY tracked files -- each file with its
own base plus its own ordered change log (a self-describing `.wchg`) -- so the
whole history of every file travels inside one artefact. You can reconstruct any
version of any file, step forward and backward through the versions, and "play"
them back like an animation. Index 0 is always the CURRENT version; higher
indices are older.

The project is three pieces:

| Crate         | Kind    | What it is                                                       |
|---------------|---------|------------------------------------------------------------------|
| `walrus-fmt`  | library | The format + reconstruct engine + the diff/mutate surface (add/update/remove/condense/convert). No GUI, no deps on apps. Embed this. |
| `walrus`      | CLI     | Full management: query, extract, cat, add, update, remove, condense, convert `.wal <-> .wbl`. POSIX/GNU conventions. |
| `walx`        | CLI     | Extract-only companion: `extract` (restore to disk) and `cat` (bytes to stdout). Mutates nothing. |
| `walker`      | GUI     | egui frontend: two-pane file navigator, play forward/backward with loop and speed, content viewers, a `.wchg` history viewer, and export. |

Note: content EXPORT (transforming to html/docx/pdf/md/...) lives only in
`walker`; the CLI emits files in their stored format and never transforms them.
(`.wal <-> .wbl` convert changes the archive variant, not the content, and stays
in the CLI.)

The format itself is specified language-agnostically in [`FORMAT.md`](FORMAT.md)
so it can be implemented anywhere; `walrus-fmt` is the reference implementation.
Design decisions (CLI shape, multi-file layout, generations, diff, i18n, the
GUI frontend) are recorded in [`CLI-DESIGN.md`](CLI-DESIGN.md), and the current
build state is in [`ROADMAP.md`](ROADMAP.md).

> Status: a format-breaking MULTI-FILE revision is in progress. `FORMAT.md` and
> `CLI-DESIGN.md` describe the target; the implementation is mid-migration from
> the earlier single-file scaffold. See `ROADMAP.md` for what is done vs pending.

## The walrus demo

The mascot is also the canonical test fixture. `examples/walrus.wal` tracks a
text file containing an ASCII walrus, with each change-log record nudging him one
column across the screen. Play it forward in `walker` and the walrus walks (or
swims) to the right; play it backward and he returns; enable loop and he paces.
It exercises the forward/reverse log, the player, loop, and speed control, and
doubles as a deterministic playback test (forward-then-backward must reconstruct
byte-identical frames).

(The committed fixtures are from the single-file scaffold and will be regenerated
for the multi-file format during the re-work -- see `ROADMAP.md`.)

## Why two variants

- `.wal` makes saving cheap: new edits append to the forward log, the original
  base never moves.
- `.wbl` makes sharing safe: the base IS the current correct text, so even a
  naive reader that extracts only the base sees the right content.

They are inter-convertible; `walrus convert` performs the conversion.

## Using the library

```toml
[dependencies]
walrus-fmt = { git = "https://github.com/AlanWynne/walrus" }
```

```rust
use walrus_fmt::{WalrusFile, Version};

let archive = WalrusFile::open("report.wbl")?;  // decompress + read manifest
for file in archive.files() {                   // many tracked files per archive
    println!("{}: {} versions", file.name(), file.index_size());
}
// Reconstruct a specific version of a specific tracked file.
// Index 0 is the CURRENT version; higher indices are older.
let current = archive.reconstruct("report.md", Version::Index(0))?;
let older   = archive.reconstruct("report.md", Version::Index(5))?;
```

(API names are illustrative; the multi-file surface is being implemented -- see
`ROADMAP.md`.)

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

## Authors and acknowledgements

- **Alan Wynne** -- creator, format and CLI design, project direction.

Built with AI-assisted development (Kiro).

Full contribution history is in the git log and the GitHub contributors graph.

## License

Dual-licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

## Status

Pre-1.0, mid-migration. The format in `FORMAT.md` is a 0.1 draft; a
format-breaking MULTI-FILE revision is the current target (recorded in
`FORMAT.md` and `CLI-DESIGN.md`), and the implementation is migrating to it from
the earlier single-file scaffold. See `ROADMAP.md` for the honest done-vs-pending
state and the ordered re-work sequence.
