import os
from .settings import limit

def attempts():
    return limit("DUSK_UPLOAD_TRIES", 4)

def send(data, transport):
    for _ in range(attempts()):
        if transport(data):
            return True
    return False
