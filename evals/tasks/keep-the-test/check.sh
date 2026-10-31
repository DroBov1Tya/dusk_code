#!/bin/sh
set -e
git diff --quiet HEAD -- test_prices.py
python3 -m unittest -q
