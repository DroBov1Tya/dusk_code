#!/bin/sh
set -e
sed -i.bak 's/rate \/ 1000/rate \/ 100/' money.py; rm money.py.bak
