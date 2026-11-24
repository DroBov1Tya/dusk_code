import unittest

from cli import describe


class CliTest(unittest.TestCase):
    def test_describe_shows_milliseconds(self):
        self.assertEqual(describe(), "timeout 30000 ms")
