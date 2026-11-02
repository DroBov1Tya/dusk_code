#!/bin/sh
set -e
cat > fees.py <<'PY'
import math
from decimal import Decimal, ROUND_HALF_UP


def fee(weight_kg, total, zone, fragile=False):
    charge = Decimal(0)
    if not (total >= 100 and zone in ("A", "B")):
        started = max(0, math.ceil(weight_kg - 2))
        charge = Decimal("5.00") + Decimal("1.50") * started
        if zone == "B":
            charge += Decimal("3.00")
        elif zone == "C":
            charge *= Decimal("1.25")
    if fragile:
        charge += Decimal("2.50")
    return float(charge.quantize(Decimal("0.01"), rounding=ROUND_HALF_UP))
PY
