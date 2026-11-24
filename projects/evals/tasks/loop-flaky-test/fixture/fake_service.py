"""A stand-in for the sync server, as unreliable as the real one: each session drops
up to two of its first requests."""

import os

STATE = os.path.join(os.path.dirname(os.path.abspath(__file__)), ".service_state")


class FlakyService:
    def __init__(self):
        try:
            with open(STATE) as file:
                sessions = int(file.read().strip() or 0)
        except (OSError, ValueError):
            sessions = 0
        with open(STATE, "w") as file:
            file.write(str(sessions + 1))
        self.drops = sessions % 3
        self.calls = 0

    def get(self):
        self.calls += 1
        if self.calls <= self.drops:
            raise ConnectionError("connection reset by peer")
        return {"items": [1, 2, 3]}
