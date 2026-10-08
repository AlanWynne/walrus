# Examples

## walrus-story.wal (narrated title-card demo)

`walrus-story.wal` is a ready-to-open `.wal` that plays like film credits: a
stable ASCII-walrus header with a caption line that is rewritten one frame at a
time to tell a short story about what Walrus does. Version 0 is the opening
frame ("a walrus clears his throat..."); the head is the closing bow. Each
forward change record is a single splice that replaces the whole caption region
with the next line, so every version is byte-exact and backward playback rewinds
the narration cleanly.

Try it:

```sh
walx info examples/walrus-story.wal         # 0..=16, 16 change records
walx extract examples/walrus-story.wal 0    # opening frame
walx extract examples/walrus-story.wal 16   # closing frame
cargo run -p walker                         # then open it, press play, toggle loop
```

Regenerate it with the committed generator (the canonical producer):

```sh
cargo run -p walrus-fmt --example generate_story
```

The generator (`crates/walrus/examples/generate_story.rs`) is also a worked
example of the container writer: build a `WalrusFile` from a base plus a forward
log of splices, then `write()` it.

## walrus.wal (the mascot demo)

`walrus.wal` is the canonical demo and deterministic playback fixture: an ASCII
walrus (see `walrus-ascii.txt`) that walks across the screen as the change log
is replayed. Frame `N` places the walrus at column `N`, so playback is testable
by asserting the leading-space count at each version index.

### Status

The fixture is NOT committed as a binary yet, because it requires the Walrus
*container writer*, which is still a stub in `walrus-fmt` (the current crate
implements the reader and the reconstruct engine; see `FORMAT.md` and the
`container.rs` TODO). The generator below is the intended producer; wire it up
when the writer lands.

### Intended generator (pseudocode)

```text
base    = the ASCII walrus with 0 leading spaces (the ORIGINAL, leftmost frame)
for col in 1..=WIDTH:
    record = splice that increases the leading indentation by one column
    append record to the forward log
write .wal = zstd(tar{ manifest.json, base, log.jsonl })
```

Because `.wal` is `original + forward log = current`, the base is the walrus at
the far left and the head is the walrus at the far right. The matching `.wbl`
(produced by `walx convert walrus.wal walrus.wbl`) has the rightmost frame as
its base, so a naive reader of a `.wbl` sees the walrus already arrived.

### Playing it

```sh
walker            # then File > Open > walrus.wal, press play, toggle loop
walx info walrus.wal
walx extract walrus.wal 0   # leftmost frame
walx extract walrus.wal 20  # 20 columns across
```
