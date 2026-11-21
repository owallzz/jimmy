# Third-party software in jimmy's release builds

jimmy's own code is MIT licensed (see [LICENSE](LICENSE)). The release builds link these
libraries statically. Because mpv is built with its default GPL option, **the release binaries
as a whole are distributed under GPL-2.0-or-later**.

| Library | Version | License | Source |
|---|---|---|---|
| mpv (libmpv) | 0.42.0 | GPL-2.0-or-later | https://github.com/mpv-player/mpv |
| FFmpeg | 8.1.3 | LGPL-2.1-or-later | https://ffmpeg.org/releases/ |
| libplacebo | 7.387.0 | LGPL-2.1-or-later | https://code.videolan.org/videolan/libplacebo |
| dav1d | 1.5.4 | BSD-2-Clause | https://code.videolan.org/videolan/dav1d |
| libass | 0.17.5 | ISC | https://github.com/libass/libass |
| libdisplay-info (Linux) | 0.3.0 | MIT | https://gitlab.freedesktop.org/emersion/libdisplay-info |
| FreeType (Windows, macOS) | 2.15.0 | FTL or GPL-2.0-or-later | https://freetype.org |
| HarfBuzz (Windows, macOS) | 15.2.0 | MIT | https://github.com/harfbuzz/harfbuzz |
| FriBidi (Windows, macOS) | 1.0.18 | LGPL-2.1-or-later | https://github.com/fribidi/fribidi |
| fast_float (inside libplacebo) | bundled | Apache-2.0 or MIT or BSL-1.0 | https://github.com/fastfloat/fast_float |
| zlib, libiconv (Windows) | MSYS2 builds | Zlib, LGPL-2.1-or-later | https://packages.msys2.org |
| mingw-w64 winpthreads, libstdc++ (Windows) | MSYS2 UCRT64 toolchain | MIT and BSD-3-Clause; GPL-3.0 with GCC Runtime Library Exception | https://www.mingw-w64.org, https://gcc.gnu.org |

The Rust crates jimmy depends on are listed in [Cargo.lock](Cargo.lock), all unmodified from
crates.io. Most are MIT or Apache-2.0. These are under other licenses:

| Crate | License |
|---|---|
| libmpv2, libmpv2-sys | LGPL-2.1 |
| nucleo-matcher, option-ext | MPL-2.0 |
| epaint_default_fonts (the embedded Hack, Noto Emoji, Ubuntu Light and emoji icon fonts) | OFL-1.1, Ubuntu Font Licence 1.0, MIT |

`cargo tree` or a tool such as `cargo about` lists every crate with its license.

## Corresponding source

The exact library versions above are fetched from upstream (release tarballs, or shallow clones
of the release tags, with submodules for libplacebo) and built by the scripts in [packaging/](packaging/): `build-slim-mpv.sh` (Linux),
`windows/build-mpv-windows.sh` and `macos/build-mpv-macos.sh`. The scripts apply a few small
patches inline, and those patches are documented in the scripts:

- FFmpeg never calls `vaMapBuffer2`.
- mpv leaves out `vo_drm` (Linux).
- mpv builds `d3d11_helpers.c` with d3d-hwaccel (Windows).
- mpv compiles `utils-mac.c` without cocoa (macOS).

The source of every release, including these scripts, is the tagged commit in this repository.
