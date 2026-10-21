#!/usr/bin/env python3
"""The synthetic workflow observer releases each actual readonly connection."""
import importlib.util
from pathlib import Path
import sqlite3
import tempfile
import unittest

SPEC = importlib.util.spec_from_file_location(
    "workflow_supported", Path(__file__).with_name("e2e") / "workflow_supported.py")
RUNNER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RUNNER)


class ReaderLifetimeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        (self.root / ".ledger").mkdir()
        (self.root / ".ledger/local.db").touch()
        self.journey = RUNNER.Journey.__new__(RUNNER.Journey)
        self.journey.installation = self.root

    def tearDown(self):
        self.temporary.cleanup()

    def test_success_closes_reader_before_next_observation(self):
        with self.journey.connection() as connection:
            self.assertEqual(connection.execute("SELECT 1").fetchone(), (1,))
        with self.assertRaises(sqlite3.ProgrammingError):
            connection.execute("SELECT 1")
        with self.journey.connection() as next_connection:
            self.assertIsNot(next_connection, connection)
            self.assertEqual(next_connection.execute("SELECT 1").fetchone(), (1,))
        with self.assertRaises(sqlite3.ProgrammingError):
            next_connection.execute("SELECT 1")

    def test_failed_observation_closes_reader(self):
        with self.assertRaisesRegex(RuntimeError, "synthetic observation failure"):
            with self.journey.connection() as connection:
                self.assertEqual(connection.execute("SELECT 1").fetchone(), (1,))
                raise RuntimeError("synthetic observation failure")
        with self.assertRaises(sqlite3.ProgrammingError):
            connection.execute("SELECT 1")


if __name__ == "__main__":
    unittest.main()
