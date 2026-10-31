#!/bin/sh
set -e
python3 - <<'PY'
text = open('urls.py').read().replace('{parts.netloc.lower()}{parts.path}"', '{parts.netloc.lower()}{parts.path.rstrip(chr(47))}"')
open('urls.py', 'w').write(text)
PY
