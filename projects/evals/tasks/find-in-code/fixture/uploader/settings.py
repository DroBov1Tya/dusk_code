import os

def limit(name, fallback):
    return int(os.environ.get(name, fallback))
