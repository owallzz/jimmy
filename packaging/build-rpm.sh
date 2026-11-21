#!/usr/bin/env bash
# Package jimmy as an RPM. Builds a release binary with cargo first, unless JIMMY_BIN points at
# one that's already built (the release workflow packages its portable AlmaLinux 9 binary).
#   packaging/build-rpm.sh [OUT_DIR]
# Needs: rpm-build; to compile also cargo, gcc, systemd-devel, and mpv-devel unless
# JIMMY_MPV_PREFIX is set.
#
# JIMMY_BIN         package this binary as is (no cargo, no dist tag: one RPM for every
#                   Fedora/EL9+ release)
# JIMMY_MPV_PREFIX  link the static mpv from packaging/build-slim-mpv.sh instead of system libmpv
# JIMMY_VARIANT     suffix for the release field, e.g. native (default: none)
# JIMMY_TARBALL=1   also write jimmy-<ver>-<variant>-linux-<arch>.tar.gz (old release format)
set -euo pipefail

SRC="$(cd "$(dirname "$0")/.." && pwd)"
OUT="${1:-$SRC/dist}"
VERSION="$(grep -m1 '^version' "$SRC/Cargo.toml" | cut -d'"' -f2)"
# Release = commit count, so every new build upgrades cleanly with `dnf install`.
COUNT="$(git -C "$SRC" rev-list --count HEAD 2>/dev/null || echo 1)"
VARIANT="${JIMMY_VARIANT:-}"
# RPM release field can't contain '-'.
RELEASE="$COUNT${VARIANT:+.${VARIANT//-/_}}"
ARCH="$(uname -m)"
TOPDIR="$(mktemp -d)"
trap 'rm -rf "$TOPDIR"' EXIT

dist_args=()
if [ -n "${JIMMY_BIN:-}" ]; then
    BIN="$(realpath "$JIMMY_BIN")"
    dist_args=(--define "dist %{nil}")
else
    TARGET_DIR="${CARGO_TARGET_DIR:-$SRC/target}"
    echo "==> cargo build --release ($ARCH, mpv: ${JIMMY_MPV_PREFIX:-system libmpv})"
    CARGO_TARGET_DIR="$TARGET_DIR" cargo build --release --locked --manifest-path "$SRC/Cargo.toml"
    BIN="$TARGET_DIR/release/jimmy"
fi
# Linked against the system libmpv, or mpv/FFmpeg built in (sets License and Requires).
if readelf -d "$BIN" | grep -q 'NEEDED.*libmpv\.so'; then SYSTEM_MPV=1; else SYSTEM_MPV=0; fi

echo "==> rpmbuild jimmy $VERSION-$RELEASE (system mpv: $SYSTEM_MPV)"
rpmbuild -bb "$SRC/packaging/jimmy.spec" \
    --define "_topdir $TOPDIR" \
    --define "jimmy_version $VERSION" \
    --define "jimmy_release $RELEASE" \
    --define "jimmy_system_mpv $SYSTEM_MPV" \
    --define "jimmy_src $SRC" \
    --define "jimmy_bin $BIN" "${dist_args[@]}" --quiet

mkdir -p "$OUT"
cp "$TOPDIR"/RPMS/*/jimmy-"$VERSION-$RELEASE".*.rpm "$OUT"/
ls -1 "$OUT"/jimmy-"$VERSION-$RELEASE".*.rpm

if [ "${JIMMY_TARBALL:-0}" = 1 ]; then
    if [ "$SYSTEM_MPV" = 1 ]; then kind=dynamic; else kind=static; fi
    NAME="jimmy-$VERSION-${VARIANT:-$kind}-linux-$ARCH"
    mkdir -p "$TOPDIR/$NAME"
    cp "$BIN" "$SRC/README.md" "$SRC/LICENSE" "$TOPDIR/$NAME/"
    cp "$SRC/assets/jimmy.desktop" "$SRC/assets/jimmy.svg" "$TOPDIR/$NAME/"
    tar -C "$TOPDIR" -czf "$OUT/$NAME.tar.gz" "$NAME"
    ls -1 "$OUT/$NAME.tar.gz"
fi
