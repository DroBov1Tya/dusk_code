#!/bin/sh
# Installs duskcode: the terminal client, the desktop client, or both.
#   curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh
# It asks which one; DUSKCODE_INSTALL=tui, gui or both answers ahead, as on a server.
# Each part has its own script in scripts/, which this one runs: from the checkout when
# it is run from one, otherwise downloaded from the same repository over HTTPS.
set -eu

repo="DroBov1Tya/dusk_code"
ref="${DUSKCODE_REF:-main}"
here="$(cd "$(dirname "$0")" 2>/dev/null && pwd || true)"

choice="${DUSKCODE_INSTALL:-}"
if [ -z "$choice" ]; then
  if [ -r /dev/tty ]; then
    printf '\n  duskcode installer\n\n'
    printf '    1  terminal client (duskcode), for servers and terminals\n'
    printf '    2  desktop client (duskcode-gui), for macOS and Linux desktops\n'
    printf '    3  both\n\n  which one? [1] '
    read -r answer < /dev/tty || answer=""
    case "$answer" in
      2) choice="gui" ;;
      3) choice="both" ;;
      *) choice="tui" ;;
    esac
  else
    choice="tui"
  fi
fi
case "$choice" in
  tui | gui | both) ;;
  *) printf 'DUSKCODE_INSTALL must be tui, gui or both\n' >&2; exit 1 ;;
esac

run_part() {
  name="$1"
  if [ -n "$here" ] && [ -f "$here/scripts/install-$name.sh" ]; then
    sh "$here/scripts/install-$name.sh"
    return
  fi
  work="$(mktemp -d)"
  curl -fsSL --proto '=https' --tlsv1.2 -o "$work/install-$name.sh" \
    "https://raw.githubusercontent.com/$repo/$ref/scripts/install-$name.sh" \
    || { rm -rf "$work"; printf 'cannot download the %s installer\n' "$name" >&2; exit 1; }
  sh "$work/install-$name.sh"
  status=$?
  rm -rf "$work"
  return "$status"
}

case "$choice" in
  tui) run_part tui ;;
  gui) run_part gui ;;
  both) run_part tui && run_part gui ;;
esac
