#!/usr/bin/env bash
# Build a static libmpv for jimmy on Windows (x86_64), in an MSYS2 UCRT64 shell:
# freetype + fribidi + harfbuzz + libass, dav1d, FFmpeg, libplacebo and mpv.
#
#   packaging/windows/build-mpv-windows.sh PREFIX
#
# PREFIX must be a mixed path (C:/...), e.g. "$(cygpath -m "$RUNNER_TEMP")/jimmy-mpv": native
# meson, pkgconf and rustc don't understand /c/... paths. Then build jimmy with
# JIMMY_MPV_PREFIX=PREFIX for x86_64-pc-windows-gnu using MSYS2's own rust (UCRT, like these
# libraries) and it links everything statically: the exe needs only system DLLs.
#
# Same codec choices as the full profile of packaging/build-slim-mpv.sh: every FFmpeg decoder,
# demuxer and parser; no encoders, muxers, devices or network; no GPL parts in FFmpeg. mpv keeps
# OpenGL (WGL) output for the render API, D3D11VA/DXVA2 decoding, WASAPI and libass (fonts
# through DirectWrite) and drops Lua, JavaScript, Blu-ray/DVD, archives and other extras.
# The Direct3D 11 video output is left out: it needs shaderc + SPIRV-Cross and jimmy renders
# through OpenGL anyway.
#
# CPU baseline is x86-64-v2 (SSE4.2/POPCNT) unless CPU_FLAGS is set.
#
# MSYS2 packages: git curl tar xz make diffutils and mingw-w64-ucrt-x86_64-{gcc,meson,ninja,
#   nasm,pkgconf,python-jinja,zlib,libiconv}. Don't install MSYS2's ffmpeg, mpv, libass,
#   libplacebo, harfbuzz or freetype: their .pc files and import libraries would be used
#   instead of ours.
#
# Cross build from Linux (to test this script, e.g. Fedora's mingw64-gcc): set MINGW_PREFIX to
# the mingw sysroot, CROSS_FILE to a meson cross file and CROSS_PREFIX=x86_64-w64-mingw32-.
set -euo pipefail

PREFIX="${1:?usage: build-mpv-windows.sh PREFIX (a C:/... path)}"
SRC="$PREFIX/src"
FREETYPE=2.15.0
FRIBIDI=1.0.18
HARFBUZZ=15.2.0
LIBASS=0.17.5
DAV1D=1.5.4
FFMPEG=8.1.3
PLACEBO=v7.387.0
MPV=v0.42.0
JOBS="$(nproc)"
STAGES="${STAGES:-syslibs freetype fribidi harfbuzz libass dav1d ffmpeg placebo mpv}"
CPU_FLAGS="${CPU_FLAGS:--march=x86-64-v2 -mtune=generic}"

mkdir -p "$SRC" "$PREFIX/lib"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
export CFLAGS="-O2 $CPU_FLAGS"
export CXXFLAGS="-O2 $CPU_FLAGS"

# --wrap-mode=nofallback: use our libraries, never a wrap download.
MESON=(--prefix="$PREFIX" --libdir=lib --buildtype=release -Ddefault_library=static --wrap-mode=nofallback)
if [ -n "${CROSS_FILE:-}" ]; then MESON+=(--cross-file="$CROSS_FILE"); fi

fetch() { # url dir: download and unpack a tarball once
    [ -d "$SRC/$2" ] || curl -fsSL "$1" | tar xJ -C "$SRC"
    cd "$SRC/$2"
}

meson_build() {
    rm -rf build
    meson setup build "${MESON[@]}" "$@"
    ninja -C build install
}

# zlib and iconv come from MSYS2. Copy the static archives into the prefix so jimmy's
# build.rs links them statically instead of picking zlib1.dll / libiconv-2.dll.
build_syslibs() {
    cp "${MINGW_PREFIX:?run this in an MSYS2 UCRT64 shell}"/lib/{libz,libiconv,libcharset}.a "$PREFIX/lib/"
}

build_freetype() {
    fetch "https://download.savannah.gnu.org/releases/freetype/freetype-$FREETYPE.tar.xz" "freetype-$FREETYPE"
    meson_build -Dzlib=internal -Dbzip2=disabled -Dpng=disabled -Dbrotli=disabled \
        -Dharfbuzz=disabled -Dtests=disabled
}

build_fribidi() {
    fetch "https://github.com/fribidi/fribidi/releases/download/v$FRIBIDI/fribidi-$FRIBIDI.tar.xz" "fribidi-$FRIBIDI"
    meson_build -Ddocs=false -Dbin=false -Dtests=false -Ddeprecated=false
}

