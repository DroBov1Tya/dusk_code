#!/bin/sh
set -e
sed -i.bak 's/start + size - 1/start + size/' pager.py; rm pager.py.bak
