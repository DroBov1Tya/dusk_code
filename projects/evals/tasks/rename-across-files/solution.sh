#!/bin/sh
set -e
sed -i.bak 's/fetch_user/load_user/g' store.py api/handlers.py api/admin.py jobs.py docs.md
find . -name '*.bak' -delete
