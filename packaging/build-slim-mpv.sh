#!/usr/bin/env bash
# Build a slim, static FFmpeg + libplacebo + libmpv for jimmy (Linux).
#
#   packaging/build-slim-mpv.sh [PREFIX]        # default: ~/.cache/jimmy-mpv
#
# Then build jimmy with JIMMY_MPV_PREFIX=PREFIX and it links this mpv statically instead of
# the system libmpv. FFmpeg keeps every decoder, demuxer and parser (play anything) but no
# encoders, muxers, devices or network; mpv keeps OpenGL/EGL output, VA-API, PipeWire/ALSA and
# libass, and drops Lua, JavaScript, Blu-ray/DVD, archives and other extras.
#
# Build deps (Fedora): meson ninja-build nasm gcc-c++ python3-jinja2 libass-devel libva-devel
#   libdrm-devel pipewire-devel alsa-lib-devel mesa-libEGL-devel zlib-ng-compat-devel
set -euo pipefail

PREFIX="${1:-$HOME/.cache/jimmy-mpv}"
SRC="$PREFIX/src"
FFMPEG=8.1.3
PLACEBO=v7.387.0
MPV=v0.42.0
JOBS="$(nproc)"
STAGES="${STAGES:-ffmpeg placebo mpv}"

mkdir -p "$SRC"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
export CFLAGS="-O2 -fPIC"
export CXXFLAGS="-O2 -fPIC"

build_ffmpeg() {
    cd "$SRC"
    [ -d "ffmpeg-$FFMPEG" ] || curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$FFMPEG.tar.xz" | tar xJ
    cd "ffmpeg-$FFMPEG"
    ./configure --prefix="$PREFIX" --libdir="$PREFIX/lib" \
        --enable-static --disable-shared --enable-pic \
        --disable-programs --disable-doc --disable-debug \
        --disable-autodetect --enable-vaapi --enable-zlib --enable-iconv \
        --disable-encoders --disable-muxers --disable-devices --disable-network \
        --disable-protocols --enable-protocol=file,pipe,fd \
        --disable-filters --enable-filter=aformat,anull,aresample,format,null,scale,crop,pad,hflip,vflip,transpose,setpts,asetpts,volume,yadif
    make -j"$JOBS"
    make install
}

build_placebo() {
    cd "$SRC"
    [ -d "libplacebo" ] || git clone -q --depth 1 --recursive -b "$PLACEBO" https://code.videolan.org/videolan/libplacebo.git
    cd libplacebo
    rm -rf build
    meson setup build --prefix="$PREFIX" --libdir=lib --buildtype=release -Ddefault_library=static \
        -Dvulkan=disabled -Dopengl=enabled -Dd3d11=disabled \
        -Dglslang=disabled -Dshaderc=disabled -Dlcms=disabled -Ddovi=disabled -Dlibdovi=disabled \
        -Dxxhash=disabled -Dunwind=disabled -Ddemos=false -Dtests=false
    ninja -C build install
}

build_mpv() {
    cd "$SRC"
    [ -d "mpv-${MPV#v}" ] || curl -fsSL "https://github.com/mpv-player/mpv/archive/refs/tags/$MPV.tar.gz" | tar xz
    cd "mpv-${MPV#v}"
    rm -rf build
    # Everything optional off (lua is a choice option, so it needs its own switch),
    # then only what jimmy needs.
    meson setup build --prefix="$PREFIX" --libdir=lib --buildtype=release -Ddefault_library=static \
        -Dlibmpv=true -Dcplayer=false --auto-features=disabled -Dlua=disabled \
        -Dgl=enabled -Degl=enabled -Dplain-gl=enabled \
        -Dvaapi=enabled -Dvaapi-drm=enabled -Ddrm=enabled \
        -Dpipewire=enabled -Dalsa=enabled -Dzlib=enabled -Diconv=enabled
    ninja -C build install
}

for s in $STAGES; do
    echo "==> $s"
    "build_$s"
done
echo "done: $PREFIX"
