import os
import tempfile
import unittest

from inventory import api


class ApiTest(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.conn = api.open_store(os.path.join(self.folder.name, "stock.db"))

    def tearDown(self):
        self.conn.close()
        self.folder.cleanup()

    def test_moves_change_the_stock(self):
        api.add_item(self.conn, "ABC-0001", "bolt")
        self.assertEqual(api.move(self.conn, "ABC-0001", 5, "delivery"), 5)
        self.assertEqual(api.move(self.conn, "ABC-0001", -2), 3)
        self.assertEqual(api.stock(self.conn), [("ABC-0001", "bolt", 3)])
        self.assertEqual(api.history(self.conn, "ABC-0001"), [(5, "delivery"), (-2, "")])

    def test_stock_never_goes_below_zero(self):
        api.add_item(self.conn, "ABC-0001", "bolt")
        with self.assertRaises(ValueError):
            api.move(self.conn, "ABC-0001", -1)

    def test_a_bad_sku_is_refused(self):
        with self.assertRaises(ValueError):
            api.add_item(self.conn, "abc-1", "bolt")


if __name__ == "__main__":
    unittest.main()
