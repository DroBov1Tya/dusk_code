#!/bin/sh
# Packs the desktop client as an AppImage, for a release.
#   sh scripts/package-linux.sh <duskcode-gui binary> <version> <x86_64|aarch64> <out dir>
# The image carries the resources of its version in usr/share/duskcode/resources, so it runs
# where nothing else was installed. The window's libraries (Wayland or X11, Vulkan, ALSA,
# fontconfig) are the machine's own, as on any desktop Linux; the image does not bundle them.
set -eu

fail() { printf 'package-linux: %s\n' "$1" >&2; exit 1; }

[ "$(uname -s)" = "Linux" ] || fail "runs on Linux only"
[ $# -eq 4 ] || fail "usage: package-linux.sh <binary> <version> <x86_64|aarch64> <out dir>"
binary="$1"
version="$2"
arch="$3"
out="$4"
here="$(cd "$(dirname "$0")/.." && pwd)"
[ -x "$binary" ] || fail "no binary at $binary"
case "$version" in
  *[!0-9.]* | "") fail "version must be digits and dots, such as 0.72.0" ;;
esac

# appimagetool is fetched by version and checked against its digest before it runs.
tool_version="1.9.1"
case "$arch" in
  x86_64) tool_sha="ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0" ;;
  aarch64) tool_sha="f0837e7448a0c1e4e650a93bb3e85802546e60654ef287576f46c71c126a9158" ;;
  *) fail "unknown architecture $arch" ;;
esac

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
tool="$work/appimagetool"
curl -fsSL --proto '=https' --tlsv1.2 -o "$tool" \
  "https://github.com/AppImage/appimagetool/releases/download/$tool_version/appimagetool-$arch.AppImage"
[ "$(sha256sum "$tool" | cut -d ' ' -f 1)" = "$tool_sha" ] || fail "appimagetool does not match its digest"
chmod +x "$tool"

app="$work/Duskcode.AppDir"
mkdir -p "$app/usr/bin" "$app/usr/share/duskcode" "$app/usr/share/applications" \
  "$app/usr/share/icons/hicolor/512x512/apps"
install -m 755 "$binary" "$app/usr/bin/duskcode-gui"
cp -R "$here/resources" "$app/usr/share/duskcode/resources"
printf '%s\n' "$version" > "$app/usr/share/duskcode/resources/VERSION"

cat > "$app/duskcode.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=Duskcode
Comment=An agent system for code
Exec=duskcode-gui
Icon=duskcode
Categories=Development;
StartupWMClass=duskcode
Terminal=false
DESKTOP
cp "$app/duskcode.desktop" "$app/usr/share/applications/duskcode.desktop"

# The picture is 1024 px; AppImage desktops take 512.
icon="$here/projects/clients/desktop/assets/icon.png"
if command -v convert >/dev/null 2>&1; then
  convert "$icon" -resize 512x512 "$app/duskcode.png"
else
  cp "$icon" "$app/duskcode.png"
fi
cp "$app/duskcode.png" "$app/usr/share/icons/hicolor/512x512/apps/duskcode.png"

cat > "$app/AppRun" <<'RUN'
#!/bin/sh
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/duskcode-gui" "$@"
RUN
chmod +x "$app/AppRun"

mkdir -p "$out"
image="$out/Duskcode-$version-linux-$arch.AppImage"
rm -f "$image"
# No FUSE is needed to build: the tool unpacks itself and runs.
ARCH="$arch" APPIMAGE_EXTRACT_AND_RUN=1 "$tool" --no-appstream "$app" "$image" >&2
printf '%s\n' "$image"
