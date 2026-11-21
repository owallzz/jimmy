#!/usr/bin/env bash
# Print the release notes for a jimmy release (Markdown): download links at the top, grouped by
# platform with the recommended file first, then what's new, first launch and known limits.
#
#   packaging/release-notes.sh VERSION RPM_FILE [WHATS_NEW_FILE] > notes.md
#
# RPM_FILE is the RPM's file name (its release number is the commit count, e.g.
# jimmy-0.1.0-55.x86_64.rpm). WHATS_NEW_FILE holds the "What's new" bullets (Markdown);
# without it the section is left out. Used by .github/workflows/release.yml and the
# release skill (.claude/skills/release/SKILL.md).
set -euo pipefail
V="${1:?usage: release-notes.sh VERSION RPM_FILE [WHATS_NEW_FILE]}"
RPM="${2:?usage: release-notes.sh VERSION RPM_FILE [WHATS_NEW_FILE]}"
NEW="${3:-}"
REPO="${GITHUB_REPOSITORY:-owallzz/jimmy}"
DL="https://github.com/$REPO/releases/download/v$V"

cat <<EOF
A lightweight, gamepad-friendly media player and library. Every build has mpv and FFmpeg built
in (every decoder and container), so there's nothing else to install.

## Downloads

**Windows** (10/11, x64)
- [Installer](${DL}/jimmy-${V}-windows-x86_64-setup.exe) (recommended): Start menu entry and *Open with* for video files; installs for you alone or for everyone
- [Portable](${DL}/jimmy-${V}-windows-x86_64-portable.exe): a single exe that runs without installing

**macOS** (11 or newer, Apple Silicon and Intel)
- [Disk image](${DL}/jimmy-${V}-macos-universal.dmg): open it and drag jimmy to Applications

**Linux** (x86_64; Ubuntu 22.04, Debian 12, Fedora, RHEL 9 and newer)
- [AppImage](${DL}/jimmy-${V}-x86_64.AppImage) (recommended): \`chmod +x\` it and run
- [Flatpak](${DL}/jimmy-${V}-x86_64.flatpak): \`flatpak install --user jimmy-${V}-x86_64.flatpak\` (needs the Flathub remote for the runtime)
- [RPM](${DL}/${RPM}) for Fedora and RHEL: \`sudo dnf install ./${RPM}\`
- [Standalone binary](${DL}/jimmy-${V}-linux-x86_64): \`chmod +x\` it and run; \`--install\` adds it to the app menu

Checksums: [SHA256SUMS](${DL}/SHA256SUMS) (\`sha256sum -c SHA256SUMS --ignore-missing\`).
EOF

if [ -n "$NEW" ]; then
    printf '\n## What'"'"'s new\n\n'
    cat "$NEW"
fi

cat <<EOF

## First launch

The builds aren't code-signed yet:
- **Windows:** if SmartScreen warns, choose *More info → Run anyway*.
- **macOS:** after macOS blocks the first launch, open *System Settings → Privacy & Security* and
  choose *Open Anyway*, or run \`xattr -dr com.apple.quarantine /Applications/jimmy.app\`.

Posters, library info and subtitle downloads use the system's \`curl\` (included with Windows 10/11,
macOS and most Linux distros). Setup and controls: [README](https://github.com/$REPO#readme).

## Known limits

- Windows: the volume control sets jimmy's own volume, not the system's; no backlight control.
- macOS: opening a file from Finder's *Open With* starts jimmy but doesn't play the file yet; no
  backlight control.
- Flatpak: no system volume, backlight or keep-awake (sandbox).

jimmy is MIT licensed. The release builds include mpv (GPL-2.0-or-later), so they're distributed
under the GPL; see [THIRD-PARTY.md](https://github.com/$REPO/blob/v$V/THIRD-PARTY.md).
EOF
