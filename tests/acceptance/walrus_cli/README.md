# walrus CLI acceptance tests

Black-box, cross-platform acceptance tests for the built `walrus` binary. They
drive the real executable as a subprocess and verify behaviour through exit
code, stdout bytes, stderr text, and on-disk results. Pure Python 3 standard
library -- no third-party packages -- so they run the same on Windows and Linux.

These complement the Rust tests: the Rust `tests/` exercise library and
per-crate internals; these exercise the SHIPPED command-line contract
end to end (argument parsing, exit codes, byte-exact `cat`, stdin ingestion,
round-trip fidelity, the full return-code table).

## Running

Build the binary first, then run the suite from the repository root:

```sh
cargo build -p walrus-cli
python -m unittest discover -s tests/acceptance/walrus_cli -v
```

By default the harness finds `target/debug/walrus` (or `walrus.exe` on
Windows), falling back to `target/release`. To test a specific binary set
`WALRUS_BIN`:

```sh
WALRUS_BIN=/path/to/walrus python -m unittest discover -s tests/acceptance/walrus_cli -v
```

Each test runs in its own temporary directory (via `tempfile`), so cases are
isolated and nothing is written into the repository tree.

## Layout

- `harness.py` -- the engine: binary discovery, `run_walrus(...)` (captures
  exit code + stdout/stderr bytes, sends exact stdin bytes), tree hashing, and
  the assertion helpers. Base class `WalrusCase`.
- `test_walrus.py` -- the case suite, grouped by subcommand
  (add / update / extract / cat / query / remove / condense / convert / stdin /
  usage / alias), including the `.wal` and `.wbl` round-trip matrix and the
  error-path return codes.

## Return-code contract (CLI-DESIGN section 6)

| Code | Meaning |
|------|---------|
| 0 | success (including a no-change no-op) |
| 2 | usage / safety error |
| 4 | extract numeric index beyond the last recorded version (original written + warning) |
| 8 | target file / mask matches nothing (update / remove / condense / convert) |
