#!/bin/sh
set -e
cat > cache.py <<'PY'
import heapq
import time
from collections import OrderedDict


class TTLCache:
    def __init__(self, capacity, ttl, clock=time.monotonic):
        self.capacity, self.ttl, self.clock = capacity, ttl, clock
        self.items = OrderedDict()
        self.deadlines = []

    def purge(self):
        now = self.clock()
        while self.deadlines and self.deadlines[0][0] <= now:
            deadline, _, key = heapq.heappop(self.deadlines)
            entry = self.items.get(key)
            if entry and entry[1] == deadline:
                del self.items[key]

    def get(self, key, default=None):
        self.purge()
        if key not in self.items:
            return default
        self.items.move_to_end(key)
        return self.items[key][0]

    def put(self, key, value):
        self.purge()
        if key not in self.items and len(self.items) >= self.capacity:
            self.items.popitem(last=False)
        deadline = self.clock() + self.ttl
        self.items[key] = (value, deadline)
        self.items.move_to_end(key)
        heapq.heappush(self.deadlines, (deadline, id(key), key))

    def __len__(self):
        self.purge()
        return len(self.items)
PY
