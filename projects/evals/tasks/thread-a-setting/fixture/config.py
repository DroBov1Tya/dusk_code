import json

class Config:
    def __init__(self, host):
        self.host = host

def load(path):
    with open(path) as file:
        data = json.load(file)
    return Config(data["host"])
