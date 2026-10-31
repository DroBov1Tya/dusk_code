#!/bin/sh
set -e
cat > prices.py <<'PY'
from decimal import Decimal, ROUND_HALF_UP


def rounded(price):
    """A price to whole cents, half up: 2.675 becomes 2.68."""
    return float(Decimal(str(price)).quantize(Decimal("0.01"), rounding=ROUND_HALF_UP))
PY
