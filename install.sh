#!/bin/sh
# Installs the latest duskcode release:
#   curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh
# Binary and resources go under $XDG_DATA_HOME/duskcode (default ~/.local/share).
# A duskcode link goes to /usr/local/bin
# when it is writable, otherwise to ~/.local/bin. DUSKCODE_VERSION=v0.46.0 picks a release.
set -eu

# Colours and marks only on a terminal, and never with NO_COLOR set.
if [ -t 1 ] && [ -z "${NO_COLOR:-}" ]; then
  esc="$(printf '\033')"
  reset="${esc}[0m" bold="${esc}[1m" dim="${esc}[2m"
  violet="${esc}[38;2;157;124;255m" rose="${esc}[38;2;240;122;176m"
  teal="${esc}[38;2;94;214;194m" sun="${esc}[38;2;255;180;84m" ember="${esc}[38;2;255;99;99m"
else
  reset="" bold="" dim="" violet="" rose="" teal="" sun="" ember=""
fi
case "${LC_ALL:-${LC_CTYPE:-${LANG:-}}}" in
  *UTF-8* | *utf8* | *UTF8* | *utf-8*) step_mark="\342\227\206" ok_mark="\342\234\223" fail_mark="\342\234\227" ;;
  *) step_mark=">" ok_mark="+" fail_mark="x" ;;
esac
step_mark="$(printf "$step_mark")" ok_mark="$(printf "$ok_mark")" fail_mark="$(printf "$fail_mark")"

step() { printf '\n  %s%s%s %s%s%s\n' "$violet" "$step_mark" "$reset" "$bold" "$1" "$reset"; }
ok() { printf '    %s%s%s %s\n' "$teal" "$ok_mark" "$reset" "$1"; }
note() { printf '    %s%s%s\n' "$dim" "$1" "$reset"; }
# A path as people write it: the home folder as ~.
short() { case "$1" in "$HOME"/*) printf '~/%s' "${1#"$HOME"/}" ;; *) printf '%s' "$1" ;; esac; }
fail() {
  printf '\n  %s%s %s%s\n    nothing was installed\n\n' "$ember" "$fail_mark" "$1" "$reset" >&2
  exit 1
}

printf '\n  %sdusk%s%scode%s  %sinstaller%s\n' "$bold$violet" "$reset" "$rose" "$reset" "$dim" "$reset"

repo="DroBov1Tya/dusk_code"
version="${DUSKCODE_VERSION:-latest}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target="aarch64-apple-darwin" ;;
  Linux-x86_64 | Linux-amd64) target="x86_64-unknown-linux-musl" ;;
  Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-musl" ;;
  *) fail "no build for $(uname -s) $(uname -m)" ;;
esac

if [ "$version" = "latest" ]; then
  api="https://api.github.com/repos/$repo/releases/latest"
else
  api="https://api.github.com/repos/$repo/releases/tags/$version"
fi
archive="duskcode-$target.tar.gz"

work="$(mktemp -d)"
binary_stage=""
trap 'rm -rf "$work"; [ -z "$binary_stage" ] || rm -f "$binary_stage"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
step "Downloading"
# The release names its tag, and GitHub keeps the SHA-256 of every file in it.
curl -fsSL --proto '=https' --tlsv1.2 -H 'Accept: application/vnd.github+json' \
  -o "$work/release.json" "$api" || fail "cannot read the $version release from GitHub"
tag="$(tr ',' '\n' < "$work/release.json" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
expected="$(tr ',' '\n' < "$work/release.json" | awk -v name="\"$archive\"" '
  index($0, "\"name\"") && index($0, name) { found = 1; next }
  found && /"digest":/ { sub(/.*"sha256:/, ""); sub(/".*/, ""); print; exit }
  found && /"name":/ { exit }')"
[ -n "$tag" ] || fail "unexpected answer from GitHub"
[ -n "$expected" ] || fail "$tag has no build for $target yet: release builds take a few minutes after a tag"
note "$tag  $target"
# A progress bar on a terminal; quiet otherwise.
if [ -t 2 ]; then progress="-#"; else progress="-sS"; fi
curl -fL "$progress" --proto '=https' --tlsv1.2 -o "$work/$archive" \
  "https://github.com/$repo/releases/download/$tag/$archive" || fail "cannot download $archive"
