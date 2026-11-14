#!/bin/sh
set -e
python3 -c "
import json, os, tempfile
from client import Client
from config import load
seen = []
transport = lambda url, **options: seen.append(options)
Client(load('settings.json'), transport).request('/a')
assert seen == [{'timeout': 30}], seen
with tempfile.TemporaryDirectory() as folder:
    path = os.path.join(folder, 's.json')
    json.dump({'host': 'h', 'timeout': 5}, open(path, 'w'))
    seen.clear()
    Client(load(path), transport).request('/a')
assert seen == [{'timeout': 5}], seen
"
python3 -m unittest -q
