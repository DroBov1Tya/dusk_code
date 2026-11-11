#!/bin/sh
# Installs or updates the desktop client, duskcode-gui.
#   sh scripts/install-gui.sh            from a checkout: builds it from source
#   DUSKCODE_VERSION=v0.71.0 sh ...      a release, once releases carry the GUI
# An existing install is found and updated in place; only the final rename activates it.
# The binary goes beside the terminal client under $XDG_DATA_HOME/duskcode/bin, with
# an app bundle in ~/Applications on macOS and a desktop entry on Linux.
set -eu

say() { printf '  %s\n' "$1"; }
fail() { printf '\n  x %s\n    nothing was changed\n\n' "$1" >&2; exit 1; }

repo="DroBov1Tya/dusk_code"
here="$(cd "$(dirname "$0")/.." 2>/dev/null && pwd || true)"
data_home="${XDG_DATA_HOME:-$HOME/.local/share}"
case "$data_home" in /*) ;; *) fail "XDG_DATA_HOME must be absolute" ;; esac
app_dir="$data_home/duskcode"
bin_dir="$app_dir/bin"
installed="$bin_dir/duskcode-gui"

printf '\n  duskcode-gui installer\n\n'
if [ -x "$installed" ]; then
  say "found $("$installed" --version 2>/dev/null || echo 'an older build'): updating it"
else
  say "no desktop client yet: installing it"
fi

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) target="aarch64-apple-darwin" ;;
  Linux-x86_64 | Linux-amd64) target="x86_64-unknown-linux-gnu" ;;
  Linux-aarch64 | Linux-arm64) target="aarch64-unknown-linux-gnu" ;;
  *) fail "no desktop build for $(uname -s) $(uname -m)" ;;
esac

work="$(mktemp -d)"
stage=""
trap 'rm -rf "$work"; [ -z "$stage" ] || rm -f "$stage"' EXIT

built=""
# A release on a Mac is the signed app in a disk image. It is fetched here with curl, which
# puts no quarantine mark on it, so macOS opens it without the Privacy & Security step a
# browser download needs. It updates itself from then on.
if [ "$(uname -s)" = "Darwin" ] && { [ -n "${DUSKCODE_VERSION:-}" ] || [ -z "$here" ] || [ ! -f "$here/projects/gui/Cargo.toml" ]; }; then
  version="${DUSKCODE_VERSION:-latest}"
  if [ "$version" = "latest" ]; then
    api="https://api.github.com/repos/$repo/releases/latest"
  else
    api="https://api.github.com/repos/$repo/releases/tags/$version"
  fi
  curl -fsSL --proto '=https' --tlsv1.2 -o "$work/release.json" "$api" \
    || fail "cannot read the $version release from GitHub"
  tag="$(tr ',' '\n' < "$work/release.json" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
  image="Duskcode-${tag#v}-macos-arm64.dmg"
  expected="$(tr ',' '\n' < "$work/release.json" | awk -v name="\"$image\"" '
    index($0, "\"name\"") && index($0, name) { found = 1; next }
    found && /"digest":/ { sub(/.*"sha256:/, ""); sub(/".*/, ""); print; exit }
    found && /"name":/ { exit }')"
  [ -n "$expected" ] || fail "${tag:-the release} has no $image yet; try again in a few minutes"
  curl -fL -sS --proto '=https' --tlsv1.2 -o "$work/$image" \
    "https://github.com/$repo/releases/download/$tag/$image" || fail "cannot download $image"
  [ "$(shasum -a 256 "$work/$image" | cut -d ' ' -f 1)" = "$expected" ] || fail "checksum mismatch"
  mkdir -p "$work/mount" "$HOME/Applications"
  hdiutil attach -nobrowse -readonly -noverify -mountpoint "$work/mount" "$work/$image" >/dev/null \
    || fail "cannot open the disk image"
  rm -rf "$HOME/Applications/Duskcode.app.new"
  ditto "$work/mount/Duskcode.app" "$HOME/Applications/Duskcode.app.new" \
    || { hdiutil detach -force "$work/mount" >/dev/null 2>&1; fail "cannot copy the app"; }
  hdiutil detach -force "$work/mount" >/dev/null 2>&1 || true
  xattr -dr com.apple.quarantine "$HOME/Applications/Duskcode.app.new" 2>/dev/null || true
  rm -rf "$HOME/Applications/Duskcode.app"
  mv "$HOME/Applications/Duskcode.app.new" "$HOME/Applications/Duskcode.app"
  say "+ $HOME/Applications/Duskcode.app"
  printf '\n  %s is ready; open it from Launchpad or Spotlight\n\n' "$tag"
  exit 0
