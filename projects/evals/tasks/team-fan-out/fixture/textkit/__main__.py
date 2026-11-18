"""python3 -m textkit FILE: the report of a chapter list.

The file holds one chapter a line, `TITLE | DURATION`, then a blank line and a free text
note. The report numbers the chapters in roman numerals, gives each a slug, sums the
durations and wraps the note to 32 columns.
"""

import sys

from textkit.duration import format_duration, parse_duration
from textkit.roman import to_roman
from textkit.slug import slugify
from textkit.wrap import wrap


def report(text):
    chapters, _, note = text.partition("\n\n")
    lines, total = [], 0
    for number, line in enumerate(chapters.splitlines(), start=1):
        title, _, duration = line.partition("|")
        seconds = parse_duration(duration.strip())
        total += seconds
        slug = slugify(title, max_length=20)
        lines.append(f"{to_roman(number)}. {slug} ({format_duration(seconds)})")
    lines.append(f"total {format_duration(total)}")
    lines.append("")
    lines.extend(wrap(note, 32))
    return "\n".join(lines)


def main(arguments):
    with open(arguments[0], encoding="utf-8") as file:
        print(report(file.read()))


if __name__ == "__main__":
    main(sys.argv[1:])
