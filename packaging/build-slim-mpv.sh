#!/usr/bin/env bash
# Build a slim, static dav1d + libdisplay-info + libass + FFmpeg + libplacebo + libmpv for
# jimmy (Linux).
#
#   PROFILE=full packaging/build-slim-mpv.sh [PREFIX]   # default prefix: ~/.cache/jimmy-mpv
#   PROFILE=slim packaging/build-slim-mpv.sh [PREFIX]   # default prefix: ~/.cache/jimmy-mpv-slim
#
# Then build jimmy with JIMMY_MPV_PREFIX=PREFIX and it links this mpv statically instead of
# the system libmpv.
#   full: every FFmpeg decoder, demuxer and parser (play anything). Used for releases.
#   slim: only the codecs and containers people actually have on disk (H.264/HEVC/AV1/VP9,
#         MPEG-1/2/4, VC-1/WMV, AAC/AC-3/DTS/TrueHD/FLAC/Opus/MP3…, MKV/MP4/AVI/TS/WebM…).
# Both: no encoders, muxers, devices or network; mpv keeps OpenGL/EGL output, VA-API,
# PipeWire/PulseAudio/ALSA and libass, and drops Lua, JavaScript, Blu-ray/DVD, archives and
# other extras.
#
# libass and libdisplay-info are built in too (the Flatpak runtime and most distros lack
# libdisplay-info's soname, and libass.so.9 isn't everywhere). Their own dependencies (freetype,
# harfbuzz, fribidi, fontconfig) stay dynamic: stable sonames everywhere, and fontconfig must
# read the host's config. FFmpeg is patched to never call vaMapBuffer2 (libva >= 2.21) and mpv
# to leave out vo_drm (libdrm >= 2.4.115), so the binary still loads with the older libva/libdrm
# of Ubuntu 22.04/24.04 and Debian 12 (packaging/check-portable.sh checks for this).
#
# Audio outputs (meson feature values: enabled/disabled):
#   PIPEWIRE  default enabled; releases set it to disabled (pipewire-pulse serves the pulse API)
#   PULSE     default enabled when libpulse's pkg-config file is installed, else disabled
#
# CPU baseline is x86-64-v2 (SSE4.2/POPCNT: Nehalem/Westmere and newer) unless CPU_FLAGS is set.
#
# Build deps (Fedora): meson ninja-build nasm gcc-c++ python3-jinja2 hwdata libva-devel
#   libdrm-devel pipewire-devel pulseaudio-libs-devel alsa-lib-devel mesa-libEGL-devel
#   zlib-ng-compat-devel freetype-devel harfbuzz-devel fribidi-devel fontconfig-devel
# Build deps (AlmaLinux/RHEL 9, with EPEL + CRB): the same with zlib-devel instead of
#   zlib-ng-compat-devel, plus bzip2-devel libpng-devel brotli-devel glib2-devel graphite2-devel
#   expat-devel (pkg-config --static needs every .pc in libass's chain); meson >= 1.3 from pip.
set -euo pipefail

PROFILE="${PROFILE:-full}"
case "$PROFILE" in
    full) DEFAULT_PREFIX="$HOME/.cache/jimmy-mpv" ;;
    slim) DEFAULT_PREFIX="$HOME/.cache/jimmy-mpv-slim" ;;
    *) echo "PROFILE must be full or slim" >&2; exit 1 ;;
esac
PREFIX="${1:-$DEFAULT_PREFIX}"
SRC="$PREFIX/src"
DAV1D=1.5.4
FFMPEG=8.1.3
PLACEBO=v7.387.0
MPV=v0.42.0
DISPLAYINFO=0.3.0
LIBASS=0.17.5
JOBS="$(nproc)"
STAGES="${STAGES:-dav1d displayinfo libass ffmpeg placebo mpv}"
PIPEWIRE="${PIPEWIRE:-enabled}"
if [ -z "${PULSE:-}" ]; then
    if pkg-config --exists libpulse; then PULSE=enabled; else PULSE=disabled; fi
fi

case "$(uname -m)" in
    x86_64) CPU_FLAGS="${CPU_FLAGS:--march=x86-64-v2 -mtune=generic}" ;;
    *) CPU_FLAGS="${CPU_FLAGS:-}" ;;
esac

mkdir -p "$SRC" "$PREFIX/lib/pkgconfig"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig"
# EL9's libdrm.pc requires valgrind (for headers only); its static-only libs would end up in
# `pkg-config --static --libs mpv` and break jimmy's link. Shadow it with an empty one.
printf 'Name: valgrind\nDescription: headers only\nVersion: 3.99\nLibs:\nCflags:\n' \
    > "$PREFIX/lib/pkgconfig/valgrind.pc"
export CFLAGS="-O2 -fPIC $CPU_FLAGS"
export CXXFLAGS="-O2 -fPIC $CPU_FLAGS"

join() { local IFS=,; echo "$*"; }

# Slim profile component lists (names from `./configure --list-decoders` etc.).
SLIM_DECODERS=(
    # video
    h264 hevc av1 libdav1d vp8 vp9 mpeg1video mpeg2video mpeg4 h263
    msmpeg4v1 msmpeg4v2 msmpeg4v3 vc1 wmv1 wmv2 wmv3 theora prores mjpeg png
    # audio
    aac aac_fixed aac_latm ac3 eac3 dca truehd mlp mp1 mp2 mp3 mp1float mp2float mp3float
    flac opus vorbis alac wmav1 wmav2 wmapro wmalossless amrnb amrwb
    pcm_s8 pcm_u8 pcm_s16le pcm_s16be pcm_s24le pcm_s24be pcm_s32le pcm_s32be
    pcm_f32le pcm_f32be pcm_f64le pcm_alaw pcm_mulaw pcm_bluray pcm_dvd
    # subtitles
    ass ssa srt subrip webvtt movtext text dvdsub dvbsub pgssub ccaption microdvd
)
SLIM_DEMUXERS=(
    matroska mov avi mpegts mpegps mpegvideo flv asf ogg ivf obu
    h264 hevc m4v av1 vc1 wav w64 aiff flac mp3 aac ac3 eac3 dts truehd
    srt ass webvtt microdvd vobsub image2 png_pipe jpeg_pipe
)
SLIM_PARSERS=(
    h264 hevc av1 vp8 vp9 vp3 mpeg4video mpegvideo h263 vc1 mjpeg png
    aac aac_latm ac3 dca mlp mpegaudio flac opus vorbis dvdsub dvbsub
)

