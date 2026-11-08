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

## Libraries, TV shows, movies and playlists

- **Libraries are typed, like Plex.** Adding a folder (Browse computer… → X, or Settings →
  Library) asks whether it holds **Movies** or **TV Shows**; ◀ ▶ on a library in Settings
  switches it. Old folder lists are converted automatically.
- **TV libraries:** each top-level folder is a show, matched by its name:
  `Show (Year) {tmdb-1396}` or `{tvdb-…}` / `{imdb-…}` hints skip the search. Inside:
  `S01E02`, `1x02`, multi-episode `S01E01-E02`, dates (`2024-03-05`), absolute numbers
  (`Show - 105`, mapped to seasons once matched), `Specials/` = season 0, split files
  (`- pt1`, `cd2`).
- **Movie libraries:** one movie per `Title (Year)` folder or per file; split movies play
  their parts back to back.
- **Matching:** TMDB for movies and shows (free key: `tmdb_api_key = "…"` in `config.toml`).
  Without a key, shows use TVmaze and movies use local files only. Local `tvshow.nfo`,
  `movie.nfo`, `poster.jpg`, `fanart.jpg` and `season01-poster.jpg` are used first.
  Everything lands in `library.json`, keyed by TMDB ID. The libraries are rescanned in the
  background (at startup and every 10 minutes) so new files appear on their own.
- **Fix Match** (M, or from the palette): search TMDB and pick the right show or movie; your
  pick sticks.
- **Art:** posters, season posters, episode thumbnails and background art, downloaded only
  when shown and shrunk once. A show's page in grid view is a wall of episode thumbnails.
- **Recently added** in the menu lists the newest files.
- **TV shows** page: season tabs (LT/RT, `[` `]`, or left/right), what's watched and next;
  X on a show plays its next episode. Playing an episode queues the rest of the show, with
  an **Up next** countdown (A plays now, B cancels). In the player: **Episodes** (E, LS),
  next / previous episode (Shift+. / Shift+, or the right stick), **frame step** with `.`
  and `,`.
- **Repeat** (R in the player) and **When a video ends** (go back, or stay with Replay /
  Back) are in Settings → Playback and the Episodes tab.
- **Playlists** are `.m3u8` files: P (right-stick click) adds a video, a show or a playlist.
- **Grid view** (V, or left-stick click) for Movies, TV shows, My media, Recently added,
  Continue watching, Recent and playlists.
- **Download subtitles** on demand: hold the Subs button and pick “+ Download subtitles…”
  (also in Tracks → Subs, D, or the palette). Results come from OpenSubtitles.com, exact
  matches for your file first; the one you pick is added and selected, and comes back every
  time you play that video. Needs a free key: `opensubtitles_api_key = "…"`; languages:
  `sub_languages = "en,es"`.
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
