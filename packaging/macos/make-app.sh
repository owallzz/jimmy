#!/usr/bin/env bash
# Build jimmy for arm64 and x86_64 against the static mpv prefixes from build-mpv-macos.sh, glue
# them into one universal binary and wrap it in dist/jimmy.app (ad-hoc signed).
#
#   packaging/macos/make-app.sh
#
#   JIMMY_MPV_PREFIX_ARM64   default ~/.cache/jimmy-mpv-macos-arm64
#   JIMMY_MPV_PREFIX_X86_64  default ~/.cache/jimmy-mpv-macos-x86_64
#   DIST                     output directory, default dist
#
# Needs: rustup target add aarch64-apple-darwin x86_64-apple-darwin; brew install librsvg (icon).
# Release builds also set CARGO_PROFILE_RELEASE_* (see .github/workflows/release.yml).
set -euo pipefail

cd "$(dirname "$0")/../.."
V="$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)"
DIST="${DIST:-dist}"
T="${CARGO_TARGET_DIR:-target}"
APP="$DIST/jimmy.app"
export MACOSX_DEPLOYMENT_TARGET=11.0

# Only /usr/lib and /System may be linked dynamically, and the minimum macOS must be 11.0.
check() {
    local bin="$1" arch="$2" bad minos
    bad="$(otool -L -arch "$arch" "$bin" | awk 'NR > 1 { print $1 }' | grep -vE '^(/usr/lib/|/System/Library/)' || true)"
    if [ -n "$bad" ]; then
        echo "error: $bin ($arch) links outside the SDK:" >&2
        echo "$bad" >&2
        exit 1
    fi
    minos="$(otool -l -arch "$arch" "$bin" | awk '$1 == "minos" { print $2; exit }')"
    [ "$minos" = 11.0 ] || { echo "error: $bin ($arch) minos is '$minos', want 11.0" >&2; exit 1; }
}

# build TRIPLE ARCH PREFIX CPU
build() {
    [ -f "$3/lib/libmpv.a" ] || { echo "no static mpv in $3 (run build-mpv-macos.sh)" >&2; exit 1; }
    # RUSTFLAGS replaces .cargo/config.toml's Homebrew -L paths, so nothing resolves to Homebrew.
    JIMMY_MPV_PREFIX="$3" RUSTFLAGS="-C target-cpu=$4" \
        cargo build --release --locked --target "$1"
    check "$T/$1/release/jimmy" "$2"
}

build aarch64-apple-darwin arm64 "${JIMMY_MPV_PREFIX_ARM64:-$HOME/.cache/jimmy-mpv-macos-arm64}" apple-m1
build x86_64-apple-darwin x86_64 "${JIMMY_MPV_PREFIX_X86_64:-$HOME/.cache/jimmy-mpv-macos-x86_64}" x86-64-v2

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
lipo -create -output "$APP/Contents/MacOS/jimmy" \
    "$T/aarch64-apple-darwin/release/jimmy" "$T/x86_64-apple-darwin/release/jimmy"
lipo "$APP/Contents/MacOS/jimmy" -verify_arch arm64 x86_64
sed "s/@VERSION@/$V/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
plutil -lint "$APP/Contents/Info.plist"
printf 'APPL????' > "$APP/Contents/PkgInfo"

# Icon: render the SVG at every size iconutil wants.
iconset="$(mktemp -d)/jimmy.iconset"
mkdir -p "$iconset"
for s in 16 32 128 256 512; do
    rsvg-convert -w "$s" -h "$s" -o "$iconset/icon_${s}x${s}.png" assets/jimmy.svg
    rsvg-convert -w $((s * 2)) -h $((s * 2)) -o "$iconset/icon_${s}x${s}@2x.png" assets/jimmy.svg
done
iconutil -c icns -o "$APP/Contents/Resources/jimmy.icns" "$iconset"
cp LICENSE THIRD-PARTY.md "$APP/Contents/Resources/"
rm -rf "$(dirname "$iconset")"

# Ad-hoc signature (no Apple Developer ID): seals the bundle so Gatekeeper offers "Open Anyway"
# instead of calling it damaged.
codesign --force --sign - --timestamp=none "$APP"
codesign --verify --strict --verbose=2 "$APP"

for a in arm64 x86_64; do check "$APP/Contents/MacOS/jimmy" "$a"; done
lipo -info "$APP/Contents/MacOS/jimmy"
# Smoke test each slice that this Mac can run (x86_64 needs Rosetta on Apple Silicon).
for a in arm64 x86_64; do
    if arch "-$a" /usr/bin/true 2>/dev/null; then
        arch "-$a" "$APP/Contents/MacOS/jimmy" --version
    else
        echo "(can't run $a here, skipped)"
    fi
done
echo "done: $APP ($V)"
