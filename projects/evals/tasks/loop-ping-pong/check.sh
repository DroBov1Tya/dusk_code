#!/bin/sh
set -e
git diff --quiet HEAD -- test_client.py test_cli.py http_lib.py
python3 -m unittest -q
python3 -c "
import settings
assert settings.TIMEOUT == 30000, settings.TIMEOUT
"
# The setting stays in milliseconds and the client converts it, whatever its value: run the
# project again, in a copy, with settings.py giving another value, so constants derived from
# it at import time are checked as well.
copy=$(mktemp -d)
trap 'rm -rf "$copy"' EXIT
cp -R . "$copy"
cd "$copy"
python3 -c "
import re
text = open('settings.py').read()
text, count = re.subn(r'(?m)^TIMEOUT\s*=.*$', 'TIMEOUT = 5_000', text)
assert count == 1, 'settings.py has no single TIMEOUT line'
open('settings.py', 'w').write(text)
"
python3 -c "
from client import request_options
from cli import describe
assert request_options()['timeout'] == 5, request_options()
assert describe() == 'timeout 5000 ms', describe()
"