ok "$archive"

step "Verifying"

if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$work/$archive" | cut -d ' ' -f 1)"
else
  actual="$(shasum -a 256 "$work/$archive" | cut -d ' ' -f 1)"
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch"
ok "sha256 $(printf '%s' "$actual" | cut -c 1-16)..."

tar -xzf "$work/$archive" -C "$work"
package="$work/duskcode-$target"
resources="$package/resources"
binary_version="$("$package/duskcode" --version)"
binary_version="${binary_version#duskcode }"
printf '%s\n' "$binary_version" | LC_ALL=C grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$' \
  || fail "invalid binary version"
[ "$tag" = "v$binary_version" ] || fail "binary does not match the $tag release"
ok "duskcode $binary_version runs here"
# 0.65.1 is the first resource-aware release. Historical binary-only archives,
# including the stable 0.60.0, remain installable; newer packages must fail closed.
major="${binary_version%%.*}"
minor_patch="${binary_version#*.}"
minor="${minor_patch%%.*}"
patch="${minor_patch#*.}"
resource_aware=false
if [ "$major" -gt 0 ] || [ "$minor" -gt 65 ] || { [ "$minor" -eq 65 ] && [ "$patch" -ge 1 ]; }; then
  resource_aware=true
fi
if [ "$resource_aware" = true ] || [ -e "$resources" ] || [ -L "$resources" ]; then
  step "Installing skills, agents and prompts"
  if [ ! -f "$resources/FORMAT" ] || [ "$(cat "$resources/FORMAT")" != 1 ] || \
     [ ! -f "$resources/VERSION" ] || [ "$(cat "$resources/VERSION")" != "$binary_version" ]; then
    fail "missing or incompatible resources for $binary_version"
  fi
  # The new binary validates and stages its resources using an OS advisory lock.
  # No config/UI initialization, resource download, or final binary activation here.
  "$package/duskcode" --install-resources "$resources" || fail "cannot stage the resources"
  skills="$(find "$resources/skills" -name SKILL.md | wc -l | tr -d ' ')"
  agents="$(find "$resources/agents" -name AGENT.md | wc -l | tr -d ' ')"
  ok "$skills skills, $agents agents"
fi
step "Installing the binary"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
case "$data_home" in
  /*) ;;
  *) fail "XDG_DATA_HOME must be absolute" ;;
esac
app_dir="$data_home/duskcode"
bin_dir="$app_dir/bin"
mkdir -p "$bin_dir"
[ ! -L "$bin_dir" ] || fail "install directory must not be a symlink"
# Stage beside the installed binary; only the final rename activates the update.
# A failure here can leave an unused complete snapshot, never a mismatched binary.
binary_stage="$(mktemp "$bin_dir/.duskcode.XXXXXX")"
install -m 755 "$package/duskcode" "$binary_stage"
mv -f "$binary_stage" "$bin_dir/duskcode"
binary_stage=""

if [ -d /usr/local/bin ] && [ -w /usr/local/bin ]; then
  link_dir="/usr/local/bin"
else
  link_dir="$HOME/.local/bin"
  mkdir -p "$link_dir"
fi
ln -sf "$bin_dir/duskcode" "$link_dir/duskcode"

ok "$(short "$bin_dir/duskcode")"
ok "linked from $(short "$link_dir/duskcode")"

printf '\n  %s%s %s%s is ready%s\n' "$teal" "$ok_mark" "$bold" "$("$bin_dir/duskcode" --version)" "$reset"
case ":$PATH:" in
  *":$link_dir:"*) printf '    run %sduskcode%s in the folder to work in\n\n' "$sun" "$reset" ;;
  *) printf '    add %s to PATH first:\n    %secho '"'"'export PATH="%s:$PATH"'"'"' >> ~/.profile%s\n\n' \
       "$(short "$link_dir")" "$sun" "$link_dir" "$reset" ;;
esac
