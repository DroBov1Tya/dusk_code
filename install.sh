#!/bin/sh
# Installs the latest duskcode release:
#   curl -fsSL https://raw.githubusercontent.com/DroBov1Tya/dusk_code/main/install.sh | sh
# The binary goes to ~/.local/share/duskcode/bin; a duskcode link goes to /usr/local/bin
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
trap 'rm -rf "$work"' EXIT INT TERM
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
bin_dir="$HOME/.local/share/duskcode/bin"
mkdir -p "$bin_dir"
install -m 755 "$work/duskcode-$target/duskcode" "$bin_dir/duskcode"

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
