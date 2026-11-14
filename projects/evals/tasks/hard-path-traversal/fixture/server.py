import os
from urllib.parse import unquote


def resolve(root, requested):
    """The file `requested` stands for under `root`."""
    path = unquote(requested).replace("../", "")
    return os.path.join(root, path.lstrip("/"))