build_harfbuzz() {
    fetch "https://github.com/harfbuzz/harfbuzz/releases/download/$HARFBUZZ/harfbuzz-$HARFBUZZ.tar.xz" "harfbuzz-$HARFBUZZ"
    meson_build --auto-features=disabled -Dfreetype=enabled -Draster=disabled -Dvector=disabled \
        -Dgpu=disabled -Dsubset=disabled -Dtests=disabled -Dutilities=disabled
}

build_libass() {
    fetch "https://github.com/libass/libass/releases/download/$LIBASS/libass-$LIBASS.tar.xz" "libass-$LIBASS"
    meson_build --auto-features=disabled -Ddirectwrite=enabled -Dasm=enabled
}

build_dav1d() {
    cd "$SRC"
    [ -d "dav1d-$DAV1D" ] || git clone -q --depth 1 -b "$DAV1D" https://code.videolan.org/videolan/dav1d.git "dav1d-$DAV1D"
    cd "dav1d-$DAV1D"
    meson_build -Denable_tools=false -Denable_tests=false -Denable_examples=false
}

build_ffmpeg() {
    fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG.tar.xz" "ffmpeg-$FFMPEG"
    local cross=()
    if [ -n "${CROSS_PREFIX:-}" ]; then
        cross=(--enable-cross-compile --target-os=mingw32 --arch=x86_64 --cross-prefix="$CROSS_PREFIX")
    fi
    make distclean >/dev/null 2>&1 || true
    ./configure --prefix="$PREFIX" --libdir="$PREFIX/lib" "${cross[@]}" \
        --pkg-config-flags=--static \
        --enable-static --disable-shared \
        --disable-programs --disable-doc --disable-debug \
        --disable-autodetect --enable-d3d11va --enable-dxva2 --enable-zlib --enable-iconv --enable-libdav1d \
        --disable-encoders --disable-muxers --disable-devices --disable-network \
        --disable-protocols --enable-protocol=file,pipe,fd \
        --disable-filters --enable-filter=aformat,anull,aresample,format,null,scale,crop,pad,hflip,vflip,transpose,setpts,asetpts,volume,yadif
    # Threads aren't covered by --disable-autodetect; make sure we got native ones, not winpthreads.
    grep -q '^#define HAVE_W32THREADS 1' config.h || { echo "FFmpeg: no win32 threads" >&2; exit 1; }
    make -j"$JOBS"
    make install
}

build_placebo() {
    cd "$SRC"
    [ -d "libplacebo" ] || git clone -q --depth 1 --recursive -b "$PLACEBO" https://code.videolan.org/videolan/libplacebo.git
    cd libplacebo
    meson_build -Dvulkan=disabled -Dopengl=enabled -Dd3d11=disabled \
        -Dglslang=disabled -Dshaderc=disabled -Dlcms=disabled -Ddovi=disabled -Dlibdovi=disabled \
        -Dxxhash=disabled -Dunwind=disabled -Ddemos=false -Dtests=false
}

build_mpv() {
    cd "$SRC"
    [ -d "mpv-${MPV#v}" ] || curl -fsSL "https://github.com/mpv-player/mpv/archive/refs/tags/$MPV.tar.gz" | tar xz
    cd "mpv-${MPV#v}"
    # vf_d3d11vpp.c (built with d3d-hwaccel) needs d3d11_helpers.c, which mpv only builds with
    # the d3d11 VO: undefined d3d11_get_csp_name at link time, and a DXGI_DEBUG_D3D11 redefinition.
    sed -i "s/if features\['d3d11'\] or features\['egl-angle-win32'\] or (/if features['d3d11'] or features['egl-angle-win32'] or get_option('d3d-hwaccel').enabled() or (/" meson.build
    grep -q "or get_option('d3d-hwaccel').enabled() or (" meson.build
    # Everything optional off (lua is a choice option, so it needs its own switch), then only
    # what jimmy needs. gl-win32 is what links opengl32; dxinterop gives DXVA2 zero-copy when
    # the driver has WGL_NV_DX_interop, otherwise the -copy hwdecs are used.
    meson_build -Dlibmpv=true -Dcplayer=false --auto-features=disabled -Dlua=disabled \
        -Dwin32-threads=enabled -Dgl=enabled -Dplain-gl=enabled -Dgl-win32=enabled -Dgl-dxinterop=enabled \
        -Dd3d-hwaccel=enabled -Dd3d9-hwaccel=enabled -Dgl-dxinterop-d3d9=enabled \
        -Dwasapi=enabled -Dzlib=enabled -Diconv=enabled
}

for s in $STAGES; do
    echo "==> $s"
    "build_$s"
done
# What jimmy's build.rs will link (fails here if a .pc file is missing).
"${PKG_CONFIG:-pkg-config}" --static --libs mpv
echo "done: $PREFIX"
