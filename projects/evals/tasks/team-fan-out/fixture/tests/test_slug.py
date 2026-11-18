import unittest

from textkit.slug import slugify


class SlugTest(unittest.TestCase):
    def test_words_become_lower_case_and_hyphens(self):
        self.assertEqual(slugify("Hello, World!"), "hello-world")
        self.assertEqual(slugify("  a  --  b  "), "a-b")

    def test_accents_are_dropped(self):
        self.assertEqual(slugify("Café Crème"), "cafe-creme")

    def test_nothing_left_gives_an_empty_slug(self):
        self.assertEqual(slugify("!!!"), "")

    def test_a_long_slug_is_cut_at_a_hyphen(self):
        self.assertEqual(slugify("one two three", max_length=9), "one-two")


if __name__ == "__main__":
    unittest.main()
