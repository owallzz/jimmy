# Packaging

`build-rpm.sh` builds a release binary and wraps it in an RPM with the launcher and icon:

| Installed file | Purpose |
|---|---|
| `/usr/bin/jimmy` | the app |
| `/usr/share/applications/jimmy.desktop` | app menu entry, "Open With" for videos |
| `/usr/share/icons/hicolor/scalable/apps/jimmy.svg` | icon |
| `/usr/share/doc/jimmy/README.md` | docs |

The release number is the git commit count, so each new build upgrades the installed one.
The RPM depends on `mpv-libs` (plus `libmpv.so.2` and `libudev.so.1`, detected automatically).

## On Fedora

```bash
sudo dnf install cargo rust gcc mpv-devel systemd-devel rpm-build
packaging/build-rpm.sh            # → dist/jimmy-<version>-1.fc<NN>.<arch>.rpm
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

## Releases (GitHub Actions)

`.github/workflows/release.yml` builds three x86_64 variants in a Fedora 44 container, all with
`-C target-cpu=x86-64-v2` (SSE4.2/POPCNT; any x86_64 CPU since 2009):

| Variant | mpv / FFmpeg | Built with |
|---|---|---|
| `dynamic` | system libmpv (RPM requires `mpv-libs`) | `packaging/build-rpm.sh` |
| `static` | built in, every decoder/demuxer | `PROFILE=full build-slim-mpv.sh`, then `JIMMY_MPV_PREFIX=… build-rpm.sh` |
| `static-slim` | built in, common codecs/containers only | `PROFILE=slim build-slim-mpv.sh`, then the same |

Each produces `jimmy-<ver>-<count>.<variant>.fc44.x86_64.rpm` and
`jimmy-<ver>-<variant>-linux-x86_64.tar.gz`. Running the workflow by hand uploads them as
workflow artifacts; pushing a tag `v<version>` (must match `Cargo.toml`) publishes a GitHub release
with all six files and `SHA256SUMS`. The static mpv builds are cached per profile and rebuild only
when `build-slim-mpv.sh` changes.
