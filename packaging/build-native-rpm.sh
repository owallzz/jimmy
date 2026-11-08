#!/usr/bin/env bash
# Release RPM tuned for the machine it's built on: slim static mpv/FFmpeg (from
# build-slim-mpv.sh), -C target-cpu=native, full LTO, one codegen unit.
#   packaging/build-native-rpm.sh [OUT_DIR]
# The binary only runs on CPUs with the same features as the build machine.
set -euo pipefail
SRC="$(cd "$(dirname "$0")/.." && pwd)"
export JIMMY_VARIANT="${JIMMY_VARIANT:-native}"
export JIMMY_MPV_PREFIX="${JIMMY_MPV_PREFIX:-$HOME/.cache/jimmy-mpv}"
[ -f "$JIMMY_MPV_PREFIX/lib/libmpv.a" ] || { echo "no slim mpv in $JIMMY_MPV_PREFIX; run packaging/build-slim-mpv.sh first" >&2; exit 1; }
export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=native"
export CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_OPT_LEVEL=3
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$SRC/target-native}"
exec "$SRC/packaging/build-rpm.sh" "${1:-$SRC/dist}"
