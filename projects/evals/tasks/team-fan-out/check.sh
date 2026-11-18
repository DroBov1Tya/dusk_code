#!/bin/sh
set -e
git diff --quiet HEAD -- tests textkit/__main__.py sample.txt expected.txt
for part in slug wrap roman duration; do
    python3 -m unittest -q "tests.test_$part"
done
python3 -m unittest -q
python3 -m textkit sample.txt | diff - expected.txt
python3 "$(dirname "$0")/check.py"
