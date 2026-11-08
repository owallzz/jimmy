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

### Install the RPM

```bash
sudo dnf install ./jimmy-0.1.0-*.fc44.x86_64.rpm
```

This puts `jimmy` in `/usr/bin` and adds it to the app menu. To build the RPM yourself, see
[packaging/README.md](packaging/README.md).

### Or build from source

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
| Play from start / add folder to library | Shift+Enter | o | X (□) |
| Search (filter the list) | / | / | Y (△): on-screen keyboard |
| Command palette | Ctrl+P | : | Guide (Xbox / PS button); Y (△) inside it types |
| Menu (My media, folders, settings) | Tab | | ≡ |
| Close a menu | Esc | q | B (○) |
| Play / pause | Space | | A (✕) |
| Seek ±10 s / ±60 s | ← → / Shift+← → | h l / H L | D-pad ◀ ▶ / LB RB |
| Volume | ↑ ↓ | k j | D-pad ▲ ▼ |
| Brightness | PgUp / PgDn | | right stick ▲ ▼ |
| Subtitles / audio track | C / A | | X (□) / Y (△); hold X (□) to pick from a list |
| Tracks panel | T | | ≡ |
| Stop | Backspace | q | B (○) twice |
| Fullscreen | F11, F (player) | | View |

jimmy opens on **My media** (everything in your library folders). Back goes up one folder; at the top of
a tab (My media, Continue watching, a library folder…) it opens the menu instead, so you only switch
tabs from the menu. To add folders, open the menu → **Browse computer…**, move to a folder and
press X (□) (Shift+Enter on a keyboard); press it again to remove it. Library folders are tagged
`LIBRARY`.

In the browser, press `/` to filter: type, Enter keeps the filter, Esc clears it. With a controller,
Y (△) opens an on-screen keyboard: D-pad/stick moves, A (✕) types, X (□) deletes, Y (△) adds a space,
B (○) or ≡ finishes. Results filter live while you type. Settings live in
`~/.config/jimmy/` (`config.toml`, `bindings.toml`), history in `~/.local/share/jimmy/state.toml`.
The clock is 12-hour by default; switch to 24-hour in Settings → General.

## TV shows, movies and playlists

- **TV shows** groups episodes by show and season from their names: `Show S01E02 Title.mkv`,
  `Show - 1x02.mkv`, or `Show/Season 1/03 - Title.mkv`. A show's page has season tabs (LT/RT,
  `[` `]`, or left/right), marks what's watched and what's next; X on a show plays its next
  episode. Everything else is under **Movies**.
- Playing an episode queues the rest of the show. When one ends an **Up next** card counts
  down (A plays now, B cancels). The player's **Episodes** tab (E, LS, or from Tracks) jumps
  to any episode; Shift+. / Shift+, (right stick) go to the next / previous one.
- **Repeat** (R in the player): off, this video, or the whole queue. **When a video ends**:
  go back to the list, or stay on the last frame with Replay / Back. Both are in Settings →
  Playback and the Episodes tab.
- **Playlists** are `.m3u8` files. P (right-stick click) adds the selected video, a whole show,
  or a playlist to a new or existing playlist. Any `.m3u`/`.m3u8` in your folders works too.
- **Posters and info**: `tvshow.nfo`, `movie.nfo` / `<name>.nfo` and `poster.jpg` /
  `folder.jpg` / `<name>-poster.jpg` next to your files are used first, offline. Online,
  shows come from TVmaze (no account needed); movies need a free TMDB key in `config.toml`:
  `tmdb_api_key = "…"`. Only what you look at is looked up, via the system `curl`. Turn it
  off or refresh it in Settings → Library.
- **Grid view** (V, or left-stick click, in the browser) shows Movies, TV shows, My media,
  Continue watching, Recent and playlists as a poster grid; it's remembered. Folders and a
  show's episode page stay as lists.
- Settings → Library → **Clear watch history** forgets positions and watched marks.

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
