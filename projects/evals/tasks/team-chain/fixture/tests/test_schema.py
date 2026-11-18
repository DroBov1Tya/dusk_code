import sqlite3
import unittest

from inventory.schema import migrate


class SchemaTest(unittest.TestCase):
    def setUp(self):
        self.conn = sqlite3.connect(":memory:")

    def test_tables_are_made_and_versioned(self):
        migrate(self.conn)
        tables = {row[0] for row in self.conn.execute("SELECT name FROM sqlite_master")}
        self.assertTrue({"items", "moves"} <= tables)
        self.assertEqual(self.conn.execute("PRAGMA user_version").fetchone()[0], 1)

    def test_a_second_migration_changes_nothing(self):
        migrate(self.conn)
        self.conn.execute("INSERT INTO items (sku, name) VALUES ('ABC-0001', 'bolt')")
        migrate(self.conn)
        rows = self.conn.execute("SELECT sku, name, quantity FROM items").fetchall()
        self.assertEqual(rows, [("ABC-0001", "bolt", 0)])


if __name__ == "__main__":
    unittest.main()
