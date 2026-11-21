# Packaging

`build-rpm.sh` builds a release binary (or takes a prebuilt one from `JIMMY_BIN`) and wraps it in
an RPM with the launcher and icon:

| Installed file | Purpose |
|---|---|
| `/usr/bin/jimmy` | the app |
| `/usr/share/applications/jimmy.desktop` | app menu entry, "Open With" for videos |
| `/usr/share/icons/hicolor/scalable/apps/jimmy.svg` | icon |
| `/usr/share/doc/jimmy/README.md` | docs |
| `/usr/share/licenses/jimmy/LICENSE` | MIT license |
| `/usr/share/licenses/jimmy/THIRD-PARTY.md` | licenses of the bundled libraries |

The release number is the git commit count, so each new build upgrades the installed one.
Built against the system libmpv, the RPM depends on `mpv-libs` (plus `libmpv.so.2` and
`libudev.so.1`, detected automatically). With mpv built in (see below) it doesn't, and its License
tag lists the bundled libraries. `JIMMY_TARBALL=1` also writes the old `.tar.gz`.

## On Fedora

```bash
sudo dnf install cargo rust gcc mpv-devel systemd-devel rpm-build
packaging/build-rpm.sh            # → dist/jimmy-<version>-<commit count>.fc<NN>.<arch>.rpm
sudo dnf install ./dist/jimmy-*.rpm
```

Build on the same Fedora release you install on: the binary links against that release's glibc.

## x86_64 RPM from an Apple Silicon Mac

Uses the Lima VM with Rosetta and an x86_64 Fedora container:

```bash
limactl shell jimmy-fedora -- bash -lc '
  podman build --platform linux/amd64 -t jimmy-build:fc44-x86_64 -f packaging/Containerfile packaging &&
  podman run --rm --platform linux/amd64 \
    -v "$PWD":/src:ro -v "$HOME/dist-x86":/out -v "$HOME/x86-target":/target -e CARGO_TARGET_DIR=/target \
    jimmy-build:fc44-x86_64 /src/packaging/build-rpm.sh /out'
limactl copy 'jimmy-fedora:dist-x86/jimmy-*.x86_64.rpm' dist/
```

Change `ARG FEDORA=44` (or pass `--build-arg FEDORA=43`) to target an older Fedora.

Installing the RPM does not touch per-user installs from `jimmy --install`; if you used that before,
run `jimmy --uninstall` once so the menu doesn't show two entries.

## Slim, CPU-tuned build (what's installed on the iMac)

```bash
packaging/build-slim-mpv.sh          # once: static dav1d + FFmpeg + libplacebo + libmpv in ~/.cache/jimmy-mpv
packaging/build-native-rpm.sh        # jimmy + that mpv, -C target-cpu=native, full LTO
sudo dnf install ./dist/jimmy-*.rpm
```

Links 31 shared libraries instead of ~300 (Fedora's libmpv/FFmpeg pull in Samba, Tesseract,
Python, video encoders…). On the iMac, playing 1080p H.264: 99 MB private / 139 MB RSS, versus
116 / 184 MB with Fedora's libmpv. The package no longer requires `mpv-libs`.

## Linux releases (GitHub Actions)

`.github/workflows/release.yml` builds **one** portable x86_64 binary in an `almalinux:9` container
(glibc 2.34) and makes every Linux asset from it:

| Asset | Made by |
|---|---|
| `jimmy-<ver>-linux-x86_64` | the binary itself (`chmod +x`, then `./jimmy-… --install` for the app menu) |
| `jimmy-<ver>-<count>.x86_64.rpm` | `JIMMY_BIN=… build-rpm.sh` (no dist tag; Fedora and EL9+) |
| `jimmy-<ver>-x86_64.flatpak` | `linux/make-flatpak.sh` (org.freedesktop.Platform 26.08) |
| `jimmy-<ver>-x86_64.AppImage` | `linux/make-appimage.sh` (appimagetool, no bundled libraries) |

