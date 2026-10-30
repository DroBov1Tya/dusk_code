#!/bin/sh
# Installs the latest duskcode release:
#   curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh
# Binary and resources go under $XDG_DATA_HOME/duskcode (default ~/.local/share).
# A duskcode link goes to /usr/local/bin
# when it is writable, otherwise to ~/.local/bin. DUSKCODE_VERSION=v0.46.0 picks a release.
set -eu

repo="DroBov1Tya/dusk_code"
version="${DUSKCODE_VERSION:-latest}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target="aarch64-apple-darwin" ;;
  Linux-x86_64 | Linux-amd64) target="x86_64-unknown-linux-musl" ;;
  Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-musl" ;;
  *) echo "duskcode: no build for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac

if [ "$version" = "latest" ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/$version"
fi
archive="duskcode-$target.tar.gz"

work="$(mktemp -d)"
binary_stage=""
trap 'rm -rf "$work"; [ -z "$binary_stage" ] || rm -f "$binary_stage"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
echo "Downloading $archive"
curl -fsSL --proto '=https' --tlsv1.2 -o "$work/$archive" "$base/$archive"
curl -fsSL --proto '=https' --tlsv1.2 -o "$work/$archive.sha256" "$base/$archive.sha256"

expected="$(cut -d ' ' -f 1 < "$work/$archive.sha256")"
if command -v sha256sum >/dev/null 2>&1; then
  actual="$(sha256sum "$work/$archive" | cut -d ' ' -f 1)"
else
  actual="$(shasum -a 256 "$work/$archive" | cut -d ' ' -f 1)"
fi
if [ "$expected" != "$actual" ]; then
  echo "duskcode: checksum mismatch, nothing installed" >&2
  exit 1
fi

tar -xzf "$work/$archive" -C "$work"
package="$work/duskcode-$target"
resources="$package/resources"
binary_version="$("$package/duskcode" --version)"
binary_version="${binary_version#duskcode }"
if ! printf '%s\n' "$binary_version" | LC_ALL=C grep -Eq '^[0-9]+\.[0-9]+\.[0-9]+$'; then
  echo "duskcode: invalid binary version, nothing installed" >&2
  exit 1
fi
if [ "$version" != latest ] && [ "$version" != "v$binary_version" ]; then
  echo "duskcode: binary does not match the requested release, nothing installed" >&2
  exit 1
fi
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
  if [ ! -f "$resources/FORMAT" ] || [ "$(cat "$resources/FORMAT")" != 1 ] || \
     [ ! -f "$resources/VERSION" ] || [ "$(cat "$resources/VERSION")" != "$binary_version" ]; then
    echo "duskcode: missing or incompatible resources for $binary_version, nothing installed" >&2
    exit 1
  fi
  # The new binary validates and stages its resources using an OS advisory lock.
  # No config/UI initialization, resource download, or final binary activation here.
  "$package/duskcode" --install-resources "$resources"
fi
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
case "$data_home" in
  /*) ;;
  *) echo "duskcode: XDG_DATA_HOME must be absolute" >&2; exit 1 ;;
esac
app_dir="$data_home/duskcode"
bin_dir="$app_dir/bin"
mkdir -p "$bin_dir"
if [ -L "$bin_dir" ]; then
  echo "duskcode: install directory must not be a symlink" >&2
  exit 1
fi
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

echo "Installed $("$bin_dir/duskcode" --version) to $bin_dir, linked from $link_dir/duskcode"
case ":$PATH:" in
  *":$link_dir:"*) echo "Run: duskcode" ;;
  *) echo "Add $link_dir to PATH, for example: echo 'export PATH=\"$link_dir:\$PATH\"' >> ~/.profile" ;;
esac
