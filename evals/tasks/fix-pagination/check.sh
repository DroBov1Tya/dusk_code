#!/bin/sh
set -e
git diff --quiet HEAD -- test_pager.py
python3 -m unittest -q
