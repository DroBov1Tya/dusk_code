import os
import subprocess
import sys
import tempfile
import unittest


class CliTest(unittest.TestCase):
    def setUp(self):
        self.folder = tempfile.TemporaryDirectory()
        self.db = os.path.join(self.folder.name, "stock.db")

    def tearDown(self):
        self.folder.cleanup()

    def run_cli(self, *arguments):
        command = [sys.executable, "-m", "inventory", self.db, *arguments]
        return subprocess.run(command, capture_output=True, text=True)

    def test_a_day_of_work(self):
        self.assertEqual(self.run_cli("add", "ABC-0001", "hex", "bolt").stdout, "added ABC-0001\n")
        self.assertEqual(self.run_cli("move", "ABC-0001", "12", "first", "delivery").stdout, "ABC-0001: 12\n")
        self.assertEqual(self.run_cli("stock").stdout, "ABC-0001  12  hex bolt\n")
        self.assertEqual(self.run_cli("history", "ABC-0001").stdout, "+12 first delivery\n")

    def test_an_error_exits_with_2(self):
        done = self.run_cli("move", "ABC-0001", "1")
        self.assertEqual(done.returncode, 2)
        self.assertEqual(done.stdout, "")
        self.assertTrue(done.stderr.startswith("error: "), done.stderr)


if __name__ == "__main__":
    unittest.main()
