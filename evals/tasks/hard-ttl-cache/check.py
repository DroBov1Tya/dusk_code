import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
import random, time
from cache import TTLCache
class Clock:
    now = 0.0
    def __call__(self):
        return self.now
class Model:
    def __init__(self, capacity, ttl):
        self.capacity, self.ttl, self.items = capacity, ttl, []
    def purge(self, now):
        self.items = [item for item in self.items if item[2] > now]
    def find(self, key):
        return next((item for item in self.items if item[0] == key), None)
    def get(self, key, now):
        self.purge(now)
        item = self.find(key)
        if item is None:
            return "missing"
        self.items.remove(item); self.items.append(item)
        return item[1]
    def put(self, key, value, now):
        self.purge(now)
        item = self.find(key)
        if item:
            self.items.remove(item)
        elif len(self.items) == self.capacity:
            self.items.pop(0)
        self.items.append((key, value, now + self.ttl))
    def size(self, now):
        self.purge(now)
        return len(self.items)
random.seed(11)
for trial in range(300):
    capacity, ttl = random.randint(1, 6), random.choice([1, 2, 3, 5, 8])
    clock = Clock(); cache = TTLCache(capacity, ttl, clock=clock); model = Model(capacity, ttl)
    for step in range(300):
        clock.now += random.choice([0, 0, 0, 0.5, 1, 1, 2, 3])
        kind = random.random(); key = random.randint(0, 9)
        if kind < 0.45:
            cache.put(key, step); model.put(key, step, clock.now)
        elif kind < 0.9:
            got, wanted = cache.get(key, "missing"), model.get(key, clock.now)
            assert got == wanted, (trial, step, key, got, wanted)
        else:
            assert len(cache) == model.size(clock.now), (trial, step)
assert TTLCache(1, 1).get("nothing") is None
big = TTLCache(200000, 1000)
started = time.perf_counter()
for number in range(200000):
    big.put(number, number)
for number in range(200000):
    big.get(number)
assert time.perf_counter() - started < 6, "too slow for a large cache"
