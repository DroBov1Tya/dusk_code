#!/bin/sh
# Packs the desktop client as Duskcode.app inside a disk image, for a release.
#   sh scripts/package-macos.sh <duskcode-gui binary> <version> <out dir>
# The app carries the resources of its version in Contents/Resources, so it runs on a
# Mac where nothing else was installed. With DUSK_SIGN_IDENTITY set (the name of a code
# signing certificate in a keychain, see scripts/make-signing-identity.sh) every release is
# signed by the same certificate, so macOS keeps the permissions and keychain access the
# user gave an earlier version. Without it the app is signed ad hoc, and macOS treats each
# version as a new program. Neither is notarized: the first start asks once in Privacy &
# Security.
set -eu

fail() { printf 'package-macos: %s\n' "$1" >&2; exit 1; }

[ "$(uname -s)" = "Darwin" ] || fail "runs on macOS only"
[ $# -eq 3 ] || fail "usage: package-macos.sh <binary> <version> <out dir>"
binary="$1"
version="$2"
out="$3"
here="$(cd "$(dirname "$0")/.." && pwd)"
[ -x "$binary" ] || fail "no binary at $binary"
case "$version" in
  *[!0-9.]* | "") fail "version must be digits and dots, such as 0.72.0" ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
app="$work/image/Duskcode.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

install -m 755 "$binary" "$app/Contents/MacOS/duskcode-gui"
cp -R "$here/resources" "$app/Contents/Resources/resources"
printf '%s\n' "$version" > "$app/Contents/Resources/resources/VERSION"

cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
  <key>CFBundleName</key><string>Duskcode</string>
  <key>CFBundleDisplayName</key><string>Duskcode</string>
  <key>CFBundleIdentifier</key><string>dev.duskcode.gui</string>
  <key>CFBundleExecutable</key><string>duskcode-gui</string>
  <key>CFBundleIconFile</key><string>icon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>$version</string>
  <key>CFBundleVersion</key><string>$version</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>NSMicrophoneUsageDescription</key><string>Duskcode listens to the microphone only while you dictate a message.</string>
</dict></plist>
PLIST

# Every size the Dock and Finder ask for, from the one picture.
set_dir="$work/icon.iconset"
mkdir -p "$set_dir"
for size in 16 32 128 256 512; do
  sips -z "$size" "$size" "$here/projects/gui/assets/icon.png" \
    --out "$set_dir/icon_${size}x${size}.png" >/dev/null
  double=$((size * 2))
  sips -z "$double" "$double" "$here/projects/gui/assets/icon.png" \
    --out "$set_dir/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$set_dir" -o "$app/Contents/Resources/icon.icns"

# Apple silicon runs only signed code. The designated requirement names the certificate,
# not this build's hash, so the next version signed by it is the same program to macOS.
identity="${DUSK_SIGN_IDENTITY:--}"
if [ "$identity" = "-" ]; then
  printf 'package-macos: DUSK_SIGN_IDENTITY is not set; signing ad hoc\n' >&2
fi
codesign --force --deep --timestamp=none --sign "$identity" "$app"
codesign --verify --deep --strict "$app"
codesign --display --requirements - "$app" 2>&1 | grep designated >&2 || true

# Dragging the app onto the Applications link installs it.
ln -s /Applications "$work/image/Applications"
mkdir -p "$out"
dmg="$out/Duskcode-$version-macos-arm64.dmg"
rm -f "$dmg"
hdiutil create -quiet -volname "Duskcode $version" -srcfolder "$work/image" \
  -fs HFS+ -format UDZO -ov "$dmg"
printf '%s\n' "$dmg"
