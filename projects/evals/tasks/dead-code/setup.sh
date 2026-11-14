#!/bin/sh
set -e
mkdir mods
: > mods/__init__.py
number=0
while [ "$number" -lt 40 ]; do
  {
    if [ "$number" -eq 17 ]; then printf 'def legacy_flush():\n    return "old"\n\n\n'; fi
    printf 'def work():\n    return %s\n\n\ndef legacy_flush_all():\n    return %s\n' "$number" "$number"
  } > "mods/part_$number.py"
  number=$((number + 1))
done
