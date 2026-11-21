#!/usr/bin/env bash
# Package the portable binary as a single-file Flatpak bundle (nothing is compiled).
#   packaging/linux/make-flatpak.sh BIN [OUT_DIR]   # → OUT_DIR/jimmy-<ver>-x86_64.flatpak
# Needs flatpak and flatpak-builder, and the flathub remote added with --user (the runtime and
# SDK are installed from it, per user, on first use).
set -euo pipefail
SRC="$(cd "$(dirname "$0")/../.." && pwd)"
BIN="$1"
OUT="${2:-$SRC/dist}"
VERSION="$(grep -m1 '^version' "$SRC/Cargo.toml" | cut -d'"' -f2)"
ID=io.github.owallzz.jimmy

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT
cp "$SRC/packaging/linux/$ID.yml" "$BIN" "$SRC/assets/jimmy.desktop" "$SRC/assets/jimmy.svg" \
    "$SRC/LICENSE" "$SRC/THIRD-PARTY.md" "$WORK/"
[ "$(basename "$BIN")" = jimmy ] || mv "$WORK/$(basename "$BIN")" "$WORK/jimmy"
sed "s|<release version=\"[^\"]*\" date=\"[^\"]*\"/>|<release version=\"$VERSION\" date=\"$(date -u +%F)\"/>|" \
    "$SRC/packaging/linux/$ID.metainfo.xml" > "$WORK/$ID.metainfo.xml"

flatpak-builder --user --install-deps-from=flathub --disable-rofiles-fuse --force-clean \
    --state-dir="$WORK/state" --repo="$WORK/repo" "$WORK/build" "$WORK/$ID.yml"
mkdir -p "$OUT"
OUT="$(cd "$OUT" && pwd)"
DEST="$OUT/jimmy-$VERSION-x86_64.flatpak"
flatpak build-bundle --runtime-repo=https://dl.flathub.org/repo/flathub.flatpakrepo \
    "$WORK/repo" "$DEST" "$ID"
ls -l "$DEST"
