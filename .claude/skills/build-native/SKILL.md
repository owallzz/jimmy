---
name: build-native
description: Compile and run jimmy with every optimization on - static slim mpv/FFmpeg/libplacebo/dav1d built from source (smaller footprint, ~30 shared libs instead of ~300), fat LTO, one codegen unit, opt-level 3, panic=abort, stripped, tuned for the build machine's CPU. Use when asked to build an optimized, release, native, LTO, slim or "fully optimized" jimmy, to run that build, or to measure its size or memory.
argument-hint: "[full|slim] [args for jimmy]"
---

# Build and run the fully optimized jimmy

## What "all optimizations" means here

| Part | Setting |
|---|---|
| mpv + FFmpeg + libplacebo + dav1d | built from source by `packaging/build-slim-mpv.sh`: **static**, no encoders/muxers/network/Lua/JS/Blu-ray, `CPU_FLAGS="-march=native -mtune=native"` |
| jimmy | `opt-level=3`, `lto=fat`, `codegen-units=1`, `panic=abort`, `strip=true`, `RUSTFLAGS="-C target-cpu=native"` |
| Linking | `build.rs` links the static mpv when `JIMMY_MPV_PREFIX` points at its prefix |

`Cargo.toml`'s release profile only has thin LTO and 4 codegen units. Override them with
`CARGO_PROFILE_RELEASE_*` env vars; don't edit `Cargo.toml` for this.

Profiles for the static mpv: `PROFILE=full` (default, every decoder/demuxer) or `PROFILE=slim`
(common codecs/containers only, smaller). Use full unless the user asks for slim.

## 1. Pick where to build

- **Fedora with the build deps installable** (`dnf` works): install the deps listed at the top of
  `packaging/build-slim-mpv.sh` plus `cargo rust gcc systemd-devel`, and build on the host.
- **Immutable host** (Silverblue, Bazzite, Kinoite...) or no `-devel` packages: build in a Fedora
  container matching the host release (`. /etc/os-release; echo $VERSION_ID`), so the libraries jimmy
  still links dynamically (libass, libva, pipewire, alsa, EGL, libudev...) match the host's. Don't
  layer build deps onto the host.

If the repo has a local helper script for this (e.g. an untracked `packaging/build-native-local.sh`),
use it; otherwise run the steps below.

## 2. Build

Keep the static mpv in a per-user cache so it's built once:
`MPV=~/.cache/jimmy-mpv-native-$PROFILE`. The `.pc` files bake in the prefix path, so the mpv build
and the jimmy build must see the mpv at the **same path**.

### On the host

```bash
PROFILE=full
MPV=~/.cache/jimmy-mpv-native-$PROFILE
[ -f "$MPV/lib/libmpv.a" ] || CPU_FLAGS="-march=native -mtune=native" PROFILE=$PROFILE packaging/build-slim-mpv.sh "$MPV"
JIMMY_MPV_PREFIX="$MPV" RUSTFLAGS="-C target-cpu=native" \
CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_OPT_LEVEL=3 \
CARGO_PROFILE_RELEASE_PANIC=abort CARGO_PROFILE_RELEASE_STRIP=true \
cargo build --release --locked --target-dir target-native
```

### In a container (podman)

```bash
FEDORA=$(. /etc/os-release && echo "$VERSION_ID"); PROFILE=full
IMAGE=jimmy-native-build:fc$FEDORA
MPV=~/.cache/jimmy-mpv-native-$PROFILE; mkdir -p "$MPV" ~/.cache/jimmy-cargo-registry
podman image exists $IMAGE || podman build -t $IMAGE -f - . <<EOF
FROM registry.fedoraproject.org/fedora:$FEDORA
RUN dnf -y --setopt=install_weak_deps=False install git cargo rust gcc gcc-c++ systemd-devel \
      pkgconf-pkg-config meson ninja-build nasm python3-jinja2 curl xz libass-devel libva-devel \
      libdrm-devel libdisplay-info-devel pipewire-devel alsa-lib-devel mesa-libEGL-devel \
      zlib-ng-compat-devel && dnf clean all
EOF
podman run --rm --security-opt label=disable -v "$PWD":/src -v "$MPV":/opt/jimmy-mpv \
  -v ~/.cache/jimmy-cargo-registry:/root/.cargo/registry -e PROFILE=$PROFILE -w /src $IMAGE bash -euc '
    [ -f /opt/jimmy-mpv/lib/libmpv.a ] || CPU_FLAGS="-march=native -mtune=native" packaging/build-slim-mpv.sh /opt/jimmy-mpv
    JIMMY_MPV_PREFIX=/opt/jimmy-mpv RUSTFLAGS="-C target-cpu=native" \
    CARGO_PROFILE_RELEASE_LTO=fat CARGO_PROFILE_RELEASE_CODEGEN_UNITS=1 CARGO_PROFILE_RELEASE_OPT_LEVEL=3 \
    CARGO_PROFILE_RELEASE_PANIC=abort CARGO_PROFILE_RELEASE_STRIP=true \
    cargo build --release --locked --target-dir target-native'
```

- The first run compiles FFmpeg and takes a while: run it in the background and wait for it, don't poll.
  Later runs reuse the cached mpv and only rebuild jimmy (~2 min with fat LTO).
- To rebuild mpv (after changing `build-slim-mpv.sh` or the CPU flags), delete `$MPV/lib/libmpv.a` first.
- Output: `target-native/release/jimmy` (git-ignored).

## 3. Check the binary

```bash
B=target-native/release/jimmy
ls -lh $B
ldd $B | grep -c '=>'            # ~30; a few hundred means the system libmpv got linked
ldd $B | grep -E 'mpv|avcodec'   # must print nothing (static)
ldd $B | grep 'not found'        # must print nothing
```

If a library is `not found` on the host, the container had a runtime lib the host lacks: report which.

## 4. Run

It's a GUI app: launch it in the background so the session isn't blocked.

```bash
target-native/release/jimmy [FILE | FOLDER]
```

Headless smoke test that exits by itself (use a temp config dir and look at the screenshot after):

```bash
JIMMY_HOME=$(mktemp -d) target-native/release/jimmy VIDEO --windowed --size 1280x720 \
  --actions "wait:4000,screenshot:$(mktemp -d)/player.png,quit"
```

Expected log noise: `WARN jimmy::player] mpv option load-scripts=no: Raw(-5)` (and `load-console`,
`ytdl`, ...). Those options don't exist in this mpv because Lua/JS are compiled out. Harmless.

Optional: `packaging/bench-memory.sh` for memory numbers; `target-native/release/jimmy --install` to
install it for the user (ask first); `packaging/build-native-rpm.sh` for an RPM.

## Report

Binary path and size, number of linked shared libraries, the profile used, and whether it ran.
The binary is CPU-specific (`target-cpu=native`): never commit or ship it. For portable builds use
`.github/workflows/release.yml` (x86-64-v2).
