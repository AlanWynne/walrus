"""Deep single-file history acceptance test (CLI-DESIGN-5-deep-history).

Seeds ONE tracked file (``one.txt``) into a ``.wal`` archive, then records about
fifty successive versions through the same ``walrus update -R`` mechanism
``test_update.py`` uses. The per-version content is a PURE function of the
version number and deliberately MIXES change sizes: single-character edits, tiny
single-line edits, and periodic multi-kilobyte whole-content rewrites, so the
change log holds both small and large splices.

Every version's exact bytes are recorded in a Python list as the history is
built, independently of the engine. The test then RESTORES and verifies every
version byte-for-byte via ``walrus cat one.txt --index i`` (the primary oracle),
and spot-checks the named / relative / timestamp selectors.

Determinism (per testing.md): no RNG, no clock, no set/dict iteration order --
version ``i`` is a pure function of ``i``.
"""

from __future__ import annotations

import unittest

from harness import WalrusCase, covers


# The CLI index model (confirmed by test_cat.py::test_cat_older_version_by_index):
# ``--index 0`` is the CURRENT (last-written) version, higher indices are OLDER,
# and ``--index <count>`` is the ORIGINAL (seed). The stored name of a file added
# with ``-R`` under ``<work>/src`` is ``src/<relative>`` (see test_cat.py), so the
# seeded file is addressed as ``src/one.txt``.
STORED_NAME = "src/one.txt"

# How many versions to record on top of the seed. ~50 successive updates.
VERSION_COUNT = 50


def version_bytes(i: int) -> bytes:
    """The exact content of version ``i`` as a pure function of ``i``.

    Version 0 is the seed. Each later version mixes change sizes by cycling a
    pattern on ``i``:
    - a periodic multi-kilobyte whole-content rewrite (large splice),
    - a tiny single-line edit (small splice),
    - and otherwise a single-character marker edit (one-byte splice).

    The content is built purely from ``i`` so two runs produce identical bytes.
    """
    if i == 0:
        return b"one\nseed line\n"
    phase = i % 7
    if phase == 0:
        # Large whole-content rewrite: a multi-kilobyte block keyed on i.
        block = (f"block-{i:04d}-" .encode("ascii")) * 400  # ~5 KB, varies by i
        return b"BIG\n" + block + f"\nend-{i:04d}\n".encode("ascii")
    if phase == 3:
        # Tiny single-line edit: a short line whose text depends on i.
        return f"one\nline edit {i}\n".encode("ascii")
    # Single-character / short marker edit: a minimal change keyed on i.
    return f"v{i}\n".encode("ascii")


class DeepHistoryTests(WalrusCase):

    @covers("CLI-DESIGN-5-deep-history", "FR-W10-AC4", "FR-W10-AC3")
    def test_deep_single_file_history_restores_byte_exact_at_every_index(self):
        # history[0] = seed, history[-1] = last written; built independently of
        # the engine so it is a trustworthy oracle.
        history: list[bytes] = [version_bytes(0)]

        src = self.work / "src"
        one = src / "one.txt"
        self.write_bytes(one, history[0])
        # Seed the archive with the single-file tree (mirrors seed_archive's add
        # shape but with just one.txt under the recursed root).
        self.assert_code(self.run_walrus("add", self.work / "a.wal", src, "-R", "--wal"), 0)
        arc = self.work / "a.wal"

        # Record ~50 successive versions through update -R, exactly as the suite
        # records a new version of an existing tracked file (test_update.py).
        for i in range(1, VERSION_COUNT + 1):
            content = version_bytes(i)
            self.write_bytes(one, content)
            self.assert_code(self.run_walrus("update", arc, src, "-R"), 0)
            history.append(content)

        last = len(history) - 1  # == VERSION_COUNT; the ORIGINAL end's index
        self.assertEqual(self.query_versions(arc, STORED_NAME), last)

        # Primary oracle: for EVERY index i, cat --index i must equal the version
        # that is i steps back from current. Index 0 == current == history[last];
        # index `last` == original == history[0]. So index i maps to
        # history[last - i].
        for index in range(0, last + 1):
            expected = history[last - index]
            r = self.run_walrus("cat", arc, STORED_NAME, "--index", str(index))
            self.assert_code(r, 0)
            self.assert_stdout_bytes(r, expected)

        # Spot-check the named endpoints.
        r = self.run_walrus("cat", arc, STORED_NAME, "--version", "original")
        self.assert_code(r, 0)
        self.assert_stdout_bytes(r, history[0])

        r = self.run_walrus("cat", arc, STORED_NAME, "--version", "current")
        self.assert_code(r, 0)
        self.assert_stdout_bytes(r, history[last])

        # A couple of relative selectors: +k moves OLDER from current (index 0),
        # so --version +k == history[last - k].
        for k in (1, 5):
            r = self.run_walrus("cat", arc, STORED_NAME, "--version", f"+{k}")
            self.assert_code(r, 0)
            self.assert_stdout_bytes(r, history[last - k])

    @covers("CLI-DESIGN-5-deep-history", "CLI-DESIGN-6a-utc-stamp")
    def test_deep_history_at_sentinel_resolves_to_current(self):
        # The suite has no way to set or read a specific per-version timestamp,
        # so exact mid-history --at addressing is NOT determinable. Exercise --at
        # only with the documented far-past sentinel, which (newest-at-or-before)
        # resolves to the CURRENT version, as test_cat.py does.
        src = self.work / "src"
        one = src / "one.txt"
        self.write_bytes(one, version_bytes(0))
        arc = self.work / "a.wal"
        self.assert_code(self.run_walrus("add", arc, src, "-R", "--wal"), 0)

        last_content = version_bytes(1)
        self.write_bytes(one, last_content)
        self.assert_code(self.run_walrus("update", arc, src, "-R"), 0)

        r = self.run_walrus("cat", arc, STORED_NAME, "--at", "2000-01-01T00:00:00Z")
        self.assert_code(r, 0)
        self.assert_stdout_bytes(r, last_content)

    def test_exact_mid_history_at_addressing_is_skipped(self):
        # Exact per-index --at addressing cannot be driven by this black-box
        # suite because it cannot pin a known per-version timestamp.
        self.skipTest("suite cannot pin per-version timestamps; see FR-W11-AC8")


if __name__ == "__main__":
    unittest.main(verbosity=2)