build_dav1d() {
    cd "$SRC"
    [ -d "dav1d-$DAV1D" ] || git clone -q --depth 1 -b "$DAV1D" https://code.videolan.org/videolan/dav1d.git "dav1d-$DAV1D"
    cd "dav1d-$DAV1D"
    rm -rf build
    meson setup build --prefix="$PREFIX" --libdir=lib --buildtype=release -Ddefault_library=static \
        -Denable_tools=false -Denable_tests=false -Denable_examples=false
    ninja -C build install
}

build_displayinfo() {
    cd "$SRC"
    [ -d "libdisplay-info-$DISPLAYINFO" ] || git clone -q --depth 1 -b "$DISPLAYINFO" \
        https://gitlab.freedesktop.org/emersion/libdisplay-info.git "libdisplay-info-$DISPLAYINFO"
    cd "libdisplay-info-$DISPLAYINFO"
    rm -rf build
    meson setup build --prefix="$PREFIX" --libdir=lib --buildtype=release -Ddefault_library=static
    ninja -C build install
}

build_libass() {
    cd "$SRC"
    [ -d "libass-$LIBASS" ] || git clone -q --depth 1 -b "$LIBASS" https://github.com/libass/libass.git "libass-$LIBASS"
    cd "libass-$LIBASS"
    rm -rf build
    meson setup build --prefix="$PREFIX" --libdir=lib --buildtype=release -Ddefault_library=static \
        -Dfontconfig=enabled -Dlibunibreak=disabled -Dasm=enabled \
        -Dtest=disabled -Dcompare=disabled -Dprofile=disabled -Dfuzz=disabled -Dcheckasm=disabled
    ninja -C build install
}

build_ffmpeg() {
    cd "$SRC"
    [ -d "ffmpeg-$FFMPEG" ] || curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$FFMPEG.tar.xz" | tar xJ
    cd "ffmpeg-$FFMPEG"
    # Use plain vaMapBuffer even with libva >= 2.21 headers (same behavior, older libva works).
    sed -i 's/VA_CHECK_VERSION(1, 21, 0)/0/' libavutil/hwcontext_vaapi.c
    if grep -q 'VA_CHECK_VERSION(1, 21, 0)' libavutil/hwcontext_vaapi.c; then
        echo "vaMapBuffer2 patch did not apply" >&2; exit 1
    fi
    local profile_args=()
    if [ "$PROFILE" = slim ]; then
        profile_args=(
            --disable-decoders --enable-decoder="$(join "${SLIM_DECODERS[@]}")"
            --disable-demuxers --enable-demuxer="$(join "${SLIM_DEMUXERS[@]}")"
            --disable-parsers --enable-parser="$(join "${SLIM_PARSERS[@]}")"
        )
    fi
    make distclean >/dev/null 2>&1 || true
    ./configure --prefix="$PREFIX" --libdir="$PREFIX/lib" \
        --pkg-config-flags=--static \
        --enable-static --disable-shared --enable-pic \
        --disable-programs --disable-doc --disable-debug \
        --disable-autodetect --enable-vaapi --enable-zlib --enable-iconv --enable-libdav1d \
        --disable-encoders --disable-muxers --disable-devices --disable-network \
        --disable-protocols --enable-protocol=file,pipe,fd \
        --disable-filters --enable-filter=aformat,anull,aresample,format,null,scale,crop,pad,hflip,vflip,transpose,setpts,asetpts,volume,yadif \
        "${profile_args[@]}"
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
    # Drop vo_drm (jimmy renders through the libmpv API): it's the only user of the dumb-buffer
    # calls from libdrm 2.4.115+, which Ubuntu 22.04 lacks. DRM hwdec interop stays.
    sed -i "/'video\/out\/vo_drm.c'/d; s|'video/out/hwdec/hwdec_drmprime_overlay.c',|'video/out/hwdec/hwdec_drmprime_overlay.c')|" meson.build
    sed -i '/&video_out_drm,/d' video/out/vo.c
    if grep -q "vo_drm.c\|&video_out_drm," meson.build video/out/vo.c; then
        echo "vo_drm patch did not apply" >&2; exit 1
    fi
    rm -rf build
    # Everything optional off (lua is a choice option, so it needs its own switch),
    # then only what jimmy needs.
    meson setup build --prefix="$PREFIX" --libdir=lib --buildtype=release -Ddefault_library=static \
        -Dlibmpv=true -Dcplayer=false --auto-features=disabled -Dlua=disabled \
        -Dgl=enabled -Degl=enabled -Dplain-gl=enabled \
        -Dvaapi=enabled -Dvaapi-drm=enabled -Ddrm=enabled \
        -Dpipewire="$PIPEWIRE" -Dpulse="$PULSE" -Dalsa=enabled -Dzlib=enabled -Diconv=enabled
    ninja -C build install
}

for s in $STAGES; do
    echo "==> $s ($PROFILE, pipewire $PIPEWIRE, pulse $PULSE)"
    "build_$s"
done
echo "done: $PREFIX"
