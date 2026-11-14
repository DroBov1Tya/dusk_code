import unittest
from prices import rounded

class RoundedTest(unittest.TestCase):
    def test_half_up(self):
        self.assertEqual(rounded(2.675), 2.68)

    def test_plain(self):
        self.assertEqual(rounded(1.2), 1.2)
