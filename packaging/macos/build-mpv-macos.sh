#!/usr/bin/env bash
# Build a static freetype + fribidi + harfbuzz + libass + dav1d + FFmpeg + libplacebo + libmpv for
# jimmy on macOS, for one architecture.
#
#   TARGET_ARCH=arm64  packaging/macos/build-mpv-macos.sh [PREFIX]   # default: ~/.cache/jimmy-mpv-macos-<arch>
#   TARGET_ARCH=x86_64 packaging/macos/build-mpv-macos.sh [PREFIX]
#
# Same codec choices as the full profile of packaging/build-slim-mpv.sh: every FFmpeg decoder,
# demuxer and parser; no encoders, muxers, devices or network. macOS specifics: VideoToolbox and
# AudioToolbox in FFmpeg; mpv with OpenGL (render API only), CoreAudio and libass (CoreText).
# Cocoa, gl-cocoa, videotoolbox-gl and Swift stay off: in mpv 0.42 cocoa needs Swift, so hwdec is
# videotoolbox-copy. TARGET_ARCH=x86_64 on an Apple Silicon Mac cross-compiles (needs Rosetta 2
# for meson's sanity checks: softwareupdate --install-rosetta --agree-to-license).
#
# Minimum macOS is 11.0. x86_64 CPU baseline is x86-64-v2 unless CPU_FLAGS is set.
#
# Build deps: Xcode command line tools, then brew install meson ninja nasm pkgconf
# Then build jimmy with JIMMY_MPV_PREFIX=PREFIX (packaging/macos/make-app.sh does both arches).
set -euo pipefail

[ "$(uname -s)" = Darwin ] || { echo "macOS only" >&2; exit 1; }
TARGET_ARCH="${TARGET_ARCH:-$(uname -m)}"
case "$TARGET_ARCH" in
    arm64) CPU_FLAGS="${CPU_FLAGS:-}" ;;
    x86_64) CPU_FLAGS="${CPU_FLAGS:--march=x86-64-v2 -mtune=generic}" ;;
    *) echo "TARGET_ARCH must be arm64 or x86_64" >&2; exit 1 ;;
esac
PREFIX="${1:-$HOME/.cache/jimmy-mpv-macos-$TARGET_ARCH}"
SRC="$PREFIX/src"
FREETYPE=2.15.0
FRIBIDI=1.0.18
HARFBUZZ=15.2.0
LIBASS=0.17.5
DAV1D=1.5.4
FFMPEG=8.1.3
PLACEBO=v7.387.0
MPV=v0.42.0
JOBS="$(sysctl -n hw.ncpu)"
STAGES="${STAGES:-freetype fribidi harfbuzz libass dav1d ffmpeg placebo mpv}"

mkdir -p "$SRC"
export MACOSX_DEPLOYMENT_TARGET=11.0
# Only our prefix: never pick up Homebrew's .pc files (and its dylibs with them).
export PKG_CONFIG_LIBDIR="$PREFIX/lib/pkgconfig"
unset PKG_CONFIG_PATH
export CFLAGS="-O2 $CPU_FLAGS"
export CXXFLAGS="-O2 $CPU_FLAGS"

# Cross-compiling x86_64 on arm64: a meson cross file and FFmpeg's cross options.
MESON_X=()
FF_X=(--cc=clang --cxx=clang++)
if [ "$TARGET_ARCH" != "$(uname -m)" ]; then
    cat > "$PREFIX/cross.ini" <<EOF
[binaries]
c = ['clang', '-arch', '$TARGET_ARCH']
cpp = ['clang++', '-arch', '$TARGET_ARCH']
objc = ['clang', '-arch', '$TARGET_ARCH']
objcpp = ['clang++', '-arch', '$TARGET_ARCH']
ar = 'ar'
strip = 'strip'
nasm = 'nasm'
pkg-config = 'pkg-config'

[properties]
pkg_config_libdir = ['$PREFIX/lib/pkgconfig']
needs_exe_wrapper = false

