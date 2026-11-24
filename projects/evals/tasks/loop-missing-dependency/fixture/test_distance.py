import unittest

from distance import between


class DistanceTest(unittest.TestCase):
    def test_paris_to_london(self):
        self.assertAlmostEqual(between((48.8566, 2.3522), (51.5074, -0.1278)), 343.6, delta=1)
