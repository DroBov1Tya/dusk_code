import unittest
import socket

class LiveTest(unittest.TestCase):
    def test_service(self):
        with socket.create_connection(("127.0.0.1", 59321), timeout=1):
            pass
