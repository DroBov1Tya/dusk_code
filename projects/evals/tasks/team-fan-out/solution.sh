#!/bin/sh
set -e
cat > textkit/slug.py <<'PY'
import re
import unicodedata


def slugify(text, max_length=None):
    if max_length is not None and max_length < 1:
        raise ValueError("max_length must be at least 1")
    plain = unicodedata.normalize("NFKD", text).encode("ascii", "ignore").decode()
    slug = re.sub(r"[^a-z0-9]+", "-", plain.lower()).strip("-")
    if max_length is None or len(slug) <= max_length:
        return slug
    cut = slug[: max_length + 1]
    if "-" in cut:
        return cut[: cut.rindex("-")].rstrip("-")
    return slug[:max_length]
PY
cat > textkit/wrap.py <<'PY'
import re


def wrap(text, width):
    if width < 1:
        raise ValueError("width must be at least 1")
    lines = []
    for paragraph in re.split(r"\n\s*\n", text):
        words = paragraph.split()
        if not words:
            continue
        if lines:
            lines.append("")
        line = ""
        for word in words:
            if line and len(line) + 1 + len(word) <= width:
                line += " " + word
                continue
            if line:
                lines.append(line)
            while len(word) > width:
                lines.append(word[:width])
                word = word[width:]
            line = word
        lines.append(line)
    return lines
PY
cat > textkit/roman.py <<'PY'
NUMERALS = [
    (1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"),
    (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I"),
]


def to_roman(number):
    if not 1 <= number <= 3999:
        raise ValueError("roman numerals go from 1 to 3999")
    written = ""
    for value, numeral in NUMERALS:
        count, number = divmod(number, value)
        written += numeral * count
    return written


def from_roman(text):
    total, rest = 0, text
    for value, numeral in NUMERALS:
        while rest.startswith(numeral):
            total += value
            rest = rest[len(numeral):]
    if rest or not 1 <= total <= 3999 or to_roman(total) != text:
        raise ValueError(f"not a roman numeral: {text!r}")
    return total
PY
cat > textkit/duration.py <<'PY'
import re

FORM = re.compile(r"(?:([0-9]+)h)?(?:([0-9]+)m)?(?:([0-9]+)s)?")


def parse_duration(text):
    match = FORM.fullmatch(text)
    if not text or not match:
        raise ValueError(f"not a duration: {text!r}")
    hours, minutes, seconds = (int(part or 0) for part in match.groups())
    return hours * 3600 + minutes * 60 + seconds


def format_duration(seconds):
    if seconds < 0:
        raise ValueError("a duration is not negative")
    hours, rest = divmod(seconds, 3600)
    minutes, seconds = divmod(rest, 60)
    parts = [(hours, "h"), (minutes, "m"), (seconds, "s")]
    return "".join(f"{value}{unit}" for value, unit in parts if value) or "0s"
PY
