---
name: release
description: Publish a new jimmy release (version bump, what's new, tag, static builds for Linux/Windows/macOS, release notes with download links) and verify the result. Use when asked to release, publish, tag or ship a new version of jimmy.
---

# Publish a jimmy release

Releases are built by `.github/workflows/release.yml` on GitHub-hosted runners when a `vX.Y.Z` tag
is pushed. The tag's `release` job checks the assets, writes `SHA256SUMS`, publishes everything in
one go with notes from `packaging/release-notes.sh`, and then checks every download link.
Publishing is outward-facing and people download the files, so only do it when the user asked for
a release.

Every asset links the trimmed static mpv/FFmpeg/libplacebo/dav1d/libass (the `full` codec profile;
the `slim` profile is never released). Every asset is built with opt-level 3, fat LTO, one codegen
unit, `panic=abort` and stripped symbols (the workflow's `CARGO_PROFILE_RELEASE_*` env). Don't
weaken either.

Repo conventions: commits and tags use the noreply identity already set in this checkout, with
UTC timestamps (`TZ=UTC git commit …`, `TZ=UTC git tag …`). No personal info (names, emails,
machine paths) goes in notes, commits or files.

## 1. Pick the version
- Check the latest release (`gh release list -L 3`) and what changed since its tag
  (`git log --oneline <last-tag>..HEAD`).
- Semver while 0.x: new features → bump minor (0.1.0 → 0.2.0), only fixes → bump patch. Ask the
  user if unsure.
- If `Cargo.toml` already has a version that was never tagged (check `git tag -l`), release that
  version instead of bumping again.

## 2. Bump and write what's new (skip the parts already done)
1. `version` in `Cargo.toml`, then `cargo update -w --offline` to refresh `Cargo.lock` (it doesn't
   need libmpv). `grep -A1 'name = "jimmy"' Cargo.lock` shows the new version.
2. The AppStream `<release>` version and date in `packaging/linux/io.github.owallzz.jimmy.metainfo.xml`
   are filled in from `Cargo.toml` at build time. Don't add entries by hand.
3. Write `docs/whats-new/X.Y.Z.md`: 3–6 Markdown bullets of user-facing features and fixes since the
   last release, taken from the git log. Use plain words: no internal names, file names or commit
   hashes. The release job puts it under "What's new" in the notes.
4. Commit ("Release X.Y.Z") and push to `main`.

## 3. Wait for green builds on the exact commit you'll tag
- A push to `main` that touches `Cargo.lock`, `build.rs`, `packaging/**` or the workflow runs
  `release.yml` as a full test build (every asset, no publishing) as well as `ci.yml`. Otherwise
  start one with `gh workflow run release.yml --ref main`. The test build also keeps the static
  mpv caches on `main`, where the tag run can restore them. Tag runs can't share caches with each
  other.
- Find the runs with `gh run list -L 4 --branch main`. Wait for them in the background and only
  continue when every job of both workflows on the head commit is green:
  ```bash
  gh run watch <id> --exit-status --interval 120
  ```
- Before pushing, check locally when the code changed:
  - `cargo check --locked --all-targets --target x86_64-pc-windows-gnu` and the same for
    `aarch64-apple-darwin` (`rustup target add` both once).
  - Linux tests in the build container (see the build-native skill).
- If a job fails, read its log:
  ```bash
  curl -sSL -H "Authorization: Bearer $(gh auth token)" \
    https://api.github.com/repos/owallzz/jimmy/actions/jobs/<job id>/logs
  ```
  Strip the ANSI codes and grep for `error`. Fix it on `main` and wait again.

## 4. Tag and push
```bash
TZ=UTC git tag -a vX.Y.Z -m "jimmy vX.Y.Z"
git push origin vX.Y.Z
```
- `check-version` fails the run if the tag doesn't match `Cargo.toml`.
- The run takes about 12 minutes with warm caches, and 25+ when the static mpv has to be rebuilt.
  Watch it in the background with `gh run watch <id> --exit-status`.

If the run fails:
1. Fix it on `main`.
2. Delete the tag locally and remotely: `git tag -d vX.Y.Z && git push origin :refs/tags/vX.Y.Z`.
3. Delete a partial release if one was created: `gh release delete vX.Y.Z --yes`.
4. Tag again once the builds are green.

Never re-tag a version that was published with assets people may have downloaded: bump the patch
version instead.

## 5. Verify the published release
- `gh release view vX.Y.Z --json assets -q '.assets[].name'` lists exactly these 8 files:
  - `jimmy-X.Y.Z-windows-x86_64-setup.exe`
  - `jimmy-X.Y.Z-windows-x86_64-portable.exe`
  - `jimmy-X.Y.Z-macos-universal.dmg`
  - `jimmy-X.Y.Z-x86_64.AppImage`
  - `jimmy-X.Y.Z-x86_64.flatpak`
  - `jimmy-X.Y.Z-<commit count>.x86_64.rpm`
  - `jimmy-X.Y.Z-linux-x86_64`
  - `SHA256SUMS`

  None of them is an archive.
- Download into a fresh, empty scratchpad directory (`gh release download vX.Y.Z -D <dir>`), then
  check:
  - Checksums: `sha256sum -c SHA256SUMS`.
  - Linux binary: `chmod +x`, then `packaging/check-portable.sh jimmy-X.Y.Z-linux-x86_64`
    (glibc 2.34 baseline, no dynamic mpv/FFmpeg) and `./jimmy-X.Y.Z-linux-x86_64 --version`.
  - AppImage: `APPIMAGE_EXTRACT_AND_RUN=1 ./jimmy-X.Y.Z-x86_64.AppImage --version`.
  - macOS: `7z x` the dmg, then `file …/jimmy.app/Contents/MacOS/jimmy` must say "universal binary
    with 2 architectures".
  - Windows: `file` says "PE32+ … (GUI), x86-64" for the portable exe and "PE32 … (GUI), Intel
    i386" for the setup exe (the Inno Setup stub is 32-bit, which is expected). CI already checked
    that the exe needs only Windows' own DLLs.
- Leak check: `strings` on the binaries may only show CI paths: `/github/home`, `/__w/jimmy` and
  `/opt/jimmy-mpv` (Linux), `/Users/runner` (macOS), `D:\a\jimmy` and `D:/a/_temp` (Windows).
  It must never show a personal home directory, username or hostname.
- The release job already checked every download link in the notes. To check again:
  ```bash
  gh release view vX.Y.Z --json body -q .body | grep -o 'https://github.com/owallzz/jimmy/releases/download/[^)]*' \
    | while read -r u; do curl -sIL -o /dev/null -w '%{http_code} ' "$u"; echo "${u##*/}"; done
  ```

## 6. Release notes
- `packaging/release-notes.sh VERSION RPM_FILE [WHATS_NEW_FILE]` writes them. The release job runs it
  with `docs/whats-new/X.Y.Z.md`. The structure:
  - A one-line summary.
  - **Downloads**: direct links grouped by platform, recommended file first.
    - Windows: Installer, then Portable.
    - macOS: Disk image.
    - Linux: AppImage, Flatpak, RPM, then Standalone binary.
    - Then `SHA256SUMS`.
  - **What's new**.
  - **First launch**: SmartScreen and Gatekeeper on the unsigned builds, plus the curl note.
  - **Known limits**.
  - The license note (GPL binaries, MIT source).
- To change the notes of a published release, regenerate them into a scratchpad file and apply:
  ```bash
  gh release edit vX.Y.Z --title "jimmy vX.Y.Z" --notes-file <file>
  ```
  When something about the downloads, platforms or limits changes for good, edit the script, not
  just one release.
- Add anything not verified on real hardware to Known limits (Windows and macOS have so far only
  been run by CI).

## 7. Afterwards
- Tell the user the release URL and what was verified.
- If the release added features or changed platforms, make sure `README.md` (Features, Install,
  controls) and `THIRD-PARTY.md` (library versions, when a build script's pins changed) match.
