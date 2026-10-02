#!/usr/bin/env python3
"""Focused tests for SQLite WAL comparison during old-writer qualification."""

import importlib.util
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("check-native-package-journey.py")
SPEC = importlib.util.spec_from_file_location("native_package_journey", SCRIPT)
if SPEC is None or SPEC.loader is None:
    raise RuntimeError(f"cannot load qualification script at {SCRIPT}")
JOURNEY = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(JOURNEY)


class DurableFileComparisonTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.wal = self.root / JOURNEY.SQLITE_WAL_PATH
        self.wal.parent.mkdir(parents=True)

    def tearDown(self):
        self.temp.cleanup()

    def snapshot(self):
        return JOURNEY.file_hashes(self.root), JOURNEY.sqlite_sidecar_snapshot(self.wal)

    def compare(self, before, after, phase="synthetic test"):
        return JOURNEY.require_same_durable_files(
            before[0], after[0], before[1], after[1], phase,
            exclude_empty_sqlite_wal=True,
        )

    def test_absent_and_zero_byte_wal_lifecycle_is_allowed(self):
        absent = self.snapshot()
        self.wal.touch()
        empty = self.snapshot()
        self.assertTrue(self.compare(absent, empty))
        self.assertTrue(self.compare(empty, absent))

    def test_nonempty_wal_change_is_rejected(self):
        self.wal.write_bytes(b"wal-header-and-frame-a")
        before = self.snapshot()
        self.wal.write_bytes(b"wal-header-and-frame-b")
        after = self.snapshot()
        with self.assertRaisesRegex(RuntimeError, "SQLite WAL before=.*after="):
            self.compare(before, after)

    def test_nonempty_wal_appearance_or_disappearance_is_rejected(self):
        absent = self.snapshot()
        self.wal.write_bytes(b"nonempty journal")
        nonempty = self.snapshot()
        with self.assertRaises(RuntimeError):
            self.compare(absent, nonempty)
        self.wal.write_bytes(b"")
        empty = self.snapshot()
        with self.assertRaises(RuntimeError):
            self.compare(nonempty, empty)
        with self.assertRaises(RuntimeError):
            self.compare(empty, nonempty)
        self.wal.unlink()
        with self.assertRaises(RuntimeError):
            self.compare(nonempty, self.snapshot())

    def test_unchanged_nonempty_wal_is_strictly_equal(self):
        self.wal.write_bytes(b"same journal")
        before = self.snapshot()
        self.assertFalse(self.compare(before, self.snapshot()))

    def test_other_paths_and_ordinary_empty_files_remain_strict(self):
        unrelated = self.root / "notes.txt"
        unrelated.touch()
        before = self.snapshot()
        unrelated.unlink()
        with self.assertRaises(RuntimeError):
            self.compare(before, self.snapshot())
        unrelated.touch()
        before = self.snapshot()
        unrelated.write_bytes(b"changed")
        with self.assertRaises(RuntimeError):
            self.compare(before, self.snapshot())

    def test_shared_memory_exclusion_is_opt_in_and_exact_path_only(self):
        shm = self.root / ".ledger" / "local.db-shm"
        shm.touch()
        default_hashes = JOURNEY.file_hashes(self.root)
        self.assertIn(".ledger/local.db-shm", default_hashes)
        self.assertNotIn(".ledger/local.db-shm", JOURNEY.file_hashes(self.root, exclude_sqlite_shm=True))
        ordinary = self.root / ".ledger" / "local.db-other"
        ordinary.touch()
        self.assertIn(".ledger/local.db-other", JOURNEY.file_hashes(self.root, exclude_sqlite_shm=True))


if __name__ == "__main__":
    unittest.main()
