#!/bin/sh
set -e
sed -i.bak 's|timeout=settings.TIMEOUT)|timeout=settings.TIMEOUT / 1000)|' client.py; rm client.py.bak