The binary links the `full` profile of `build-slim-mpv.sh` statically (mpv, FFmpeg, libplacebo,
dav1d, libass, libdisplay-info), built with `PIPEWIRE=disabled PULSE=enabled` (pipewire-pulse
covers PipeWire systems, and the Flatpak only has the pulse socket). jimmy itself is built with
opt-level 3, fat LTO, one codegen unit, `panic=abort`, stripped, `-C target-cpu=x86-64-v2`
(SSE4.2/POPCNT; any x86_64 CPU since 2009), set by `CARGO_PROFILE_RELEASE_*` env vars.

It still uses a few system libraries every desktop has: glibc 2.34+, libstdc++, libva, libva-drm,
libdrm, libEGL, libpulse, libasound, libudev, zlib, freetype, harfbuzz, fribidi and fontconfig.
That covers Fedora, EL9, Debian 12, Ubuntu 22.04 and newer. `check-portable.sh BIN` fails the build
if the binary needs a newer glibc or libstdc++, a library outside that list, a dynamic
mpv/FFmpeg/libass, or symbols from a newer libva/libdrm than Ubuntu 22.04's (`build-slim-mpv.sh`
patches FFmpeg not to use `vaMapBuffer2` and leaves mpv's `vo_drm` out).

Locally, the same build in podman (takes a while the first time; mpv lands in `/opt/jimmy-mpv`):

```bash
podman run --rm -v "$PWD":/src:Z -v jimmy-mpv-el9:/opt/jimmy-mpv -w /src almalinux:9 bash -euc '
  dnf -y install epel-release dnf-plugins-core && dnf config-manager --set-enabled crb
  dnf -y --setopt=install_weak_deps=False install git gcc gcc-c++ make nasm xz tar gzip python3-pip \
    python3-jinja2 pkgconf-pkg-config ninja-build hwdata binutils rpm-build systemd-devel libva-devel \
    libdrm-devel mesa-libEGL-devel alsa-lib-devel pulseaudio-libs-devel zlib-devel freetype-devel \
    harfbuzz-devel fribidi-devel fontconfig-devel bzip2-devel libpng-devel brotli-devel glib2-devel \
    graphite2-devel expat-devel
  pip3 install "meson>=1.3,<2"
  curl -fsSL https://sh.rustup.rs | sh -s -- -y --profile minimal && . ~/.cargo/env
  PROFILE=full PIPEWIRE=disabled PULSE=enabled packaging/build-slim-mpv.sh /opt/jimmy-mpv
  JIMMY_MPV_PREFIX=/opt/jimmy-mpv RUSTFLAGS="-C target-cpu=x86-64-v2" CARGO_PROFILE_RELEASE_LTO=fat \
    CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 cargo build --release --locked --target-dir target-el9
  packaging/check-portable.sh target-el9/release/jimmy
  JIMMY_BIN=target-el9/release/jimmy packaging/build-rpm.sh dist'
packaging/linux/make-appimage.sh target-el9/release/jimmy dist
packaging/linux/make-flatpak.sh target-el9/release/jimmy dist   # needs flatpak-builder
```

The `slim` profile (common codecs only) is not released; it stays for local builds.

## Windows

Built in CI on `windows-latest` inside MSYS2 UCRT64, with MSYS2's own `rust` (UCRT, like the C
libraries) for `x86_64-pc-windows-gnu`. The exe links everything statically and needs only system
DLLs, so the same file is both the portable build and what the installer copies.

```bash
# MSYS2 UCRT64 shell
pacman -S --needed git curl tar xz make diffutils mingw-w64-ucrt-x86_64-{gcc,meson,ninja,nasm,pkgconf,python-jinja,zlib,libiconv,rust}
P="$(cygpath -m ~/jimmy-mpv)"                         # C:/... paths: native meson/pkgconf/rustc need them
packaging/windows/build-mpv-windows.sh "$P"           # freetype, fribidi, harfbuzz, libass, dav1d, FFmpeg, libplacebo, mpv
JIMMY_MPV_PREFIX="$P" RUSTFLAGS="-C target-cpu=x86-64-v2" CARGO_PROFILE_RELEASE_LTO=fat \
  CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 cargo build --release --locked --target x86_64-pc-windows-gnu
objdump -p target/x86_64-pc-windows-gnu/release/jimmy.exe | grep 'DLL Name'   # system DLLs only
```

