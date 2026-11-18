import unittest

from textkit.wrap import wrap


class WrapTest(unittest.TestCase):
    def test_words_fill_each_line(self):
        self.assertEqual(wrap("a bb ccc dddd", 6), ["a bb", "ccc", "dddd"])

    def test_paragraphs_are_kept_apart(self):
        self.assertEqual(wrap("one two\n\nthree", 10), ["one two", "", "three"])

    def test_empty_text_gives_no_lines(self):
        self.assertEqual(wrap("   ", 5), [])


if __name__ == "__main__":
    unittest.main()
