import unittest

from textkit.duration import format_duration, parse_duration


class DurationTest(unittest.TestCase):
    def test_parts_are_added_up(self):
        self.assertEqual(parse_duration("1h30m15s"), 5415)
        self.assertEqual(parse_duration("45m"), 2700)

    def test_seconds_are_written_back(self):
        self.assertEqual(format_duration(5415), "1h30m15s")
        self.assertEqual(format_duration(0), "0s")

    def test_garbage_is_refused(self):
        with self.assertRaises(ValueError):
            parse_duration("soon")


if __name__ == "__main__":
    unittest.main()