[host_machine]
system = 'darwin'
subsystem = 'macos'
kernel = 'xnu'
cpu_family = '$TARGET_ARCH'
cpu = '$TARGET_ARCH'
endian = 'little'
EOF
    MESON_X=(--cross-file "$PREFIX/cross.ini")
    FF_X=(--enable-cross-compile --target-os=darwin --arch="$TARGET_ARCH" --host-cc=clang
          --cc="clang -arch $TARGET_ARCH" --cxx="clang++ -arch $TARGET_ARCH"
          --objcc="clang -arch $TARGET_ARCH" --extra-ldflags="-arch $TARGET_ARCH")
fi

# ${a[@]+"${a[@]}"}: macOS bash 3.2 treats an empty array as unbound under set -u.
msetup() {
    rm -rf build
    meson setup build ${MESON_X[@]+"${MESON_X[@]}"} --prefix="$PREFIX" --libdir=lib --buildtype=release \
        -Ddefault_library=static "$@"
    ninja -C build install
}

# fetch URL SHA256: download and unpack a release tarball into $SRC (once).
fetch() {
    local f="$SRC/${1##*/}"
    [ -f "$f" ] || curl -fsSL --retry 3 -o "$f" "$1"
    echo "$2  $f" | shasum -a 256 -c - >/dev/null || { echo "checksum mismatch: $f" >&2; rm -f "$f"; exit 1; }
    tar xf "$f" -C "$SRC"
}

build_freetype() {
    [ -d "$SRC/freetype-$FREETYPE" ] || fetch "https://download.savannah.gnu.org/releases/freetype/freetype-$FREETYPE.tar.xz" \
        93cd47eb719c325107f2d3cf087d06299e944c3b48a40b55537302304dea1dd5
    cd "$SRC/freetype-$FREETYPE"
    msetup --auto-features=disabled -Dmmap=enabled -Dzlib=disabled -Dharfbuzz=disabled \
        -Dpng=disabled -Dbrotli=disabled -Dbzip2=disabled -Dtests=disabled
}

build_fribidi() {
    [ -d "$SRC/fribidi-$FRIBIDI" ] || fetch "https://github.com/fribidi/fribidi/releases/download/v$FRIBIDI/fribidi-$FRIBIDI.tar.xz" \
        1de5e8073d035388b71d0b2d452b1da1f8caeea8a7702997b401fe1b6250727f
    cd "$SRC/fribidi-$FRIBIDI"
    msetup -Ddocs=false -Dbin=false -Dtests=false
}

build_harfbuzz() {
    [ -d "$SRC/harfbuzz-$HARFBUZZ" ] || fetch "https://github.com/harfbuzz/harfbuzz/releases/download/$HARFBUZZ/harfbuzz-$HARFBUZZ.tar.xz" \
        e2ec49090744383cdbe7df808c1cedaafba06a97be8dc4211fe546ba585f12cb
    cd "$SRC/harfbuzz-$HARFBUZZ"
    msetup --auto-features=disabled -Dfreetype=enabled -Draster=disabled -Dvector=disabled \
        -Dgpu=disabled -Dsubset=disabled -Dtests=disabled -Dutilities=disabled -Ddocs=disabled \
        -Dintrospection=disabled
}

build_libass() {
    [ -d "$SRC/libass-$LIBASS" ] || fetch "https://github.com/libass/libass/releases/download/$LIBASS/libass-$LIBASS.tar.xz" \
        2dca25c0e0c837ddf00b52011b3f82cac1e4ddd3ad018227806b0c2288864acc
    cd "$SRC/libass-$LIBASS"
    msetup -Dfontconfig=disabled -Dcoretext=enabled -Ddirectwrite=disabled -Dlibunibreak=disabled \
        -Dasm=enabled -Dtest=disabled -Dcompare=disabled -Dprofile=disabled -Dfuzz=disabled \
        -Dcheckasm=disabled -Drequire-system-font-provider=true
}

build_dav1d() {
    cd "$SRC"
    [ -d "dav1d-$DAV1D" ] || git clone -q --depth 1 -b "$DAV1D" https://code.videolan.org/videolan/dav1d.git "dav1d-$DAV1D"
    cd "dav1d-$DAV1D"
    msetup -Denable_tools=false -Denable_tests=false -Denable_examples=false
}

