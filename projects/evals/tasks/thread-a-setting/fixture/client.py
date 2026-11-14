class Client:
    def __init__(self, config, transport):
        self.config = config
        self.transport = transport

    def request(self, path):
        return self.transport(self.config.host + path)
