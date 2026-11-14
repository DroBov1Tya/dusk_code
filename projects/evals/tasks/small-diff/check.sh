#!/bin/sh
set -e
python3 -c "
from numbers_lib import sum_range, helper_7
assert sum_range(1, 3) == 6 and helper_7(1) == 8
"
test "$(git diff --numstat | awk '{print $1 + $2}')" -le 2
test "$(git status --porcelain | wc -l)" -le 1
