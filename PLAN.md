# jimmy — Lightweight Media Player (Plan)

## Decisions (locked)
- **Name:** `jimmy` (always lowercase).
- **UI:** egui via `egui_glow` on winit + glutin (OpenGL 3.3).
- **Desktop:** Fedora Workstation → GNOME on Wayland (X11 fallback via winit).
- **Controllers:** Xbox and PS5 DualSense. Button glyphs switch automatically (A/B/X/Y ↔ ✕/○/□/△).
- **Brightness:** changes the **system backlight** (same setting as GNOME's brightness slider).
- **Volume:** changes the **system volume**, with boost **up to 200%** (see §4 Volume).

A fast, utilitarian, gamepad-first media player for a 2010 iMac running Fedora.
Rust, no webview, no GTK/Qt.

---

## 1. Target hardware reality check

| Part | 2010 iMac (typical) | What it means for us |
|---|---|---|
| CPU | Core i3/i5 (Clarkdale/Lynnfield), 2–4 cores | Software decode of 1080p HEVC/VP9/AV1 will struggle. H.264 is fine. |
| GPU | ATI Radeon HD 4670 / 5670 / 5750 (TeraScale, Mesa `r600`) | **OpenGL 3.3 yes, Vulkan no.** Rules out Vulkan-only renderers; wgpu would fall back to its GL path. Hardware decode (UVD) of H.264/MPEG-2/VC-1 via VA-API. No HEVC hw decode. |
| Disk | Spinning HDD (or a slow external) | Seek latency dominates. Avoid stat/probe storms, read ahead aggressively. |

Consequences for the design:
- Render with **plain OpenGL 3.3** (no Vulkan, no heavy shader pipeline).
- Let **libmpv** do decoding — it's FFmpeg underneath, so it plays practically everything.
- Default to a **"fast" mpv profile** and offer a **"Rescue mode"** toggle for files the CPU can't keep up with (skip loop filter, fast decode, frame dropping).

### Fedora gotcha (important)
Fedora ships `ffmpeg-free`, which **does not include H.264/HEVC decoders**. We need RPM Fusion:

```bash
sudo dnf install https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-$(rpm -E %fedora).noarch.rpm
sudo dnf swap ffmpeg-free ffmpeg --allowerasing
sudo dnf install mpv-libs mpv-devel
sudo dnf swap mesa-va-drivers mesa-va-drivers-freeworld   # H.264 hw decode on the Radeon
```

---

## 2. Architecture

```
┌──────────────────────────────────────────────────────────────┐
│ winit window + glutin OpenGL 3.3 context                     │
│                                                              │
│  ┌──────────────┐   ┌────────────────────────────────────┐   │
│  │ Input layer  │──▶│ Action dispatcher (single enum)    │   │
│  │ keyboard     │   │  PlayPause, Seek(+10), VolUp,      │   │
│  │ mouse        │   │  CycleSubs, OpenPalette, NavUp...  │   │
│  │ gamepad      │   └───────────────┬────────────────────┘   │
│  │ (gilrs)      │                   │                        │
│  └──────────────┘        ┌──────────┴──────────┐             │
│        ▲ bindings.toml   ▼                     ▼             │
│                   ┌─────────────┐      ┌───────────────┐     │
│                   │ UI (screens)│      │ Player (mpv)  │     │
│                   │ browser     │      │ libmpv render │     │
│                   │ player OSD  │      │ API → GL FBO  │     │
│                   │ palette     │      └───────────────┘     │
│                   │ settings    │                            │
│                   └─────────────┘                            │
│  Frame: mpv draws video into default framebuffer, UI on top  │
└──────────────────────────────────────────────────────────────┘
        Background threads: directory scanner, resume-state writer
```

Key ideas:
- **One `Action` enum** is the heart of the app. Keyboard, mouse, gamepad, and the command palette all produce `Action`s. Remapping = editing a table from input → action. The palette is literally a fuzzy search over `Action`s (with their current bindings shown).
- **mpv render API** (`mpv_render_context` with OpenGL) — mpv draws video into our GL context, we draw the UI on top in the same frame. No second window, no compositing hacks.
- **Redraw only when needed**: when paused or in the browser, the app idles at ~0% CPU (no continuous redraw loop). During playback, redraws are driven by mpv's "new frame" callback.
- **Lazy everything**: mpv is initialized on startup but no file opened; the browser shows the last directory from a cached listing instantly, then refreshes in the background.

### Crates (planned)
| Purpose | Crate |
|---|---|
| Playback | `libmpv2` (render API) or thin `libmpv-sys` bindings |
| Window / events | `winit` |
| GL context | `glutin` + `glow` |
| UI | see §3 (recommendation: `egui` with `egui_glow`) |
| Gamepad | `gilrs` (uses SDL's GameControllerDB — Xbox/PS/8BitDo/etc. map correctly) |
| Config | `serde` + `toml` |
| Fuzzy search (palette, browser filter) | `nucleo-matcher` |
| Paths | `directories` |
| System backlight | `zbus` → GNOME settings daemon (falls back to systemd-logind `SetBrightness`) |
| System volume | `libpulse-binding` (talks to PipeWire through `pipewire-pulse`, which Fedora ships by default) |
| Keep screen awake | Wayland idle-inhibit protocol / `org.freedesktop.ScreenSaver.Inhibit` via `zbus` |

---

## 3. UI library recommendations

Constraints: Rust, no webview, no GTK/Qt, must run on OpenGL 3.3 (no Vulkan), must share a GL context with mpv, quick startup, easy to make sharp/flat.

### ⭐ 1. egui (via `egui_glow`, *not* eframe's wgpu) — **Recommended**
- Immediate mode: UI is a function of state, which makes focus handling for gamepad navigation simple (we own the focus model).
- Runs on plain OpenGL through `glow` — works well with r600.
- Trivially styled sharp: `rounding = 0`, no shadows, custom fonts/colors.
- Very easy to draw *on top of* mpv's frame in the same GL context.
- Mature, huge ecosystem, fast compile, small binary, ~100–200 ms startup.
- Downsides: default look is "dev tool" (we'll restyle it entirely); text is fine but not the crispest; built-in keyboard focus is basic (we'll do our own for the 10-foot UI anyway).

### 2. Slint — strong runner-up
- Declarative `.slint` markup, designed for embedded devices → very light, has a GL (FemtoVG) renderer and a pure software renderer.
- Has an official pattern for rendering OpenGL *underneath* the UI (good for mpv).
- Better built-in focus/keyboard navigation than egui.
- Downsides: licensing is GPLv3 or a royalty-free license with attribution (fine for personal use, worth knowing); custom drawing and dynamic layouts are less flexible; another language (.slint) to learn.

### 3. iced
- Elm-style architecture, pretty, retained-ish.
- Defaults to wgpu (no Vulkan here → GL fallback, heavier startup); has a `tiny-skia` software renderer.
- Embedding an mpv GL surface is awkward compared with the other two.
- Good choice on newer hardware, **not ideal for this iMac**.

### 4. Hand-rolled: winit + glow + `femtovg` (or `glyphon`/`fontdue` for text)
- Smallest binary and fastest startup possible; total control.
- We'd write our own widgets (lists, sliders, text input). Our UI is small, so this is feasible, but it's the most work.
- Good fallback if egui's overhead ever shows up in profiling.

### Not recommended here
- **Bevy** — a game engine; heavy startup, Vulkan-first. Not a fit.
- **Dioxus/Tauri** — webview (excluded).
- **Xilem/Masonry, Freya, Floem** — interesting but younger, wgpu/Skia-based, heavier on old GPUs.

---

## 4. Features → design

### Media browser
- File-system based (pick root folders in settings; e.g. `~/Videos`, `/mnt/media`).
- List view (fast, sharp, keyboard-first). Columns: name, size, duration if known, resume marker.
- Filter-as-you-type (fuzzy), sort (name / date / size), "Recently played" and "Continue watching" virtual folders.
- No thumbnails in v1 (they cost disk seeks). Designed so posters/thumbnails can be added later (§7).

### Playback
- libmpv: every FFmpeg format/codec, embedded + external subs, chapters, multiple audio tracks.
- Resume position per file.
- Fullscreen toggle (borderless), start fullscreen option.
- Track menus: audio track, subtitle track, subtitle delay/size, audio delay.
- Playback speed, A-B loop, screenshot (cheap extras mpv gives us for free).

### Closed captions / subtitles
- Embedded: SRT, ASS/SSA, PGS (Blu-ray), VobSub (DVD), **EIA-608/708 closed captions in H.264 streams**.
- External: auto-load sidecar `.srt/.ass/.vtt` (fuzzy name match), plus manual "load subtitle file…" from the browser.
- Style options: size, position, background box on/off (for readability on a TV).

### Input
- **Gamepad**: full navigation everywhere + playback control. Default mapping (all remappable):

  | Button | Browser | Playing |
  |---|---|---|
  | A | Open / play | Play/Pause |
  | B | Back / up a folder | Show OSD → back to browser |
  | X | Context menu (file info, subs…) | Cycle subtitles |
  | Y | Command palette | Cycle audio track |
  | D-pad | Move focus | ←/→ seek 10 s, ↑/↓ volume |
  | LB / RB | Page up/down | Seek −/+ 60 s (or chapter) |
  | LT / RT (analog) | — | Hold-to-scrub, speed proportional to pressure |
  | Left stick | Move focus | Fine scrub |
  | Right stick ↑/↓ | — | Brightness |
  | Start | Settings | Pause + menu |
  | Select/Back | Toggle fullscreen | Toggle fullscreen |
- **Keyboard**: mpv-like defaults (Space, ←/→, ↑/↓, `f`, `j`/`s` subs, `#` audio, `Esc`), plus `Ctrl+P` / `:` for the **command palette**.
- **Mouse**: click/double-click to play, scroll for volume, click seek bar, right-click context menu, auto-hiding cursor.
- **Remapping**: Settings → Controls: pick an action, "press a key or button", conflict warning. Persisted to `~/.config/jimmy/bindings.toml` (also hand-editable).

### Command palette
- Fuzzy search over every action ("subtitle delay +", "toggle fullscreen", "go to Movies"), recently used at top, shows current bindings. Also searches files/folders in the browser.
- Fully operable by gamepad (on-screen list with filter by D-pad, A to run).

### Brightness (system backlight)
- Primary: GNOME's own brightness interface over D-Bus, so GNOME's slider stays in sync with jimmy.
- Fallback: systemd-logind `Session.SetBrightness` on the `/sys/class/backlight/*` device (no root needed for the active session).
- The exact GNOME interface has moved between GNOME versions, so this gets verified on the iMac early in the build. If no backlight device is exposed, the brightness controls hide themselves.
- jimmy draws its own brightness HUD (sharp bar + percentage).

### Volume (system volume, boost to 200%)
One volume control from 0% to 200%, split in two:
- **0–100%** sets the **system output volume** (PipeWire default sink, via libpulse). mpv stays at 100%.
- **100–200%** keeps the system at 100% and adds **software gain in mpv** (`volume-max=200`). Boost only affects jimmy, so other apps never get blasted, and nothing stays boosted after jimmy quits.
- The HUD shows the boost zone in a different color past 100%. A setting caps the max (100/150/200%) and decides whether boost resets when a new file starts.
- Changes made elsewhere (GNOME's slider, keyboard volume keys) are picked up and shown.
- Mute toggles the system mute.

### Keeping the screen awake
- While playing, jimmy blocks GNOME's screen blanking and suspend. The block is released on pause after a configurable delay.

### Design language
- Sharp corners, 1px borders, flat fills, no shadows/animations (or near-instant ones).
- High-contrast dark theme, one accent color, monospace or a compact sans (bundled font, so no fontconfig scan at startup).
- Large-ish text by default (it's a couch screen), scale factor setting.

---

## 5. Performance plan

### Startup (goal: window visible < 300 ms, browser populated < 500 ms on the iMac)
- No webview, no runtime font discovery (bundle one TTF), small binary (`opt-level=3`, LTO, `panic=abort`, stripped).
- Show the cached listing of the last directory immediately; rescan in the background.
- Create the mpv handle on a background thread while the window opens.

### Slow-disk optimizations
- Directory listing: one `read_dir`, filter by extension, **no per-file probing**. Durations/metadata are only filled in after a file has been played (or by a low-priority background job).
- Background jobs use **idle I/O priority** (`ioprio_set` IOPRIO_CLASS_IDLE) so they never fight with playback.
- mpv cache tuned for HDDs: large demuxer read-ahead (`demuxer-max-bytes=256MiB`, `demuxer-readahead-secs=60`, `cache=yes`), which turns many small seeks into fewer big sequential reads.
- Optional "Pre-buffer before start" (wait for N seconds of cache) for really slow / USB / network drives.
- Resume/state writes are batched and debounced (no disk writes every second).

### Low-end playback defaults (all overridable)
```
vo=libmpv (render API)   hwdec=auto-safe   profile=fast
scale=bilinear  dscale=bilinear  interpolation=no  deband=no
video-sync=audio  framedrop=vo
```
**Rescue mode** toggle (for HEVC/10-bit that the CPU can't handle):
`vd-lavc-skiploopfilter=all`, `vd-lavc-fast=yes`, `framedrop=decoder+vo`, optional downscale.

---

## 6. Project layout

```
jimmy/
├── Cargo.toml
├── src/
│   ├── main.rs            # setup, event loop
│   ├── app.rs             # app state, screen stack
│   ├── action.rs          # Action enum + metadata (name, category, description)
│   ├── input/
│   │   ├── mod.rs         # input → Action translation
│   │   ├── keyboard.rs
│   │   ├── gamepad.rs     # gilrs, analog triggers/sticks, repeat handling
│   │   └── bindings.rs    # load/save/remap bindings.toml
│   ├── player/
│   │   ├── mod.rs         # mpv handle, properties, events
│   │   └── render.rs      # mpv render context ↔ GL
│   ├── library/
│   │   ├── scan.rs        # async dir listing, caching, idle IO priority
│   │   ├── state.rs       # resume positions, recently played
│   │   └── provider.rs    # MetadataProvider trait (local only for now)
│   ├── ui/
│   │   ├── theme.rs       # sharp utilitarian style
│   │   ├── focus.rs       # gamepad/keyboard focus model
│   │   ├── browser.rs
│   │   ├── osd.rs         # player overlay (seek bar, title, tracks)
│   │   ├── palette.rs
│   │   └── settings.rs    # incl. remap screen
│   └── config.rs
├── assets/fonts/
└── mockups/               # HTML mockups (phase 2)
```

Config files: `~/.config/jimmy/{config.toml,bindings.toml}`, state in `~/.local/share/jimmy/`.

---

## 7. Future-proofing for online features (not built now)
- `MetadataProvider` trait (`fn lookup(&self, item) -> Option<Metadata>`) with only a `LocalProvider` (filename parsing: title/year/SxxEyy, sidecar `.nfo`/`poster.jpg`) in v1. TMDB/OpenSubtitles providers plug in later.
- Library items carry optional `title`, `year`, `poster_path`, `overview` fields from day one, so the browser can grow a poster view without a rewrite.
- No network crates in v1 → small binary, fast compile.

---

## 8. Phases

1. **Plan** (this document) — agree on UI library + open questions.
2. **Mockups (HTML)** — static, keyboard-navigable mockups of: Browser, Player OSD, Command palette, Track/subtitle menu, Settings → Controls remap. Iterate on look until it feels right.
3. **Build**
   1. Skeleton: winit + GL + egui, sharp theme, empty browser.
   2. mpv render integration: play a file in the window, fullscreen.
   3. Action system + keyboard bindings + palette.
   4. Browser: scanning, caching, filter, resume.
   5. OSD: seek bar, tracks, subtitles, volume/brightness.
   6. Gamepad (gilrs) + remapping UI.
   7. Settings screen, config persistence.
   8. Performance pass on the iMac (startup timing, idle CPU, HDD behavior).
4. **Test**
   - Unit tests: bindings parsing/conflicts, action dispatch, filename parsing, scanner.
   - Dev testing on this Mac (`brew install mpv`), real testing on the iMac.
   - A small test-media set: H.264/AAC MKV, HEVC 10-bit, MPEG-2 (DVD rip), AVI/XviD, VP9 WebM, files with SRT/ASS/PGS/EIA-608 captions.

### Building for the iMac
This Mac is Apple Silicon (arm64); the iMac is x86_64 Linux. Simplest path: **build on the iMac itself** (`cargo build --release`, first build is slow, incremental is fine). Alternative: an x86_64 Fedora container on this Mac for CI-style builds.

### Packaging: single executable (decided)
- jimmy ships as **one binary** that dynamically links the system **libmpv** (`mpv-libs` from RPM Fusion, already needed for codecs). Mesa/GL and the Wayland/X11 libraries come from the system as usual.
- All mpv calls live behind `src/player/`, so a fully self-contained build (mpv + FFmpeg compiled in, behind a `static-mpv` feature) can be added later without touching the rest of the app.
- If libmpv is missing at startup, jimmy prints a clear message with the `dnf` command instead of crashing.

### App menu / desktop integration
The binary embeds its own `.desktop` file and SVG icon and installs them itself, so it stays a single file:

```bash
jimmy --install     # per-user, no root
jimmy --uninstall
```

`--install` copies/links things to the standard per-user locations:

| What | Where |
|---|---|
| Binary | `~/.local/bin/jimmy` |
| Launcher | `~/.local/share/applications/jimmy.desktop` |
| Icon | `~/.local/share/icons/hicolor/scalable/apps/jimmy.svg` |

Then it refreshes the desktop and icon caches (`update-desktop-database`, `gtk-update-icon-cache`) if present. GNOME picks it up immediately: jimmy appears in the app grid and search, and can be pinned to the dash.

Launcher contents:
```ini
[Desktop Entry]
Type=Application
Name=jimmy
GenericName=Media Player
Comment=Lightweight media player
Exec=jimmy %F
Icon=jimmy
Terminal=false
Categories=AudioVideo;Video;Player;
Keywords=movie;video;player;mpv;
MimeType=video/x-matroska;video/mp4;video/x-msvideo;video/webm;video/mpeg;video/quicktime;video/x-flv;video/x-ms-wmv;video/ogg;video/3gpp;video/mp2t;
StartupWMClass=jimmy
StartupNotify=true
```

Details that matter:
- **Correct icon in the dash and Alt+Tab:** on Wayland the window's `app_id` must match the launcher's file name, so winit sets `app_id = "jimmy"` (and the X11 `WM_CLASS` too, matching `StartupWMClass`). Without this GNOME shows a generic gear icon.
- **"Open With jimmy" in Files:** `MimeType` makes jimmy show up there. `Exec=jimmy %F` passes the clicked files, which jimmy plays immediately (skipping the browser). Opening a folder starts the browser in that folder.
- **Default player:** optional, via `jimmy --install --default` (runs `xdg-mime default jimmy.desktop` for the video types) or GNOME Settings → Default Apps.
- **Icon:** one sharp, flat SVG in the jimmy style (amber block + lowercase wordmark), designed alongside the UI.
- Later, if wanted: a small RPM spec so it can be installed system-wide with `dnf`.

---

## 9. Open questions
All initial questions are answered (see "Decisions" at the top).
