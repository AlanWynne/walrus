# walrus-fmt

The reference implementation of the `.wal` / `.wbl` document-with-history format.

- `.wal` -- `original (base) + forward log = current`
- `.wbl` -- `current (base) - reverse log = original`

Each is a zstd-compressed tar bundle holding MANY tracked files -- each file with
its own base and its own self-describing `.wchg` change log. See
[`FORMAT.md`](../../FORMAT.md) for the language-agnostic specification and
[`CLI-DESIGN.md`](../../CLI-DESIGN.md) for the design decisions. Index 0 is the
CURRENT version; higher indices are older.

This crate is dependency-light and GUI-free so it can be embedded in any
application. It owns the format, the reconstruct engine, and the diff/mutate
surface (add/update/remove/condense/convert). Export formats are opt-in cargo
features; content transformation otherwise lives in the `walker` GUI.

```rust
use walrus_fmt::{WalrusFile, Version};

let archive = WalrusFile::open("report.wbl")?;
let current = archive.reconstruct("report.md", Version::Index(0))?; // index 0 = current
```

(API names are illustrative; the multi-file surface is being implemented -- see
`../../ROADMAP.md`. A format-breaking multi-file revision is in progress.)

Published as `walrus-fmt` because the crates.io name `walrus` is taken by an
unrelated crate.
