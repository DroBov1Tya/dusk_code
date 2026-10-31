#!/bin/sh
set -e
test "$(python3 report.py sample.txt)" = "lines: 2
words: 3"
python3 -c "
import json, subprocess
for arguments in (['sample.txt', '--json'], ['--json', 'sample.txt']):
    out = subprocess.check_output(['python3', 'report.py', *arguments], text=True)
    assert json.loads(out) == {'lines': 2, 'words': 3}, arguments
"
