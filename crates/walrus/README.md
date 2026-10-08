# walrus-fmt

The reference implementation of the `.wal` / `.wbl` document-with-history format.

- `.wal` -- `original (base) + forward log = current`
- `.wbl` -- `current (base) - reverse log = original`

Both are a zstd-compressed tar bundle of a base document plus an ordered change
log. See [`FORMAT.md`](../../FORMAT.md) for the language-agnostic specification.

This crate is dependency-light and GUI-free so it can be embedded in any
application. Export formats are opt-in cargo features.

```rust
use walrus_fmt::{WalrusFile, Version};

let doc = WalrusFile::open("report.wbl")?;
let bytes = doc.reconstruct(Version::Head)?;
```

Published as `walrus-fmt` because the crates.io name `walrus` is taken by an
unrelated crate.
