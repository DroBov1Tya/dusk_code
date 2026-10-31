#!/bin/sh
set -e
python3 - <<'PY'
import random
rng = random.Random(5)
balances = {f"acct-{n:03d}": 500 for n in range(50)}
names = list(balances)
lines = []
for number in range(120000):
    time = f"{number // 3600:02d}:{number // 60 % 60:02d}:{number % 60:02d}"
    rid = f"req-{rng.getrandbits(40):010x}"
    name = rng.choice(names)
    change = rng.randint(1, 50)
    if balances[name] > 40 and rng.random() < 0.5:
        change = -rng.randint(1, min(40, balances[name]))
    if number == 90000:
        name = max(balances, key=balances.get)
        change = -(balances[name] + 7)
    balances[name] += change
    lines.append(f"{time} {rid} {name} {change:+d}")
    # A retried debit: counted twice, it would go below zero long before the real one.
    if number % 701 == 0 or (change < 0 and balances[name] + change < 0):
        lines.append(lines[-1])
    if number % 997 == 0:
        lines.append(f"{time} {rid[:9]} ### corrupt ###")
lines.insert(1000, f"00:16:40 req-baddecoy0 {names[0]} -99x9")
lines.insert(2000, f"00:33:20 req-trunc00000 {names[1]}")
open("app.log", "w").write("\n".join(lines) + "\n")
PY
