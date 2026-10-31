#!/bin/sh
set -e
if grep -rn "def legacy_flush\b" --include=*.py .; then exit 1; fi
python3 -c "
import importlib
for number in range(40):
    module = importlib.import_module(f'mods.part_{number}')
    assert module.work() == number
    assert module.legacy_flush_all() == number
"
