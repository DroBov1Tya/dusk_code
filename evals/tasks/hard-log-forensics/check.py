import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
import os, re
line = re.compile(r"^\d\d:\d\d:\d\d (req-[0-9a-f]{10}) (acct-\d{3}) ([-+]\d+)$")
def first(dedupe):
    balances, seen = {}, set()
    for row in open("app.log"):
        match = line.match(row.strip())
        if not match:
            continue
        rid, account, change = match.group(1), match.group(2), int(match.group(3))
        if dedupe and rid in seen:
            continue
        seen.add(rid)
        balances[account] = balances.get(account, 500) + change
        if balances[account] < 0:
            return rid, account
right, naive = first(True), first(False)
assert right != naive, "the task does not tell the two apart"
answer = open(os.environ["EVAL_ANSWER"]).read()
assert right[0] in answer and right[1] in answer, ("wrong", right)
assert naive[0] not in answer, "counted a retry twice"
