#!/bin/sh
# Prints the map of docs/structure.md: for each crate, its module directories up to three
# levels under src/ with the number of .rs files in each, so the numbers are never typed.
#   scripts/structure-map.sh            every crate
#   scripts/structure-map.sh core       one crate, by its place under projects/
set -eu

here=$(cd "$(dirname "$0")/.." && pwd)
crates="protocol core server clients/terminal clients/desktop clients/browser plugins/telegram evals"
[ $# -gt 0 ] && crates="$*"

for crate in $crates; do
  src="$here/projects/$crate/src"
  [ -d "$src" ] || { echo "no crate at projects/$crate" >&2; exit 1; }
  printf '### %s\n\n```\n' "$crate"
  (cd "$src" && find . -mindepth 1 -maxdepth 3 -type d | sort | while read -r dir; do
    depth=$(printf '%s' "${dir#./}" | tr -cd '/' | wc -c | tr -d ' ')
    files=$(find "$dir" -name '*.rs' | wc -l | tr -d ' ')
    [ "$files" -eq 0 ] && continue
    indent=$(printf '%*s' $((depth * 2)) '')
    printf '%s%s/  (%s)\n' "$indent" "$(basename "$dir")" "$files"
  done)
  printf '```\n\n'
done
