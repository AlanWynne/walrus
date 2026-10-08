---
inclusion: always
---

# Testing -- Discipline, Commands, and Organisation

Favour test-driven development: for a bug fix or a new behaviour with a clear
acceptance criterion, write the failing test first, watch it fail, then write
the minimum code to make it pass. The reconstruct engine, version addressing,
and the player state machine are pure logic and MUST be covered this way.

## TDD cycle (for every acceptance criterion)

```
1. WRITE   the test that exercises the criterion
2. RUN     cargo test -- confirm it FAILS (red)
            -> if it passes before implementation, the test is wrong; fix it
3. WRITE   the minimum implementation to pass
4. RUN     cargo test -- confirm it PASSES (green)
5. REFACTOR if needed, keeping all tests green
```

Never skip step 2. A test that was never seen to fail proves nothing.

## What must be covered automatically

- **`walrus-fmt`** (library): container read/write round-trips, reconstruct to
  any index for both `.wal` and `.wbl`, version addressing (index / base / head
  / relative / checkpoint-label / timestamp), splice inversion, and the
  `.wal <-> .wbl` conversion once implemented. These are deterministic and have
  no excuse to be manual.
- **`walx`** (CLI): subcommand behaviour over fixture files.
- **`walker`** player logic (`player.rs`): the loop / stop / wrap / direction /
  speed state machine is pure and unit-tested; keep it that way.

## GUI behaviour testing (walker)

For rendered-widget behaviour in `walker` (transport controls, seek, the CSV
grid), prefer a headless `egui_kittest` harness test over deferring to manual
verification. Reserve MANUAL verification for things that genuinely cannot be
driven headlessly: pixel-exact appearance, OS-native `rfd` file dialogs, real OS
windows, or anything needing a GPU the test runner lacks. When a behaviour is
left manual, state WHY in a comment.

Keep harness tests deterministic: run a fixed, bounded number of frames, never
loop until a timeout, and assert on widget ids / focus / model state, not on
frame counts or pixels.

## Test organisation

- Unit tests: `#[cfg(test)] mod tests { ... }` at the bottom of the source file.
- Integration tests: `tests/` at the crate root, one file per feature area.
- Property tests (if used): `proptest`, with a comment naming the property.

## Test naming

Describe scenario and outcome as a sentence:
```rust
// Good
fn reconstruct_wbl_at_index_zero_returns_current_base() { ... }
// Bad
fn test_reconstruct() { ... }
```

## Test quality

- Deterministic -- no `HashMap` iteration order, system time, or external files
  outside `tests/fixtures/`.
- Each test asserts one primary behaviour.
- `tempfile::TempDir` for any test that writes to disk.
- Never `#[ignore]` a failing test -- fix or delete it.
- An implementation MUST produce byte-identical output for the same version
  index regardless of variant (per FORMAT.md section 5); assert that directly.

## The walrus demo as a test

`examples/walrus.wal` (once the writer lands) doubles as a deterministic
playback fixture: each change-log record nudges the ASCII walrus one column.
Playing it forward then backward MUST return byte-identical frames. Treat it as
an integration test, not just a demo.

---

## Build and test commands

```sh
cargo build                      # debug build, whole workspace
cargo build --release            # release build
cargo check                      # compile check, no binary

cargo test                       # all tests
cargo test -p walrus-fmt         # scoped to one crate (preferred inner loop)
cargo test -p walker
cargo test -- --nocapture        # show stdout
cargo test reconstruct_wbl       # a single test by name

cargo clippy -- -D warnings      # lint, warnings as errors
cargo fmt                        # format
cargo fmt -- --check             # check formatting without changing files

cargo run -p walker              # launch the GUI
cargo run -p walx -- info FILE   # run the CLI
```

Prefer scoped `-p <crate>` runs during active development; they return faster
than a whole-workspace run. Run `cargo fmt` and `cargo clippy -- -D warnings`
before considering a change complete.

Note on `cargo run -p walker`: the GUI is a long-running process. Launch it in a
terminal yourself when verifying interactively; do not block an automated step
on it.
