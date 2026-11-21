#!/usr/bin/env bash
# Fail if a Linux release binary won't run on the distros we promise (glibc 2.34+: EL9,
# Debian 12, Ubuntu 22.04 and newer). The binary is linked with BIND_NOW, so one missing
# symbol stops it from starting at all.
#   packaging/check-portable.sh BIN
set -euo pipefail
BIN="$1"
fail=0

newest() { { grep -o "$1[0-9.]*" || true; } | sort -Vu | tail -1; }
older_or_equal() { [ "$(printf '%s\n%s\n' "$1" "$2" | sort -V | tail -1)" = "$2" ]; }

syms="$(objdump -T "$BIN")"
glibc="$(newest GLIBC_ <<<"$syms")"
glibcxx="$(newest GLIBCXX_ <<<"$syms")"
echo "newest symbol versions: $glibc ${glibcxx:-(no libstdc++)}"
older_or_equal "$glibc" GLIBC_2.34 || { echo "FAIL: needs $glibc (max GLIBC_2.34)"; fail=1; }
# GCC 11's libstdc++ (EL9); Ubuntu 22.04 has 3.4.30.
[ -z "$glibcxx" ] || older_or_equal "$glibcxx" GLIBCXX_3.4.29 || { echo "FAIL: needs $glibcxx (max GLIBCXX_3.4.29)"; fail=1; }

needed="$(readelf -d "$BIN" | sed -n 's/.*(NEEDED).*\[\(.*\)\]/\1/p')"
echo "NEEDED:"; sed 's/^/  /' <<<"$needed"
# Libraries every desktop distro (and the Flatpak runtime) has, with stable sonames.
allowed='ld-linux-x86-64\.so\.2|lib(c|m|dl|pthread|rt|gcc_s|stdc\+\+|z|asound|pulse|drm|EGL|va|va-drm|udev|freetype|harfbuzz|fribidi|fontconfig)\.so\.[0-9]+'
extra="$(grep -vxE "$allowed" <<<"$needed" || true)"
[ -z "$extra" ] || { echo "FAIL: NEEDED outside the allowlist:"; sed 's/^/  /' <<<"$extra"; fail=1; }
dyn="$(grep -E 'mpv|av(codec|format|util|filter)|sw(scale|resample)|placebo|dav1d|libass|display-info' <<<"$needed" || true)"
[ -z "$dyn" ] || { echo "FAIL: linked dynamically, should be static: $(tr "\n" " " <<<"$dyn")"; fail=1; }

undef="$(nm -D --undefined-only "$BIN")"
# Newer than Ubuntu 22.04's libva 2.14 / libdrm 2.4.113 (build-slim-mpv.sh patches FFmpeg and
# mpv not to use them).
too_new="$(grep -owE 'vaMapBuffer2|drmModeCreateDumbBuffer|drmModeMapDumbBuffer|drmModeDestroyDumbBuffer' <<<"$undef" || true)"
[ -z "$too_new" ] || { echo "FAIL: imports symbols from newer libva/libdrm: $(tr '\n' ' ' <<<"$too_new")"; fail=1; }
echo "libva/libdrm imports (review when bumping FFmpeg/mpv):"
{ grep -oE '\b(va|drm)[A-Z][A-Za-z0-9_]*' <<<"$undef" || true; } | sort -u | tr '\n' ' '; echo

[ "$fail" = 0 ] && echo "portable: OK"
exit "$fail"
