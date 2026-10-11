"""Acceptance tests for `walrus alias` (CLI-DESIGN section 4).

These tests NEVER touch the real PATH: they create an isolated temp `--dir` and
inject it as the search path via the WALRUS_ALIAS_PATH environment variable, so
install/status/remove operate only on the scratch directory.

install creates a `wal` shim; status reports it as ours; remove deletes only
ours. A FOREIGN `wal` sitting in the dir makes install refuse (RC 2) creating
nothing, even with `--force`; re-installing over our own prior shim is
idempotent. The shim filename is OS-specific (`wal.cmd` on Windows, `wal` on
POSIX) and is resolved through a glob rather than hard-coded.
"""

from __future__ import annotations

import os
import unittest
from pathlib import Path

from harness import WalrusCase, covers


class AliasTests(WalrusCase):

    def _bindir(self) -> Path:
        bindir = self.work / "bin"
        bindir.mkdir(parents=True, exist_ok=True)
        return bindir

    def _env(self, bindir: Path) -> dict[str, str]:
        # Inject the isolated dir as the only alias search path.
        return {"WALRUS_ALIAS_PATH": str(bindir)}

    def _shim(self, bindir: Path) -> Path:
        name = "wal.cmd" if os.name == "nt" else "wal"
        return bindir / name

    @covers("CLI-DESIGN-4-alias-install")
    def test_install_creates_our_shim(self):
        bindir = self._bindir()
        r = self.run_walrus("alias", "install", "--dir", bindir, env=self._env(bindir))
        self.assert_code(r, 0)
        shims = list(bindir.glob("wal*"))
        self.assertTrue(shims, "alias install should create a wal shim")

    @covers("CLI-DESIGN-4-alias-status")
    def test_status_reports_ours(self):
        bindir = self._bindir()
        self.assert_code(
            self.run_walrus("alias", "install", "--dir", bindir, env=self._env(bindir)), 0
        )
        r = self.run_walrus("alias", "status", "--dir", bindir, env=self._env(bindir))
        self.assert_code(r, 0)
        self.assert_stdout_contains(r, "ours")

    @covers("CLI-DESIGN-4-alias-remove")
    def test_remove_deletes_only_ours(self):
        bindir = self._bindir()
        self.assert_code(
            self.run_walrus("alias", "install", "--dir", bindir, env=self._env(bindir)), 0
        )
        r = self.run_walrus("alias", "remove", "--dir", bindir, env=self._env(bindir))
        self.assert_code(r, 0)
        self.assertFalse(list(bindir.glob("wal*")), "our shim should be gone")

    @covers("CLI-DESIGN-4-alias-foreign", "CLI-DESIGN-6-rc2")
    def test_foreign_wal_makes_install_refuse_creating_nothing(self):
        bindir = self._bindir()
        foreign = self._shim(bindir)
        foreign.write_bytes(b"not ours")
        r = self.run_walrus("alias", "install", "--dir", bindir, env=self._env(bindir))
        self.assert_code(r, 2)
        # The foreign file is untouched and nothing else was created.
        self.assertEqual(foreign.read_bytes(), b"not ours")
        self.assertEqual(sorted(p.name for p in bindir.iterdir()), [foreign.name])

    @covers("CLI-DESIGN-4-alias-foreign")
    def test_force_still_refuses_a_foreign_shim(self):
        bindir = self._bindir()
        foreign = self._shim(bindir)
        foreign.write_bytes(b"not ours")
        r = self.run_walrus("alias", "install", "--dir", bindir, "--force",
                            env=self._env(bindir))
        self.assert_code(r, 2)
        self.assertEqual(foreign.read_bytes(), b"not ours")

    @covers("CLI-DESIGN-4-alias-idempotent")
    def test_reinstall_over_our_shim_is_idempotent(self):
        bindir = self._bindir()
        self.assert_code(
            self.run_walrus("alias", "install", "--dir", bindir, env=self._env(bindir)), 0
        )
        self.assert_code(
            self.run_walrus("alias", "install", "--dir", bindir, env=self._env(bindir)), 0
        )
        self.assertEqual(len(list(bindir.glob("wal*"))), 1,
                         "a re-install must not duplicate the shim")


if __name__ == "__main__":
    unittest.main(verbosity=2)
