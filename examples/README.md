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
example of the multi-file mutate API: seed an `Archive`, `add` the base file,
append a forward log of splices with `update_with_splices`, then `write()` it.

## walrus.wal (the mascot demo)

`walrus.wal` is the canonical demo and deterministic playback fixture: an ASCII
walrus (see `walrus-ascii.txt`) that walks across the screen as the change log
is replayed. The base (version 0) is the walrus at the far left (zero shift);
each forward record shifts him one column right, so frame `N` is the walrus `N`
columns across. Playing forward walks him right, backward returns him, and loop
makes him pace. Playback is testable by asserting the leading-space shift at
each version index.

Because `.wal` is `original + forward log = current`, the base is the walrus at
the far left and the head is the walrus at the far right. A matching `.wbl` (via
`walx convert walrus.wal walrus.wbl`, once convert lands) would have the
rightmost frame as its base, so a naive reader of the `.wbl` sees the walrus
already arrived.

### Regenerate it

The committed generator is the canonical producer:

```sh
cargo run -p walrus-fmt --example generate_walk
```

It builds the base walrus at column 0, appends one forward splice per column of
travel, writes `examples/walrus.wal`, and self-verifies that the walrus shifted
the expected number of columns between the base and the head.

### Playing it

```sh
walker                                 # open walrus.wal, press play, toggle loop
walx info examples/walrus.wal
walx extract examples/walrus.wal 0     # leftmost frame
walx extract examples/walrus.wal 24    # rightmost frame (head)
```