fi
if [ -n "$here" ] && [ -f "$here/projects/gui/Cargo.toml" ] && [ -z "${DUSKCODE_VERSION:-}" ]; then
  command -v cargo >/dev/null 2>&1 || fail "cargo is needed to build from source: https://rustup.rs"
  say "building from $here (a few minutes the first time)"
  # gpui needs a newer Rust than some stable installs carry.
  (cd "$here" && cargo build --release -p dusk_gui) || fail "the build failed; try: rustup update stable"
  built="$here/target/release/duskcode-gui"
  icon="$here/projects/gui/assets/icon.png"
else
  version="${DUSKCODE_VERSION:-latest}"
  if [ "$version" = "latest" ]; then
    api="https://api.github.com/repos/$repo/releases/latest"
  else
    api="https://api.github.com/repos/$repo/releases/tags/$version"
  fi
  # A release on Linux is the AppImage; it is fetched whole and checked against its digest.
  case "$target" in
    x86_64-*) image_arch="x86_64" ;;
    aarch64-*) image_arch="aarch64" ;;
  esac
  curl -fsSL --proto '=https' --tlsv1.2 -o "$work/release.json" "$api" \
    || fail "cannot read the $version release from GitHub"
  tag="$(tr ',' '\n' < "$work/release.json" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n 1)"
  archive="Duskcode-${tag#v}-linux-$image_arch.AppImage"
  expected="$(tr ',' '\n' < "$work/release.json" | awk -v name="\"$archive\"" '
    index($0, "\"name\"") && index($0, name) { found = 1; next }
    found && /"digest":/ { sub(/.*"sha256:/, ""); sub(/".*/, ""); print; exit }
    found && /"name":/ { exit }')"
  [ -n "$expected" ] || fail "${tag:-the release} has no $archive yet; run this script from a checkout to build it"
  curl -fL -sS --proto '=https' --tlsv1.2 -o "$work/$archive" \
    "https://github.com/$repo/releases/download/$tag/$archive" || fail "cannot download $archive"
  actual="$(sha256sum "$work/$archive" | cut -d ' ' -f 1)"
  [ "$expected" = "$actual" ] || fail "checksum mismatch"
  chmod +x "$work/$archive"
  built="$work/$archive"
  # The icon of the desktop entry comes out of the image; it runs without FUSE for this.
  (cd "$work" && APPIMAGE_EXTRACT_AND_RUN=1 "$built" --appimage-extract duskcode.png >/dev/null 2>&1) || true
  icon="$work/squashfs-root/duskcode.png"
fi
APPIMAGE_EXTRACT_AND_RUN=1 "$built" --version >/dev/null 2>&1 || fail "the new build does not run here"

mkdir -p "$bin_dir"
[ ! -L "$bin_dir" ] || fail "install directory must not be a symlink"
stage="$(mktemp "$bin_dir/.duskcode-gui.XXXXXX")"
install -m 755 "$built" "$stage"
mv -f "$stage" "$installed"
stage=""
say "+ $installed"

case "$(uname -s)" in
  Darwin)
    # A bundle, so the Dock and Spotlight show it with its icon.
    bundle="$HOME/Applications/Duskcode.app"
    mkdir -p "$bundle/Contents/MacOS" "$bundle/Contents/Resources"
    cat > "$bundle/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Duskcode</string>
  <key>CFBundleIdentifier</key><string>dev.duskcode.gui</string>
  <key>CFBundleExecutable</key><string>duskcode-gui</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Duskcode listens to the microphone only while you dictate a message.</string>
</dict></plist>
PLIST
    ln -sf "$installed" "$bundle/Contents/MacOS/duskcode-gui"
    # Apple silicon runs only signed code; a build made here is signed ad hoc.
    codesign --force --sign - "$installed" >/dev/null 2>&1 || true
    if [ -f "$icon" ] && command -v iconutil >/dev/null 2>&1; then
      set_dir="$work/icon.iconset"
      mkdir -p "$set_dir"
      for size in 16 32 128 256 512; do
        sips -z "$size" "$size" "$icon" --out "$set_dir/icon_${size}x${size}.png" >/dev/null
        double=$((size * 2))
        sips -z "$double" "$double" "$icon" --out "$set_dir/icon_${size}x${size}@2x.png" >/dev/null
      done
      iconutil -c icns "$set_dir" -o "$bundle/Contents/Resources/icon.icns"
    fi
    say "+ $bundle"
    ;;
  Linux)
    apps="$data_home/applications"
    icons="$data_home/icons/hicolor/512x512/apps"
    mkdir -p "$apps" "$icons"
    [ ! -f "$icon" ] || install -m 644 "$icon" "$icons/duskcode.png"
    cat > "$apps/duskcode.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Duskcode
Exec=$installed
Icon=duskcode
StartupWMClass=duskcode
Categories=Development;
DESKTOP
    say "+ $apps/duskcode.desktop"
    ;;
esac

printf '\n  %s is ready\n\n' "$("$installed" --version)"
