# Walrus CLI acceptance suite

Black-box, cross-platform acceptance tests that drive the real built `walrus`
binary and verify its behaviour through exit code, stdout bytes, stderr text,
and on-disk results. Pure Python 3 standard library (no `pip`, no third-party
packages); runs identically on Windows and Linux.

## Layout

One test file per subcommand / concern area, each discovered by
`unittest discover`:

| File | Area |
| --- | --- |
| `harness.py` | `WalrusCase` base: binary discovery, per-test tempdir, run + assert + fixture helpers, the `@covers` decorator, codec magic constants |
| `requirements.py` | the authoritative requirement-id REGISTRY (the coverage contract) |
| `test_query.py` | `query` (and the bare-archive default) |
| `test_extract.py` | `extract` core (selectors, output, overwrite, mask) |
| `test_extract_safety.py` | extract naming safety (CLI-DESIGN 5.4d) |
| `test_cat.py` | `cat` byte fidelity and match rules |
| `test_add.py` | `add` (create, basename, recurse, direction, generations) |
| `test_update.py` | `update` (version, RC 8, transactional, no-change, stamp) |
| `test_stdin.py` | `-` + `--name` stdin ingestion (CLI-DESIGN 5.4e, AC9) |
| `test_remove.py` | `remove` (single / multi / no-match) |
| `test_condense.py` | `condense` (keep 0 / keep n / no-match) |
| `test_convert.py` | `convert` (round-trip, masked, no-rename, no-match) |
| `test_alias.py` | `alias` install / status / remove (isolated `--dir`) |
| `test_usage_global.py` | help, usage, two-operations, the RC 0/2/4/8 self-check |
| `test_codec_format.py` | container codec magic and clean-error-on-bad-magic |
| `run_tcr.py` | the Test Completion Report generator |

## Running the suite

Build the binary first, then point the suite at it with `WALRUS_BIN` (or let the
harness find `target/debug/walrus(.exe)` on its own):

```sh
cargo build -p walrus-cli
# optional: pin the binary explicitly
export WALRUS_BIN=.../target/debug/walrus      # Windows: $env:WALRUS_BIN=...
python -m unittest discover -s tests/acceptance/walrus_cli -v
```

## Traceability and the `@covers` decorator

Every test method is stamped with the requirement ids it exercises:

```python
@covers("FR-W11-AC5", "CLI-DESIGN-5.3-direction")
def test_direction_flag_ignored_for_already_tracked_file(self):
    ...
```

`requirements.py` holds the REGISTRY of every requirement id that must be
covered (drawn from the FR-W10 / FR-W11 acceptance criteria and the CLI-DESIGN
command sections). A registry id with no covering `@covers` test is a COVERAGE
GAP that fails the TCR sign-off.

## OS gating

OS-specific cases use `@unittest.skipIf(os.name != "nt", "<reason>")` (or the
inverse) with a human reason string, so both platforms stay green: a case that
cannot run on the current OS is skipped with its reason, never failed.

## Determinism

Tests use only per-test temp directories, fixed timestamp strings (never the
wall clock in assertions), non-UTF-8 fixtures built from numeric byte escapes,
and `sorted()` wherever order matters.

## Test Completion Report (TCR)

`run_tcr.py` discovers and runs the whole suite, collects each test's outcome
and `@covers` tags, and emits a Test Completion Report:

```sh
python tests/acceptance/walrus_cli/run_tcr.py
```

It writes both a human report and a machine form to the git-ignored
`.gate/tcr/` directory:

- `.gate/tcr/walrus-cli-TCR.md`
- `.gate/tcr/walrus-cli-TCR.json`

The report carries run metadata (UTC timestamp, platform, Python version, the
resolved binary path and its version -- the CLI has no `--version`, so the
identity falls back to the git short-sha with a recorded finding), the totals,
the full REQUIREMENT TRACEABILITY MATRIX (id -> covering test(s) -> PASS / FAIL
/ UNCOVERED), the uncovered list, the failures list, the findings, and a
SIGN-OFF VERDICT:

```
PASS iff zero failures AND zero errors AND zero uncovered requirement ids
FAIL otherwise, with the reasons listed
```

`run_tcr.py` exits 0 on PASS and non-zero on FAIL, so it can gate a pipeline.

The generator (`run_tcr.py`) is COMMITTED; its generated output under
`.gate/tcr/` is a run artifact and is NOT committed (`.gate/` is git-ignored).
