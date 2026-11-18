#!/bin/sh
set -e
git diff --quiet HEAD -- tests inventory/__main__.py
for part in schema api cli; do
    python3 -m unittest -q "tests.test_$part"
done
python3 -m unittest -q
python3 "$(dirname "$0")/check.py"
