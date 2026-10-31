#!/bin/sh
set -e
python3 - <<'PY'
import re
path = 'mods/part_17.py'
text = open(path).read()
text = re.sub(r'def legacy_flush\(\):\n    return "old"\n\n\n', '', text)
open(path, 'w').write(text)
PY
