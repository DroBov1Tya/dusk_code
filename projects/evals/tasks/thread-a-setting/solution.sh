#!/bin/sh
set -e
python3 - <<'PY'
text = open('config.py').read()
text = text.replace("def __init__(self, host):\n        self.host = host", "def __init__(self, host, timeout=30):\n        self.host = host\n        self.timeout = timeout")
text = text.replace('return Config(data["host"])', 'return Config(data["host"], data.get("timeout", 30))')
open('config.py', 'w').write(text)
text = open('client.py').read().replace("self.transport(self.config.host + path)", "self.transport(self.config.host + path, timeout=self.config.timeout)")
open('client.py', 'w').write(text)
PY
