import unittest

from client import request_options


class ClientTest(unittest.TestCase):
    def test_timeout_is_given_in_seconds(self):
        self.assertEqual(request_options()["timeout"], 30)
