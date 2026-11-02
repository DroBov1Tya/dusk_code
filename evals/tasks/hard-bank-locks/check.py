import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
import random, sys, threading, time
from bank import Bank
sys.setswitchinterval(1e-5)
names = [f"a{number}" for number in range(6)]
bank = Bank({name: 1000 for name in names})
done = threading.Event()
threading.Thread(target=lambda: (bank.transfer("a0", "a0", 10), done.set()), daemon=True).start()
assert done.wait(3), "a transfer to the same account hangs"
assert bank.total() == 6000 and bank.accounts["a0"].balance == 1000
assert bank.transfer("a0", "a1", 5000) is False
stop = threading.Event(); wrong = []
def mover(seed):
    rng = random.Random(seed)
    while not stop.is_set():
        bank.transfer(rng.choice(names), rng.choice(names), rng.randint(1, 50))
def auditor():
    while not stop.is_set():
        seen = bank.total()
        if seen != 6000:
            wrong.append(seen)
threads = [threading.Thread(target=mover, args=(n,), daemon=True) for n in range(8)]
threads += [threading.Thread(target=auditor, daemon=True) for _ in range(2)]
for thread in threads: thread.start()
time.sleep(5); stop.set()
for thread in threads: thread.join(8)
assert not any(thread.is_alive() for thread in threads), "deadlock"
assert not wrong, f"total was wrong {len(wrong)} times: {wrong[:3]}"
assert bank.total() == 6000
bank = Bank({"a": 10, "b": 10, "c": 10, "d": 10})
bank.accounts["a"].lock.acquire()
finished = threading.Event()
threading.Thread(target=lambda: (bank.transfer("c", "d", 1), finished.set()), daemon=True).start()
assert finished.wait(3), "an unrelated transfer waited on another account's lock"