Don't install MSYS2's `ffmpeg`, `mpv`, `libass`, `libplacebo`, `harfbuzz` or `freetype`: their
pkg-config files and import libraries would be picked up instead of the static ones.

`build-mpv-windows.sh` uses the same versions and codec choices as `PROFILE=full build-slim-mpv.sh`
(every decoder, demuxer and parser; no encoders, muxers, network or GPL FFmpeg parts). mpv gets
WGL OpenGL for the render API, D3D11VA/DXVA2 decoding, WASAPI, and libass with DirectWrite fonts.
Zero-copy DXVA2 needs a driver that exposes `WGL_NV_DX_interop`; otherwise use the `-copy` hwdecs.

The installer is `windows/jimmy.iss` (Inno Setup 6, preinstalled on the runner):

```powershell
& "${env:ProgramFiles(x86)}\Inno Setup 6\ISCC.exe" /DAppVersion=0.1.0 `
  /DSrcExe="$PWD\target\x86_64-pc-windows-gnu\release\jimmy.exe" packaging\windows\jimmy.iss
# → dist\jimmy-0.1.0-windows-x86_64-setup.exe
```

It asks whether to install for the current user (no admin) or all users, adds a Start menu entry,
an optional desktop icon and an uninstaller, and lists jimmy under "Open with" for common video
files without changing the default player. Settings and history live in `%APPDATA%\jimmy` for
both the installed and the portable exe.

`.gitattributes` keeps LF line endings so MSYS2 bash can run the scripts from a Git for Windows
checkout.

## macOS (universal .dmg)

`packaging/macos/` builds `jimmy-<ver>-macos-universal.dmg`: one `jimmy.app` with an arm64 and an
x86_64 slice, macOS 11.0 or newer, mpv built in. Run on an Apple Silicon Mac (the x86_64 half is
cross-compiled):

```bash
softwareupdate --install-rosetta --agree-to-license   # meson runs x86_64 test programs
brew install meson ninja nasm pkgconf librsvg
rustup target add aarch64-apple-darwin x86_64-apple-darwin
TARGET_ARCH=arm64  packaging/macos/build-mpv-macos.sh   # → ~/.cache/jimmy-mpv-macos-arm64
TARGET_ARCH=x86_64 packaging/macos/build-mpv-macos.sh   # → ~/.cache/jimmy-mpv-macos-x86_64
CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 packaging/macos/make-app.sh   # → dist/jimmy.app
packaging/macos/make-dmg.sh                             # → dist/jimmy-<ver>-macos-universal.dmg
```

| Script | What it does |
|---|---|
| `build-mpv-macos.sh` | static freetype, fribidi, harfbuzz, libass (CoreText), dav1d, FFmpeg, libplacebo and libmpv for one `TARGET_ARCH`, with the same codecs as the `full` profile of `build-slim-mpv.sh` |
| `make-app.sh` | `cargo build` for both targets (`-C target-cpu=apple-m1` / `x86-64-v2`), `lipo`, `Info.plist`, icon from `assets/jimmy.svg`, ad-hoc `codesign`; fails if anything outside `/usr/lib` and `/System` is linked dynamically |
| `make-dmg.sh` | compressed `.dmg` with `jimmy.app` and an `Applications` link |

FFmpeg decodes with VideoToolbox, but copies frames back (`videotoolbox-copy`): mpv 0.42's Cocoa
code, which zero-copy OpenGL interop needs, only builds together with its Swift parts. Audio goes
through CoreAudio.

The app has no Apple Developer ID signature and isn't notarized, so macOS blocks the first launch.
Open it once from System Settings → Privacy & Security → Open Anyway, or run
`xattr -dr com.apple.quarantine /Applications/jimmy.app`.
