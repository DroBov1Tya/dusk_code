import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
import ast, random, re
import calc
for node in ast.walk(ast.parse(open("calc.py").read())):
    if isinstance(node, ast.Call) and getattr(node.func, "id", "") in ("eval", "exec", "compile"):
        raise SystemExit("uses eval")
random.seed(7)
def expr(depth):
    if depth == 0 or random.random() < 0.25:
        return random.choice(["1", "2", "3", "0.5", "1.5", "4", "7", "10"])
    kind = random.random()
    if kind < 0.15:
        return random.choice("-+") + " " * random.randint(0, 1) + expr(depth - 1)
    if kind < 0.3:
        return "(" + expr(depth - 1) + ")"
    op = random.choice(["+", "-", "*", "/", "%", "**"])
    gap = " " * random.randint(0, 2)
    return expr(depth - 1) + gap + op + gap + expr(depth - 1)
done = 0
for _ in range(6000):
    text = expr(4)
    try:
        # As floats: integers would take for ever on something like 10**7**10.
        wanted = eval(re.sub(r"(?<![\d.])(\d+)(?![\d.])", r"\1.0", text))
    except (ZeroDivisionError, OverflowError):
        continue
    if isinstance(wanted, complex):
        continue
    try:
        got = calc.evaluate(text)
    except OverflowError:
        continue
    assert abs(got - wanted) <= 1e-9 * max(1, abs(wanted)), (text, got, wanted)
    done += 1
assert done > 3000
for text, wanted in [("-2**2", -4.0), ("2**-1", 0.5), ("2**3**2", 512.0), ("-3%5", 2.0), ("--1", 1.0), (".5+1", 1.5), ("2.*3", 6.0)]:
    assert abs(calc.evaluate(text) - wanted) < 1e-9, text
for bad in ["", "   ", "1+", "(1", "1)", "1 2", "1..2", "abc", "**2", "1/0", "2 * * 3", "()", "1 % 0"]:
    try:
        calc.evaluate(bad)
    except ValueError:
        continue
    raise SystemExit(f"accepted {bad!r}")
