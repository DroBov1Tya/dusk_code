#!/bin/sh
set -e
cat > text/slug.py <<'PY'
import re


def slugify(title):
    """A title as a URL slug: lower case, each run of characters other than a-z and 0-9
    becomes one hyphen, and the slug neither starts nor ends with one."""
    return re.sub(r"[^a-z0-9]+", "-", title.lower()).strip("-")
PY
