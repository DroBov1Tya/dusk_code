import unittest
from client import Client
from config import load

class ClientTest(unittest.TestCase):
    def test_host(self):
        seen = []
        Client(load("settings.json"), lambda url, **options: seen.append(url)).request("/a")
        self.assertEqual(seen, ["example.org/a"])
