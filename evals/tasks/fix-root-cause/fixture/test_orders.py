import unittest
from orders import total

class TotalTest(unittest.TestCase):
    def test_discount(self):
        self.assertEqual(total(200.0, 10), 180.0)