build_ffmpeg() {
    [ -d "$SRC/ffmpeg-$FFMPEG" ] || fetch "https://ffmpeg.org/releases/ffmpeg-$FFMPEG.tar.xz" \
        7138d28c96d9d3e3af4ee3d8cad72741f8ffb40da90c1112235dea3ecd3178a3
    cd "$SRC/ffmpeg-$FFMPEG"
    make distclean >/dev/null 2>&1 || true
    ./configure --prefix="$PREFIX" --libdir="$PREFIX/lib" "${FF_X[@]}" \
        --pkg-config=pkg-config --pkg-config-flags=--static \
        --enable-static --disable-shared --enable-pic \
        --disable-programs --disable-doc --disable-debug \
        --disable-autodetect --enable-videotoolbox --enable-audiotoolbox \
        --enable-zlib --enable-iconv --enable-libdav1d \
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
    msetup -Dvulkan=disabled -Dopengl=enabled -Dd3d11=disabled \
        -Dglslang=disabled -Dshaderc=disabled -Dlcms=disabled -Ddovi=disabled -Dlibdovi=disabled \
        -Dxxhash=disabled -Dunwind=disabled -Ddemos=false -Dtests=false
}

build_mpv() {
    [ -d "$SRC/mpv-${MPV#v}" ] || fetch "https://github.com/mpv-player/mpv/archive/refs/tags/$MPV.tar.gz" \
        bcfebbfa2b65a6d88dc97ea18117831072373e38b5785a2afb874ed4df5070dd
    cd "$SRC/mpv-${MPV#v}"
    # The coreaudio AO needs cfstr_* from utils-mac.c, which meson only builds with cocoa.
    grep -q "'osdep/utils-mac.c')" meson.build ||
        sed -i '' "s|'audio/out/ao_coreaudio_utils.c')|'audio/out/ao_coreaudio_utils.c', 'osdep/utils-mac.c')|" meson.build
    grep -q "'osdep/utils-mac.c')" meson.build || { echo "error: utils-mac.c patch failed" >&2; exit 1; }
    # The SDK's zlib as plain -lz. Without a .pc, meson may ask CMake, which returns the
    # absolute libz.tbd path of this Xcode and bakes it into mpv.pc.
    printf 'Name: zlib\nDescription: zlib from the macOS SDK\nVersion: 1.2.12\nLibs: -lz\n' \
        > "$PREFIX/lib/pkgconfig/zlib.pc"
    # Everything optional off (lua is a choice option, so it needs its own switch), then only what
    # jimmy needs. cocoa without swift doesn't build in 0.42, and cplayer needs swift.
    msetup -Dlibmpv=true -Dcplayer=false --auto-features=disabled -Dlua=disabled \
        -Dgl=enabled -Dplain-gl=enabled -Dcoreaudio=enabled \
        -Dcocoa=disabled -Dgl-cocoa=disabled -Dvideotoolbox-gl=disabled -Dswift-build=disabled \
        -Dzlib=enabled -Diconv=enabled
}

for s in $STAGES; do
    echo "==> $s ($TARGET_ARCH)"
    "build_$s"
done

# Nothing may point at Homebrew or at this machine's Xcode (the prefix is cached across images).
if grep -lE '/opt/homebrew|/usr/local/(lib|include|opt|Cellar)|/Applications/Xcode|/Library/Developer' "$PREFIX"/lib/pkgconfig/*.pc; then
    echo "error: .pc files above reference Homebrew or an Xcode path" >&2
    exit 1
fi
if [ -f "$PREFIX/lib/libmpv.a" ] && ! nm -g "$PREFIX/lib/libmpv.a" 2>/dev/null | grep ' T _cfstr_get_cstr$' >/dev/null; then
    echo "error: libmpv.a lacks cfstr_get_cstr (utils-mac.c not built)" >&2
    exit 1
fi
for a in "$PREFIX"/lib/*.a; do
    lipo "$a" -verify_arch "$TARGET_ARCH" || { echo "error: $a is not $TARGET_ARCH" >&2; exit 1; }
done
echo "done: $PREFIX"
