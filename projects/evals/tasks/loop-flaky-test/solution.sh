#!/bin/sh
set -e
sed -i.bak 's/range(1, attempts)/range(attempts)/' retry.py; rm retry.py.bak
