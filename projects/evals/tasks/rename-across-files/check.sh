#!/bin/sh
set -e
if grep -rn "fetch_user" --include=*.py --include=*.md .; then exit 1; fi
python3 -c "from store import load_user; assert load_user(1) == {'id': 1}"
test -f fetch_user_notes.txt
python3 -m unittest -q
