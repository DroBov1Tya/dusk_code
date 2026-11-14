#!/bin/sh
set -e
cat > slug.py <<'PY'
import re


def slugify(text):
    return re.sub(r"[\W_]+", "-", text.lower()).strip("-")
PY
