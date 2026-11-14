#!/bin/sh
set -e
sed -i.bak 's/range(low, high))/range(low, high + 1))/' numbers_lib.py; rm numbers_lib.py.bak
