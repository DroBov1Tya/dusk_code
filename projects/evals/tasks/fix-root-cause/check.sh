#!/bin/sh
set -e
git diff --quiet HEAD -- test_orders.py
python3 -c "
from orders import total
from tax import with_tax
assert total(200.0, 10) == 180.0
assert with_tax(100.0, 20) == 120.0, 'the fix belongs in percent, which tax uses as well'
"
