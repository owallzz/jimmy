#!/usr/bin/env bash
# Build a release binary and package it as an RPM and a tarball. Run on Fedora (or in a
# Fedora container).
#   packaging/build-rpm.sh [OUT_DIR]
# Needs: cargo, gcc, systemd-devel, rpm-build, and mpv-devel unless JIMMY_MPV_PREFIX is set.
#
# JIMMY_MPV_PREFIX  link the static mpv from packaging/build-slim-mpv.sh instead of system libmpv
# JIMMY_VARIANT     name of the build in file names (default: dynamic, or static with a prefix)
set -euo pipefail

SRC="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$SRC/dist}"
VERSION="$(grep -m1 '^version' "$SRC/Cargo.toml" | cut -d'"' -f2)"
# Release = commit count, so every new build upgrades cleanly with `dnf install`.
COUNT="$(git -C "$SRC" rev-list --count HEAD 2>/dev/null || echo 1)"
if [ -n "${JIMMY_MPV_PREFIX:-}" ]; then DEFAULT_VARIANT=static; else DEFAULT_VARIANT=dynamic; fi
VARIANT="${JIMMY_VARIANT:-$DEFAULT_VARIANT}"
# RPM release field can't contain '-'.
RELEASE="$COUNT.${VARIANT//-/_}"
TARGET_DIR="${CARGO_TARGET_DIR:-$SRC/target}"
ARCH="$(uname -m)"
TOPDIR="$(mktemp -d)"
trap 'rm -rf "$TOPDIR"' EXIT

echo "==> cargo build --release ($ARCH, $VARIANT, mpv: ${JIMMY_MPV_PREFIX:-system libmpv})"
CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release --locked --manifest-path "$SRC/Cargo.toml"
BIN="$TARGET_DIR/release/jimmy"

echo "==> rpmbuild jimmy $VERSION-$RELEASE"
rpmbuild -bb "$SRC/packaging/jimmy.spec" \
    --define "_topdir $TOPDIR" \
    --define "jimmy_version $VERSION" \
    --define "jimmy_release $RELEASE" \
    --define "jimmy_system_mpv $([ -n "${JIMMY_MPV_PREFIX:-}" ] && echo 0 || echo 1)" \
    --define "jimmy_src $SRC" \
    --define "jimmy_bin $BIN" --quiet

mkdir -p "$OUT"
cp "$TOPDIR"/RPMS/*/jimmy-"$VERSION-$RELEASE".*.rpm "$OUT"/

echo "==> tarball"
NAME="jimmy-$VERSION-$VARIANT-linux-$ARCH"
mkdir -p "$TOPDIR/$NAME"
cp "$BIN" "$SRC/README.md" "$TOPDIR/$NAME/"
cp "$SRC/assets/jimmy.desktop" "$SRC/assets/jimmy.svg" "$TOPDIR/$NAME/"
tar -C "$TOPDIR" -czf "$OUT/$NAME.tar.gz" "$NAME"

ls -1 "$OUT"/jimmy-"$VERSION-$RELEASE".*.rpm "$OUT/$NAME.tar.gz"
