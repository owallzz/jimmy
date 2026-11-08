#!/usr/bin/env bash
# Build a release binary and package it as an RPM. Run on Fedora (or in a Fedora container).
#   packaging/build-rpm.sh [OUT_DIR]
# Needs: cargo, gcc, mpv-devel, systemd-devel, rpm-build.
set -euo pipefail

SRC="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$SRC/dist}"
VERSION="$(grep -m1 '^version' "$SRC/Cargo.toml" | cut -d'"' -f2)"
# Release = commit count, so every new build upgrades cleanly with `dnf install`.
RELEASE="$(git -C "$SRC" rev-list --count HEAD 2>/dev/null || echo 1)"
TARGET_DIR="${CARGO_TARGET_DIR:-$SRC/target}"
TOPDIR="$(mktemp -d)"
trap 'rm -rf "$TOPDIR"' EXIT

echo "==> cargo build --release ($(uname -m))"
CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release --locked --manifest-path "$SRC/Cargo.toml"

echo "==> rpmbuild jimmy $VERSION-$RELEASE"
rpmbuild -bb "$SRC/packaging/jimmy.spec" \
    --define "_topdir $TOPDIR" \
    --define "jimmy_version $VERSION" \
    --define "jimmy_release $RELEASE" \
    --define "jimmy_src $SRC" \
    --define "jimmy_bin $TARGET_DIR/release/jimmy" --quiet

mkdir -p "$OUT"
cp "$TOPDIR"/RPMS/*/jimmy-"$VERSION"-*.rpm "$OUT"/
ls -1 "$OUT"/jimmy-"$VERSION"-*.rpm
