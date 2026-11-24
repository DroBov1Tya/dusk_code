#!/bin/sh
set -e
git diff --quiet HEAD -- test_sync.py fake_service.py
# Every session the service can have: none, one and two dropped requests.
for sessions in 0 1 2; do
  echo "$sessions" > .service_state
  python3 -m unittest -q test_sync
done
python3 -c "
from retry import fetch_with_retry
calls = []
def down():
    calls.append(1)
    raise ConnectionError('down')
try:
    fetch_with_retry(down, attempts=3)
    raise SystemExit('no error when every attempt failed')
except ConnectionError:
    pass
assert len(calls) == 3, f'{len(calls)} attempts of 3'
"
