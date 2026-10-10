# Contributing to Walrus

Thanks for your interest in Walrus. This is an early-stage project; the format
in `FORMAT.md` is a 0.1 draft and may change before 1.0.

## Getting started

```sh
git clone https://github.com/AlanWynne/walrus
cd walrus
cargo build
cargo test
```

The workspace has three crates:

- `walrus-fmt` (`crates/walrus`) -- the format + reconstruct engine (library).
- `walx` (`crates/walx`) -- the command-line tool.
- `walker` (`crates/walker`) -- the egui viewer / player (GUI).

## Before you open a pull request

Run the same checks CI runs, locally:

```sh
cargo fmt --all -- --check          # formatting
cargo clippy --workspace --all-features --all-targets -- -D warnings   # lints
cargo test --workspace --all-features                                  # tests
```

CI runs these on Linux, macOS, and Windows, plus a minimum-supported-Rust
(1.75) check. A green local run of the above is the best predictor of green CI.

## Conventions

- **The format spec is the source of truth.** `FORMAT.md` defines the on-disk
  format independently of the Rust implementation. If a change needs the format
  to change, update `FORMAT.md` in the same pull request and describe the change.
- **Keep `walrus-fmt` embeddable.** The library crate must stay GUI-free and
  free of application dependencies. GUI and export-heavy dependencies belong in
  `walker` or behind `walrus-fmt` optional cargo features.
- **Tests first.** For a bug fix or a new behaviour, add a failing test, then
  the code that makes it pass. The reconstruct engine, version addressing, and
  the player state machine are pure logic and should be unit-tested.
- **Plain ASCII** in source and docs (see the character rules the project
  follows: ASCII `--`, `->`, `<->`, `<=`, straight quotes).
- **Update `CHANGELOG.md`** under `## [Unreleased]` for any user-visible change,
  and keep `ROADMAP.md` honest when you complete a stubbed item.

## Commit messages

Short imperative subject line (e.g. "Add gzip codec sniffing to the reader"),
with a body explaining the why when it is not obvious.

## License

By contributing, you agree that your contributions will be dual-licensed under
the MIT and Apache-2.0 licenses, matching the project
(`LICENSE-MIT` / `LICENSE-APACHE`).
