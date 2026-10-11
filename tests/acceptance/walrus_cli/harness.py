"""Cross-platform acceptance-test harness for the `walrus` CLI.

This drives the REAL built `walrus` binary as a black box and verifies its
behaviour through exit code, stdout bytes, stderr text, and on-disk results.
It is pure standard-library Python 3 (no third-party packages) and runs
identically on Windows and Linux.

The test cases live in ``test_walrus.py`` and subclass :class:`WalrusCase`.

Running the binary:
  - stdin is sent as EXACT bytes (``subprocess`` ``input=``), so there is no
    shell newline injection -- essential for testing ``walrus add -`` and the
    byte-fidelity of ``cat``.
  - arguments are passed as an argv list, so no shell quoting/interpretation
    ever mangles a flag such as ``-o``.

Binary discovery:
  Looks for ``target/debug/walrus(.exe)`` (then ``release``) relative to the
  repo root (three levels up from this file), or honours the ``WALRUS_BIN``
  environment variable. If the binary is missing the tests fail fast with a
  clear message telling you to build it.
"""

from __future__ import annotations

import hashlib
import os
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path


# --- Binary discovery -------------------------------------------------------

def _repo_root() -> Path:
    # tests/acceptance/walrus_cli/harness.py -> repo root is three parents up.
    return Path(__file__).resolve().parents[3]


def find_walrus() -> Path:
    """Resolve the walrus binary, or raise with a build hint."""
    env = os.environ.get("WALRUS_BIN")
    if env:
        p = Path(env)
        if p.is_file():
            return p
        raise FileNotFoundError(f"WALRUS_BIN set but not a file: {env}")

    exe = "walrus.exe" if os.name == "nt" else "walrus"
    root = _repo_root()
    for profile in ("debug", "release"):
        cand = root / "target" / profile / exe
        if cand.is_file():
            return cand
    raise FileNotFoundError(
        f"walrus binary not found under {root / 'target'}. "
        "Build it first:  cargo build -p walrus-cli"
    )


# --- Result object ----------------------------------------------------------

class RunResult:
    """The outcome of one CLI invocation."""

    def __init__(self, code: int, out: bytes, err: bytes, argv: list[str]):
        self.code = code
        self.out = out                      # raw stdout bytes
        self.err = err                       # raw stderr bytes
        self.out_text = out.decode("utf-8", "replace")
        self.err_text = err.decode("utf-8", "replace")
        self.argv = argv

    def __repr__(self) -> str:  # shown in assertion failure messages
        return (
            f"RunResult(code={self.code}, argv={self.argv!r}, "
            f"stdout={self.out_text!r}, stderr={self.err_text!r})"
        )


# --- Base test case ---------------------------------------------------------

class WalrusCase(unittest.TestCase):
    """Base class: binary discovery, per-test tempdir, run + assert helpers."""

    walrus: Path

    @classmethod
    def setUpClass(cls) -> None:
        cls.walrus = find_walrus()

    def setUp(self) -> None:
        # A fresh, isolated scratch dir per test, auto-removed on teardown.
        self._tmp = tempfile.TemporaryDirectory(prefix="walrus_acc_")
        self.work = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    # -- running ------------------------------------------------------------

    def run_walrus(self, *args: str, stdin: bytes | None = None,
                   cwd: str | os.PathLike | None = None) -> RunResult:
        """Run the CLI with the given args; capture code/stdout/stderr.

        Never raises on a non-zero exit -- the exit code is data the test
        asserts on. ``stdin`` is sent as exact bytes when provided.
        """
        argv = [str(self.walrus), *[str(a) for a in args]]
        proc = subprocess.run(
            argv,
            input=stdin,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            cwd=str(cwd) if cwd else None,
        )
        return RunResult(proc.returncode, proc.stdout, proc.stderr, argv)

    # -- fixtures -----------------------------------------------------------

    def write(self, path: str | os.PathLike, content: str) -> Path:
        """Write a UTF-8 text fixture with LF endings and no BOM."""
        p = Path(path)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(content.replace("\r\n", "\n").encode("utf-8"))
        return p

    def write_bytes(self, path: str | os.PathLike, data: bytes) -> Path:
        p = Path(path)
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
        return p

    def make_fixture_tree(self, root: str | os.PathLike) -> Path:
        """A standard small text tree with a nested path; returns its path."""
        root = Path(root)
        self.write(root / "readme.txt", "hello walrus\nline two")
        self.write(root / "notes" / "todo.md", "# TODO\n- first item")
        self.write(root / "data" / "nums.csv", "a,b,c\n1,2,3")
        return root

    # -- tree hashing -------------------------------------------------------

    @staticmethod
    def tree_hashes(directory: str | os.PathLike) -> dict[str, str]:
        """Map every file under *directory* to its SHA-256, keyed by the
        forward-slash relative path, so two trees compare independent of
        their location or the OS path separator."""
        base = Path(directory)
        result: dict[str, str] = {}
        if not base.is_dir():
            return result
        for f in sorted(base.rglob("*")):
            if f.is_file():
                rel = f.relative_to(base).as_posix()
                result[rel] = hashlib.sha256(f.read_bytes()).hexdigest()
        return result

    # -- assertions ---------------------------------------------------------

    def assert_code(self, result: RunResult, expected: int) -> None:
        self.assertEqual(
            result.code, expected,
            f"expected exit code {expected}, got {result.code}\n{result!r}",
        )

    def assert_stdout_bytes(self, result: RunResult, expected: bytes) -> None:
        self.assertEqual(
            result.out, expected,
            f"stdout bytes mismatch: expected {expected!r}, got {result.out!r}",
        )

    def assert_stdout_contains(self, result: RunResult, needle: str) -> None:
        self.assertIn(needle, result.out_text, f"stdout missing {needle!r}\n{result!r}")

    def assert_output_contains(self, result: RunResult, needle: str) -> None:
        """Needle appears on stdout OR stderr (useful where a CLI's stream
        choice for help/warnings is not something a test should over-pin)."""
        self.assertTrue(
            needle in result.out_text or needle in result.err_text,
            f"neither stdout nor stderr contains {needle!r}\n{result!r}",
        )

    def assert_tree_equals(self, directory, expected: dict[str, str]) -> None:
        actual = self.tree_hashes(directory)
        self.assertEqual(
            actual, expected,
            f"tree under {directory} does not match expected content/paths",
        )

    def assert_file(self, path: str | os.PathLike) -> None:
        self.assertTrue(Path(path).is_file(), f"expected file to exist: {path}")

    def assert_missing(self, path: str | os.PathLike) -> None:
        self.assertFalse(Path(path).exists(), f"expected path to NOT exist: {path}")
