#!/bin/sh
set -e
cat > server.py <<'PY'
import os
from urllib.parse import unquote


def resolve(root, requested):
    """The real path of the file `requested` stands for under `root`."""
    path = requested
    for _ in range(16):
        decoded = unquote(path)
        if decoded == path:
            break
        path = decoded
    else:
        raise PermissionError(requested)
    if "\x00" in path:
        raise PermissionError(requested)
    root = os.path.realpath(root)
    real = os.path.realpath(os.path.join(root, path.lstrip("/")))
    if not real.startswith(root + os.sep):
        raise PermissionError(requested)
    return real
PY
