"""The rules of task.md the visible tests leave out, part by part."""

import sys

sys.path.insert(0, ".")


def refused(function, *arguments):
    try:
        function(*arguments)
    except ValueError:
        return True
    return False


def slug():
    from textkit.slug import slugify

    assert slugify("") == ""
    assert slugify("Ünïcödé — and 東京 too") == "unicode-and-too"
    assert slugify("v2.0_beta") == "v2-0-beta"
    assert slugify("one two three", max_length=7) == "one-two"
    assert slugify("one two three", max_length=8) == "one-two"
    assert slugify("one two three", max_length=13) == "one-two-three"
    assert slugify("extraordinary day", max_length=5) == "extra"
    assert slugify("a-b", max_length=2) == "a"
    assert refused(slugify, "x", 0)


def wrap():
    from textkit.wrap import wrap

    assert wrap("", 4) == []
    assert wrap("abcdefghij kl", 4) == ["abcd", "efgh", "ij", "kl"]
    assert wrap("xy abcdefgh z", 4) == ["xy", "abcd", "efgh", "z"]
    assert wrap("abcdef x", 3) == ["abc", "def", "x"]
    assert wrap("abcde x", 4) == ["abcd", "e x"]
    assert wrap("\n\n a \n \n\n\t\n b c \n\n", 3) == ["a", "", "b c"]
    assert wrap("one\ttwo\nthree", 20) == ["one two three"]
    assert refused(wrap, "a", 0)


def roman():
    from textkit.roman import from_roman, to_roman

    for number in range(1, 4000):
        assert from_roman(to_roman(number)) == number, number
    assert to_roman(3999) == "MMMCMXCIX" and to_roman(400) == "CD"
    for wrong in ["", "IIII", "IC", "iv", "VX", "MMMM", "XM", "IIV", "ABC", " I"]:
        assert refused(from_roman, wrong), wrong
    assert refused(to_roman, 4000) and refused(to_roman, -1)


def duration():
    from textkit.duration import format_duration, parse_duration

    assert parse_duration("0s") == 0
    assert parse_duration("90m") == 5400
    assert parse_duration("2h7s") == 7207
    for wrong in ["", "1h1h", "1m1h", "1d", "1 h", "-1s", "+1s", "h", "1.5h", "1h "]:
        assert refused(parse_duration, wrong), wrong
    assert format_duration(3600) == "1h"
    assert format_duration(3605) == "1h5s"
    assert format_duration(59) == "59s"
    assert format_duration(7260) == "2h1m"
    assert refused(format_duration, -1)


failed = []
for part in (slug, wrap, roman, duration):
    try:
        part()
    except Exception as error:
        failed.append(f"{part.__name__}: {type(error).__name__} {error}")
if failed:
    sys.exit("hidden cases fail: " + "; ".join(failed))
