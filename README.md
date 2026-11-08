# jimmy

A lightweight, gamepad-friendly media player for old hardware. Rust + egui (OpenGL) + libmpv.
No webview, no GTK/Qt.

- Plays anything FFmpeg/mpv can play, with hardware decoding where available
- Media browser built for slow disks (no file probing, idle I/O priority)
- Full gamepad control (Xbox and PlayStation button labels), keyboard, mouse
- Command palette, remappable keyboard and gamepad controls
- System volume with boost up to 200%, system backlight control
- Subtitles and closed captions (SRT, ASS, PGS, VobSub, EIA-608), resume positions
- 20 color schemes (Everforest by default)

## Fedora (the target)

Fedora's own ffmpeg can't decode H.264/HEVC. Enable RPM Fusion first:

```bash
sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm
sudo dnf swap ffmpeg-free ffmpeg --allowerasing
sudo dnf install mpv-libs mpv-devel gcc systemd-devel
# Hardware H.264 decoding on AMD/Radeon:
sudo dnf swap mesa-va-drivers mesa-va-drivers-freeworld
```

Build and add to the app menu:

```bash
curl https://sh.rustup.rs -sSf | sh     # if Rust isn't installed
cargo build --release
./target/release/jimmy --install          # add --default to make it the default video player
```

`--install` copies the binary to `~/.local/bin/jimmy` and adds a launcher and icon, so jimmy shows up in
GNOME's app grid. `jimmy --uninstall` removes them.

Gamepads: Xbox controllers work out of the box. PS5 DualSense works over USB or Bluetooth with the kernel's
`hid-playstation` driver (built into Fedora).

## macOS (development)

```bash
brew install mpv
cargo run -- ~/Movies
```

System volume works through `osascript`; there is no backlight control on macOS.

## Controls (defaults, all remappable in Settings → Controls)

| | Keyboard | vim | Gamepad |
|---|---|---|---|
| Move | ↑ ↓ | j k, `5j`, gg / G, Ctrl+D / Ctrl+U | D-pad, left stick |
| Open / up a folder | Enter, → / Backspace, ← | l / h | A (✕) / B (○) |
| Play from start | Shift+Enter | o | X (□) |
| Filter the list | / | / | — |
| Command palette | Ctrl+P | : | Y (△) in the browser |
| Menu (My media, folders, settings) | Tab | | ≡ |
| Close a menu | Esc | q | B (○) |
| Play / pause | Space | | A (✕) |
| Seek ±10 s / ±60 s | ← → / Shift+← → | h l / H L | D-pad ◀ ▶ / LB RB |
| Volume | ↑ ↓ | k j | D-pad ▲ ▼ |
| Brightness | PgUp / PgDn | | right stick ▲ ▼ |
| Subtitles / audio track | C / A | | X (□) / Y (△) |
| Tracks panel | T | | ≡ |
| Stop | Backspace | q | B (○) twice |
| Fullscreen | F11, F (player) | | View |

In the browser, press `/` to filter: type, Enter keeps the filter, Esc clears it. Settings live in
`~/.config/jimmy/` (`config.toml`, `bindings.toml`), history in `~/.local/share/jimmy/state.toml`.
The clock is 12-hour by default; switch to 24-hour in Settings → General.

## Command line

```
jimmy [FILE | FOLDER] [--fullscreen | --windowed] [--size WxH]
jimmy --install [--default] | --uninstall
```

For testing, `--actions` runs a script of actions and can save screenshots:

```bash
jimmy ~/Videos --actions "wait:1000,down,accept,wait:3000,screenshot:/tmp/player.png,quit"
jimmy --actions "key:j,key:Slash,type:moon,key:Enter,quit"   # real key presses and typing
JIMMY_FAKE_PAD=ps jimmy ...   # show PlayStation button hints without a controller
JIMMY_HOME=/tmp/jimmy-test jimmy ...   # use a separate config/history folder
```
