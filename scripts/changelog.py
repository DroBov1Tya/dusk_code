#!/usr/bin/env python3
"""Changelog fragments: `check` validates changes/*.md, `release X.Y.Z` folds them into CHANGELOG.md.

Each change adds changes/<topic>.md: the first line is the section (Added, Changed, Fixed,
Removed or Security), the rest is the entry text. Parallel branches then never touch the same
file. Standard library only.
"""

import datetime
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
SECTIONS = ["Added", "Changed", "Fixed", "Removed", "Security"]
VERSION = re.compile(r"\d+\.\d+\.\d+")


def fragments(folder: Path) -> list[tuple[Path, str, str]]:
    """Returns (path, section, text) for every fragment, sorted by file name; raises on a bad one."""
    found = []
    for path in sorted(folder.glob("*.md")):
        if path.name == "README.md":
            continue
        section, _, text = path.read_text(encoding="utf-8").partition("\n")
        section, text = section.strip(), text.strip()
        if section not in SECTIONS:
            raise ValueError(f"{path.name}: first line must be one of {', '.join(SECTIONS)}, got {section!r}")
        if not text:
            raise ValueError(f"{path.name}: the entry text is empty")
        found.append((path, section, text))
    return found


def bullet(text: str) -> str:
    if text.startswith("- "):
        return text
    first, *rest = text.splitlines()
    return "\n".join([f"- {first}"] + [f"  {line}" if line else line for line in rest])


def split_unreleased(changelog: str) -> tuple[str, dict[str, str], str]:
    """Returns (text before Unreleased, its sections, text from the next release on)."""
    match = re.search(r"^## Unreleased[ \t]*\n", changelog, re.M)
    if not match:
        raise ValueError("CHANGELOG.md has no '## Unreleased' heading")
    after = changelog[match.end():]
    nxt = re.search(r"^## ", after, re.M)
    body, tail = (after[: nxt.start()], after[nxt.start():]) if nxt else (after, "")
    parts = re.split(r"^### (.+?)[ \t]*\n", body, flags=re.M)
    if parts[0].strip():
        raise ValueError("text in Unreleased outside a '###' section")
    sections = {}
    for name, text in zip(parts[1::2], parts[2::2]):
        if name not in SECTIONS:
            raise ValueError(f"unknown section in Unreleased: {name}")
        sections[name] = text.strip()
    return changelog[: match.start()], sections, tail


def release(version: str, root: Path, today: str) -> None:
    if not VERSION.fullmatch(version):
        raise ValueError(f"version must look like 1.2.3, got {version!r}")
    changelog_path = root / "CHANGELOG.md"
    head, sections, tail = split_unreleased(changelog_path.read_text(encoding="utf-8"))
    found = fragments(root / "changes")
    out = [f"## {version} - {today}\n"]
    for name in SECTIONS:
        entries = [sections[name]] if sections.get(name) else []
        entries += [bullet(text) for _, section, text in found if section == name]
        if entries:
            out.append(f"### {name}\n\n" + "\n\n".join(entries) + "\n")
    if len(out) == 1:
        raise ValueError("nothing to release: no fragments and Unreleased is empty")
    text = head + "## Unreleased\n\n" + "\n".join(out) + ("\n" + tail if tail else "")
    changelog_path.write_text(text, encoding="utf-8")
    for path, _, _ in found:
        path.unlink()


def main(argv: list[str]) -> int:
    try:
        if argv[1:2] == ["check"] and len(argv) == 2:
            count = len(fragments(ROOT / "changes"))
            print(f"changes/: {count} fragment(s) ok")
            return 0
        if argv[1:2] == ["release"] and len(argv) == 3:
            release(argv[2], ROOT, datetime.date.today().isoformat())
            return 0
    except (ValueError, OSError) as error:
        print(f"changelog: {error}", file=sys.stderr)
        return 1
    print("usage: changelog.py check | release X.Y.Z", file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv))
