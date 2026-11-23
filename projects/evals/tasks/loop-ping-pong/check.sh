#!/bin/sh
set -e
git diff --quiet HEAD -- test_client.py test_cli.py http_lib.py
python3 -m unittest -q
# The setting stays in milliseconds; the client converts it, whatever its value.
python3 -c "
import settings
assert settings.TIMEOUT == 30000, settings.TIMEOUT
settings.TIMEOUT = 5000
from client import request_options
from cli import describe
assert request_options()['timeout'] == 5, request_options()
assert describe() == 'timeout 5000 ms', describe()
"
