import unittest
from api.handlers import show
from api.admin import audit
from jobs import sync

class AllTest(unittest.TestCase):
    def test_all(self):
        self.assertEqual(show(1), 1)
        self.assertEqual(audit(2), {"id": 2})
        self.assertEqual(sync([3]), [{"id": 3}])
