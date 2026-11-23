import unittest

from text.slug import slugify


class SlugTest(unittest.TestCase):
    def test_punctuation_goes(self):
        self.assertEqual(slugify("Hello, World!"), "hello-world")

    def test_plain_words(self):
        self.assertEqual(slugify("Release notes"), "release-notes")
