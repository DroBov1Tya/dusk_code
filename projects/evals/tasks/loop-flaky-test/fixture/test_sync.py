import unittest

from fake_service import FlakyService
from sync import sync


class SyncTest(unittest.TestCase):
    def test_sync_gets_every_item(self):
        self.assertEqual(sync(FlakyService()), [1, 2, 3])


if __name__ == "__main__":
    unittest.main()
