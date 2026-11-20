#!/bin/sh
# Removes old build artifacts, since cargo never deletes any.
#   sh scripts/clean-build.sh             what was not used for 7 days
#   sh scripts/clean-build.sh --days N    what was not used for N days
#   sh scripts/clean-build.sh --all       everything (cargo clean)
# Run from the repository root; it touches nothing outside the target folder.
set -eu

fail() { printf '  x %s\n' "$1" >&2; exit 1; }

[ -f Cargo.toml ] && [ -f scripts/clean-build.sh ] && [ -e .git ] \
  || fail "run it from the repository root"

days=7
all=no
while [ $# -gt 0 ]; do
  case "$1" in
    --all) all=yes ;;
    --days) shift; days="${1:-}" ;;
    *) fail "unknown argument: $1" ;;
  esac
  shift
done
case "$days" in '' | *[!0-9]*) fail "--days takes a whole number" ;; esac

# Always this checkout's own folder, whatever CARGO_TARGET_DIR says elsewhere.
target="target"
CARGO_TARGET_DIR="$PWD/$target"
export CARGO_TARGET_DIR
[ -d "$target" ] || { printf '  no build folder at %s\n' "$target"; exit 0; }
size() { du -sh "$target" 2>/dev/null | cut -f1; }
printf '  %s before: %s\n' "$target" "$(size)"

if [ "$all" = yes ]; then
  cargo clean
elif cargo sweep --version >/dev/null 2>&1; then
  cargo sweep --time "$days"
else
  for dir in "$target"/*/deps "$target"/*/incremental "$target"/*/build; do
    [ -d "$dir" ] || continue
    find "$dir" -mindepth 1 -depth -atime +"$days" \( -type f -o -type l \) -exec rm -f {} +
    find "$dir" -mindepth 1 -depth -type d -empty -exec rmdir {} +
  done
fi

printf '  %s after:  %s\n' "$target" "$(size)"
