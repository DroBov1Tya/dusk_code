import sys
sys.path.insert(0, ".")
sys.dont_write_bytecode = True
from fees import fee
for args, wanted in [
    ((1, 10, "A"), 5.00), ((2, 10, "A"), 5.00), ((2.1, 10, "A"), 6.50), ((3, 10, "A"), 6.50), ((3.5, 10, "A"), 8.00),
    ((1, 10, "B"), 8.00), ((1, 10, "C"), 6.25), ((3, 10, "C"), 8.13),
    ((1, 100, "A"), 0.00), ((1, 100, "B"), 0.00), ((1, 100, "C"), 6.25), ((1, 99.99, "A"), 5.00),
    ((1, 10, "A", True), 7.50), ((1, 10, "C", True), 8.75), ((1, 100, "A", True), 2.50), ((1, 100, "C", True), 8.75),
    ((3, 150, "C", True), 10.63),
]:
    got = fee(*args)
    assert abs(got - wanted) < 1e-9, (args, got, wanted)
