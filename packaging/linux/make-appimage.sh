#!/usr/bin/env bash
# Wrap the portable binary in an AppImage (no bundled libraries; same requirements as the
# standalone binary). Needs curl; runs on any x86_64 Linux, FUSE not required.
#   packaging/linux/make-appimage.sh BIN [OUT_DIR]   # → OUT_DIR/jimmy-<ver>-x86_64.AppImage
set -euo pipefail
SRC="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="$1"
OUT="${2:-$SRC/dist}"
VERSION="$(grep -m1 '^version' "$SRC/Cargo.toml" | cut -d'"' -f2)"
ID=io.github.owallzz.jimmy

# Pinned releases (the "continuous" assets change under the same URL).
TOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
TOOL_SHA=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
RUNTIME_URL=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
RUNTIME_SHA=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d
TOOLS="${APPIMAGE_TOOLS:-$HOME/.cache/jimmy-appimage}"
fetch() {
    [ -f "$TOOLS/$3" ] && echo "$2  $TOOLS/$3" | sha256sum -c --quiet && return
    curl -fsSL -o "$TOOLS/$3" "$1"
    echo "$2  $TOOLS/$3" | sha256sum -c --quiet
}
mkdir -p "$TOOLS"
fetch "$TOOL_URL" "$TOOL_SHA" appimagetool
fetch "$RUNTIME_URL" "$RUNTIME_SHA" runtime-x86_64
chmod +x "$TOOLS/appimagetool"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
APP="$WORK/AppDir"
install -Dm755 "$SRC/packaging/linux/AppRun" "$APP/AppRun"
install -Dm755 "$BIN" "$APP/usr/bin/jimmy"
install -Dm644 "$SRC/assets/jimmy.desktop" "$APP/usr/share/applications/$ID.desktop"
install -Dm644 "$SRC/assets/jimmy.svg" "$APP/usr/share/icons/hicolor/scalable/apps/jimmy.svg"
sed "s|<release version=\"[^\"]*\" date=\"[^\"]*\"/>|<release version=\"$VERSION\" date=\"$(date -u +%F)\"/>|" \
    "$SRC/packaging/linux/$ID.metainfo.xml" > "$WORK/$ID.metainfo.xml"
install -Dm644 "$WORK/$ID.metainfo.xml" "$APP/usr/share/metainfo/$ID.metainfo.xml"
install -Dm644 "$SRC/LICENSE" "$APP/usr/share/licenses/jimmy/LICENSE"
install -Dm644 "$SRC/THIRD-PARTY.md" "$APP/usr/share/licenses/jimmy/THIRD-PARTY.md"
cp "$APP/usr/share/applications/$ID.desktop" "$APP/"
cp "$SRC/assets/jimmy.svg" "$APP/jimmy.svg"
ln -s jimmy.svg "$APP/.DirIcon"

mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
DEST="$OUT/jimmy-$VERSION-x86_64.AppImage"
ARCH=x86_64 VERSION="$VERSION" APPIMAGE_EXTRACT_AND_RUN=1 \
    "$TOOLS/appimagetool" --no-appstream --runtime-file "$TOOLS/runtime-x86_64" "$APP" "$DEST"
ls -l "$DEST"
