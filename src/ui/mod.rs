//! The app: state, input → action dispatch, and the frame loop.

mod browser;
pub mod draw;
mod overlays;
mod player_view;
mod settings;

use crate::action::{Action, Context};
use crate::config::{Config, History, HistoryEntry};
use crate::input::{Bindings, KeyChord, PadEvent, PadInput, PadKind, Repeater};
use crate::library::{Entry, Scanner};
use crate::player::{DisplayHandle, GetProcAddress, Player, PlayerEvent, TrackKind};
use crate::system::System;
use crate::theme::{self, Palette};
use egui::{Color32, Rect};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Browser,
    Player,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Overlay {
    None,
    Menu,
    Palette,
    Tracks,
    Settings,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Dir(PathBuf),
    Continue,
    Recent,
}

/// One row in the browser.
#[derive(Clone, Debug)]
pub struct Item {
    pub entry: Entry,
    pub hist: Option<HistoryEntry>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hud {
    Volume,
    Brightness,
}

/// Command-line driven test steps (`--actions`), used for automated screenshots.
#[derive(Clone, Debug)]
pub enum Step {
    Act(Action),
    Wait(u64),
    Screenshot(PathBuf),
    Quit,
}

pub struct Startup {
    pub open: Option<PathBuf>,
    pub steps: Vec<Step>,
    pub fullscreen: bool,
}

pub struct App {
    pub(crate) cfg: Config,
    pub(crate) hist: History,
    pub(crate) binds: Bindings,
    pub(crate) pal: Palette,
    theme_idx: usize,

    scanner: Scanner,
    pub(crate) source: Source,
    all_items: Vec<Item>,
    pub(crate) items: Vec<Item>,
    pub(crate) loading: bool,
    pub(crate) list_error: Option<String>,
    pub(crate) filter: String,
    pub(crate) focus: usize,
    pub(crate) scroll: f32,
    pub(crate) scroll_focus: usize,
    /// Selection to restore when coming back up to a folder.
    remembered: Vec<(PathBuf, PathBuf)>,
    want_focus: Option<PathBuf>,

    pub(crate) screen: Screen,
    pub(crate) overlay: Overlay,

    pub(crate) player: Option<Player>,
    player_error: Option<String>,
    gpa: Option<GetProcAddress>,
    display: Option<DisplayHandle>,
    render_ready: bool,
    last_hist_write: Instant,
    playing_path: Option<PathBuf>,

    sys: System,
    /// Combined volume 0..=volume_max (100 = system max, above = boost inside jimmy).
    pub(crate) vol: f64,
    vol_touched: Instant,

    pad_rx: Receiver<PadEvent>,
    pub(crate) pad_kind: PadKind,
    pub(crate) pad_name: Option<String>,
    /// Hints follow whatever was used last.
    pub(crate) last_was_pad: bool,
    repeater: Repeater,
    held_pads: Vec<PadInput>,

    pub(crate) osd_until: Instant,
    hud: Option<(Hud, Instant)>,
    toast: Option<(String, Instant)>,
    pointer_moved: Instant,

    // overlay state
    pub(crate) menu_focus: usize,
    pub(crate) tracks_tab: usize,
    pub(crate) tracks_focus: usize,
    pub(crate) palette_query: String,
    pub(crate) palette_focus: usize,
    palette_from: Screen,
    pub(crate) settings_tab: usize,
    pub(crate) settings_focus: usize,
    pub(crate) settings_kb: bool,
    pub(crate) listening: Option<(Context, Action)>,

    steps: Vec<Step>,
    step_at: Instant,
    fullscreen: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        let ctx = cc.egui_ctx.clone();
        let cfg = Config::load();
        let hist = History::load();
        let theme_idx = theme::index_of(&cfg.theme);
        let pal = theme::palette(theme::id_at(theme_idx));
        theme::apply(&ctx, &pal);
        ctx.set_zoom_factor(cfg.ui_scale);
        setup_fonts(&ctx);

        let player = match Player::new(ctx.clone(), &cfg) {
            Ok(p) => Some(p),
            Err(e) => {
                log::error!("{e}");
                None
            }
        };
        let player_error = player.is_none().then(|| {
            "libmpv could not start. On Fedora: sudo dnf install mpv-libs (from RPM Fusion)".to_string()
        });

        let display = display_handle(cc);
        let mut app = Self {
            scanner: Scanner::new(ctx.clone()),
            source: Source::Continue,
            all_items: vec![],
            items: vec![],
            loading: false,
            list_error: None,
            filter: String::new(),
            focus: 0,
            scroll: 0.0,
            scroll_focus: usize::MAX,
            remembered: vec![],
            want_focus: None,
            screen: Screen::Browser,
            overlay: Overlay::None,
            player,
            player_error,
            gpa: cc.get_proc_address.clone(),
            display,
            render_ready: false,
            last_hist_write: Instant::now(),
            playing_path: None,
            sys: System::new(ctx.clone()),
            vol: 100.0,
            vol_touched: Instant::now() - Duration::from_secs(60),
            pad_rx: crate::input::spawn_gamepad_thread(ctx.clone(), 0.15),
            pad_kind: PadKind::Xbox,
            pad_name: None,
            last_was_pad: false,
            repeater: Repeater::default(),
            held_pads: vec![],
            osd_until: Instant::now(),
            hud: None,
            toast: None,
            pointer_moved: Instant::now(),
            menu_focus: 0,
            tracks_tab: 0,
            tracks_focus: 0,
            palette_query: String::new(),
            palette_focus: 0,
            palette_from: Screen::Browser,
            settings_tab: 0,
            settings_focus: 0,
            settings_kb: false,
            listening: None,
            steps: startup.steps,
            step_at: Instant::now(),
            fullscreen: startup.fullscreen,
            binds: Bindings::load(),
            theme_idx,
            pal,
            cfg,
            hist,
        };

        // Where to start: a folder/file from the command line, the last folder, or the first root.
        let start_dir = match &startup.open {
            Some(p) if p.is_dir() => Some(p.clone()),
            Some(p) => p.parent().map(Path::to_path_buf),
            None => app
                .hist
                .last_dir
                .clone()
                .filter(|d| d.is_dir())
                .or_else(|| app.cfg.roots.first().cloned()),
        };
        // For screenshots/testing without a controller: JIMMY_FAKE_PAD=xbox|ps
        if let Ok(k) = std::env::var("JIMMY_FAKE_PAD") {
            app.last_was_pad = true;
            app.pad_kind = if k == "ps" { PadKind::PlayStation } else { PadKind::Xbox };
        }
        if let Some(d) = start_dir {
            app.open_dir(d, None);
        } else {
            app.open_source(Source::Continue);
        }
        if let Some(p) = startup.open.filter(|p| p.is_file()) {
            app.want_focus = Some(p.clone());
            app.play(&p, false);
        }
        app
    }

    // ------------------------------------------------------------ theme

    pub(crate) fn set_theme(&mut self, ctx: &egui::Context, idx: usize) {
        self.theme_idx = idx % theme::count();
        self.pal = theme::palette(theme::id_at(self.theme_idx));
        theme::apply(ctx, &self.pal);
        self.cfg.theme = theme::id_at(self.theme_idx).to_string();
        self.cfg.save();
    }

    pub(crate) fn theme_idx(&self) -> usize {
        self.theme_idx
    }

    pub(crate) fn toast(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now() + Duration::from_millis(2500)));
    }

    // ------------------------------------------------------------ browsing

    pub(crate) fn open_dir(&mut self, dir: PathBuf, focus: Option<PathBuf>) {
        self.remember_focus();
        self.want_focus = focus.or_else(|| self.remembered.iter().find(|(d, _)| *d == dir).map(|(_, f)| f.clone()));
        self.hist.set_last_dir(&dir);
        self.scanner.request(&dir);
        self.loading = true;
        self.source = Source::Dir(dir);
        self.filter.clear();
    }

    pub(crate) fn open_source(&mut self, src: Source) {
        match src {
            Source::Dir(d) => self.open_dir(d, None),
            virt => {
                self.remember_focus();
                self.source = virt;
                self.filter.clear();
                self.rebuild_virtual();
            }
        }
    }

    fn remember_focus(&mut self) {
        if let (Source::Dir(d), Some(item)) = (&self.source, self.items.get(self.focus)) {
            self.remembered.retain(|(rd, _)| rd != d);
            self.remembered.push((d.clone(), item.entry.path.clone()));
        }
    }

    fn rebuild_virtual(&mut self) {
        let only_unfinished = self.source == Source::Continue;
        let entries: Vec<Item> = self
            .hist
            .recent(only_unfinished)
            .into_iter()
            .take(200)
            .map(|(path, h)| {
                let meta = std::fs::metadata(&path).ok();
                Item {
                    entry: Entry {
                        name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                        is_dir: false,
                        size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                        modified: meta.and_then(|m| m.modified().ok()),
                        subs: vec![],
                        path,
                    },
                    hist: Some(h.clone()),
                }
            })
            .collect();
        self.loading = false;
        self.list_error = None;
        self.set_items(entries);
    }

    fn set_items(&mut self, items: Vec<Item>) {
        let keep = self.want_focus.take().or_else(|| self.items.get(self.focus).map(|i| i.entry.path.clone()));
        self.all_items = items;
        self.apply_filter();
        self.focus = keep
            .and_then(|k| self.items.iter().position(|i| i.entry.path == k))
            .unwrap_or_else(|| self.first_unwatched());
        self.scroll = 0.0;
        self.scroll_focus = usize::MAX;
    }

    /// Default selection: the first thing not yet watched.
    fn first_unwatched(&self) -> usize {
        self.items
            .iter()
            .position(|i| !i.entry.is_dir && !i.hist.as_ref().is_some_and(|h| h.watched))
            .filter(|_| self.items.iter().all(|i| !i.entry.is_dir))
            .unwrap_or(0)
    }

    pub(crate) fn apply_filter(&mut self) {
        let f = self.filter.to_lowercase();
        let words: Vec<&str> = f.split_whitespace().collect();
        self.items = self
            .all_items
            .iter()
            .filter(|i| {
                let n = i.entry.name.to_lowercase();
                words.iter().all(|w| n.contains(w))
            })
            .cloned()
            .collect();
        if self.focus >= self.items.len() {
            self.focus = self.items.len().saturating_sub(1);
        }
    }

    fn refresh_hist_in_items(&mut self) {
        for list in [&mut self.all_items, &mut self.items] {
            for it in list.iter_mut() {
                it.hist = self.hist.get(&it.entry.path).cloned();
            }
        }
        if matches!(self.source, Source::Continue | Source::Recent) {
            self.rebuild_virtual();
        }
    }

    fn go_up(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.apply_filter();
            return;
        }
        match &self.source {
            Source::Dir(d) => {
                if let Some(parent) = d.parent() {
                    let child = d.clone();
                    self.open_dir(parent.to_path_buf(), Some(child));
                }
            }
            _ => {
                if let Some(d) = self.hist.last_dir.clone().or_else(|| self.cfg.roots.first().cloned()) {
                    self.open_dir(d, None);
                }
            }
        }
    }

    fn activate(&mut self, from_start: bool) {
        let Some(item) = self.items.get(self.focus).cloned() else { return };
        if item.entry.is_dir {
            self.open_dir(item.entry.path, None);
        } else {
            self.play(&item.entry.path, from_start);
        }
    }

    // ------------------------------------------------------------ playback

    pub(crate) fn play(&mut self, path: &Path, from_start: bool) {
        let Some(player) = self.player.as_mut() else {
            let msg = self.player_error.clone().unwrap_or_else(|| "playback unavailable".into());
            self.toast(msg);
            return;
        };
        let start = self
            .hist
            .get(path)
            .filter(|h| self.cfg.resume && !from_start && h.resumable())
            .map(|h| h.pos);
        player.load(path, start);
        self.playing_path = Some(path.to_path_buf());
        let e = self.hist.entry(path);
        e.last_played = crate::config::now_unix();
        if from_start {
            e.watched = false;
        }
        if self.cfg.reset_boost_on_new_file && self.vol > 100.0 {
            self.set_volume(100.0, false);
        }
        self.screen = Screen::Player;
        self.overlay = Overlay::None;
        self.osd_until = Instant::now() + Duration::from_millis(self.cfg.osd_timeout_ms);
        self.sys.inhibit(true);
    }

    fn record_position(&mut self) {
        let (Some(p), Some(path)) = (self.player.as_ref(), self.playing_path.clone()) else { return };
        if !p.s.loaded {
            return;
        }
        let (pos, dur) = (p.s.time_pos, p.s.duration);
        let e = self.hist.entry(&path);
        e.pos = pos;
        if dur > 0.0 {
            e.duration = dur;
            if pos / dur > 0.92 {
                e.watched = true;
            }
        }
        e.sub = p.s.selected(TrackKind::Sub).and_then(|t| t.lang.clone());
        e.aud = p.s.selected(TrackKind::Audio).and_then(|t| t.lang.clone());
    }

    pub(crate) fn stop(&mut self) {
        self.record_position();
        if let Some(p) = self.player.as_mut() {
            p.stop();
        }
        self.want_focus = self.playing_path.take();
        self.screen = Screen::Browser;
        self.overlay = Overlay::None;
        self.sys.inhibit(false);
        self.hist.save_if_dirty();
        self.refresh_hist_in_items();
        if let Some(f) = self.want_focus.take() {
            if let Some(i) = self.items.iter().position(|i| i.entry.path == f) {
                self.focus = i;
            }
        }
    }

    fn on_player_event(&mut self, ev: PlayerEvent) {
        match ev {
            PlayerEvent::FileLoaded => {
                if let (Some(p), Some(path)) = (self.player.as_ref(), self.playing_path.clone()) {
                    let d = p.s.duration;
                    if d > 0.0 {
                        self.hist.entry(&path).duration = d;
                    }
                }
            }
            PlayerEvent::Ended { eof } => {
                if self.screen != Screen::Player {
                    return;
                }
                if eof {
                    if let Some(path) = self.playing_path.clone() {
                        let e = self.hist.entry(&path);
                        e.watched = true;
                        e.pos = 0.0;
                    }
                    if let Some(p) = self.player.as_mut() {
                        p.stop();
                    }
                    self.want_focus = self.playing_path.take();
                    self.screen = Screen::Browser;
                    self.overlay = Overlay::None;
                    self.sys.inhibit(false);
                    self.hist.save_if_dirty();
                    self.refresh_hist_in_items();
                } else if let Some(err) = self.player.as_ref().and_then(|p| p.s.error.clone()) {
                    self.toast(err);
                    self.playing_path = None;
                    self.screen = Screen::Browser;
                    self.sys.inhibit(false);
                }
            }
        }
    }

    pub(crate) fn show_osd(&mut self) {
        self.osd_until = Instant::now() + Duration::from_millis(self.cfg.osd_timeout_ms);
    }

    // ------------------------------------------------------------ volume & brightness

    fn sys_volume_available(&self) -> bool {
        self.sys.snapshot().volume.is_some()
    }

    pub(crate) fn set_volume(&mut self, v: f64, show: bool) {
        let v = v.clamp(0.0, self.cfg.volume_max as f64).round();
        self.vol = v;
        self.vol_touched = Instant::now();
        let sys_ok = self.sys_volume_available();
        if sys_ok {
            self.sys.set_volume((v.min(100.0) / 100.0) as f32);
        }
        if let Some(p) = &self.player {
            p.set_volume(if sys_ok { v.max(100.0) } else { v });
        }
        if show {
            self.hud = Some((Hud::Volume, Instant::now() + Duration::from_millis(1500)));
        }
    }

    fn toggle_mute(&mut self) {
        let snap = self.sys.snapshot();
        if snap.volume.is_some() {
            self.sys.set_mute(!snap.muted);
            self.toast(if snap.muted { "Sound on" } else { "Muted" });
        } else if let Some(p) = &self.player {
            p.set_mute(!p.s.mute);
            self.toast(if p.s.mute { "Sound on" } else { "Muted" });
        }
    }

    pub(crate) fn is_muted(&self) -> bool {
        let snap = self.sys.snapshot();
        if snap.volume.is_some() { snap.muted } else { self.player.as_ref().is_some_and(|p| p.s.mute) }
    }

    fn change_brightness(&mut self, delta: f32) {
        match self.sys.snapshot().backlight {
            Some(b) => {
                self.sys.set_backlight(b + delta);
                self.hud = Some((Hud::Brightness, Instant::now() + Duration::from_millis(1500)));
            }
            None => self.toast("No adjustable backlight on this display"),
        }
    }

    /// Follow volume changes made outside jimmy (GNOME slider, media keys).
    fn sync_system(&mut self) {
        self.sys.refresh();
        if self.vol_touched.elapsed() < Duration::from_secs(3) {
            return;
        }
        if let Some(v) = self.sys.snapshot().volume {
            let boosted = self.vol > 100.0;
            if !boosted {
                self.vol = (v as f64 * 100.0).round();
            }
        } else if let Some(p) = &self.player {
            self.vol = p.s.volume;
        }
    }

    // ------------------------------------------------------------ input

    pub(crate) fn context(&self) -> Context {
        match self.overlay {
            Overlay::None => match self.screen {
                Screen::Browser => Context::Browser,
                Screen::Player => Context::Player,
            },
            _ => Context::List,
        }
    }

    fn collect_input(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut actions = Vec::new();
        let events = ctx.input(|i| i.events.clone());
        let text_mode = self.overlay == Overlay::Palette;
        for ev in events {
            match ev {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    self.last_was_pad = false;
                    let chord = KeyChord::from_event(key, modifiers);
                    if let Some((c, a)) = self.listening {
                        if self.settings_kb {
                            if key == egui::Key::Escape {
                                self.listening = None;
                            } else {
                                self.binds.clear_keys(c, a);
                                if let Some(lost) = self.binds.set_key(c, chord, a) {
                                    self.toast(format!("{} was also “{}”, now unbound", chord.label(), lost.name()));
                                }
                                self.binds.save();
                                self.listening = None;
                            }
                        }
                        continue;
                    }
                    if text_mode
                        && (chord.is_plain_text()
                            || matches!(key, egui::Key::Backspace | egui::Key::Delete | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Home | egui::Key::End))
                    {
                        continue;
                    }
                    if self.context() == Context::Browser {
                        if key == egui::Key::Backspace && !self.filter.is_empty() && !chord.ctrl {
                            self.filter.pop();
                            self.apply_filter();
                            continue;
                        }
                        if key == egui::Key::Escape && !self.filter.is_empty() {
                            self.filter.clear();
                            self.apply_filter();
                            continue;
                        }
                    }
                    let ctxt = self.context();
                    if let Some(a) = self.binds.key_action(ctxt, chord) {
                        // In the browser, plain letters go to the filter unless explicitly bound there.
                        if ctxt == Context::Browser
                            && chord.is_plain_text()
                            && self.binds.key_action(Context::Browser, chord).is_none()
                        {
                            continue;
                        }
                        actions.push(a);
                    }
                }
                egui::Event::Text(t) => {
                    if self.context() == Context::Browser && self.listening.is_none() {
                        let mods = ctx.input(|i| i.modifiers);
                        if !mods.ctrl && !mods.mac_cmd && !mods.alt {
                            let chord_bound = t.chars().count() == 1
                                && egui::Key::from_name(&t.to_uppercase())
                                    .is_some_and(|k| self.binds.key_action(Context::Browser, KeyChord::new(k)).is_some());
                            if !chord_bound && !(self.filter.is_empty() && t == " ") {
                                self.filter.push_str(&t);
                                self.apply_filter();
                                self.focus = 0;
                            }
                        }
                    }
                }
                egui::Event::PointerMoved(_) => self.pointer_moved = Instant::now(),
                _ => {}
            }
        }

        while let Ok(ev) = self.pad_rx.try_recv() {
            match ev {
                PadEvent::Connected { name, kind } => {
                    self.pad_kind = kind;
                    self.toast(format!("Controller connected: {name}"));
                    self.pad_name = Some(name);
                    self.last_was_pad = true;
                }
                PadEvent::Disconnected => {
                    self.pad_name = None;
                    self.last_was_pad = false;
                    self.repeater.clear();
                    self.held_pads.clear();
                    self.toast("Controller disconnected");
                }
                PadEvent::Down(p) => {
                    self.last_was_pad = true;
                    if let Some((c, a)) = self.listening {
                        if !self.settings_kb {
                            self.binds.clear_pads(c, a);
                            if let Some(lost) = self.binds.set_pad(c, p, a) {
                                self.toast(format!("{} was also “{}”, now unbound", p.label(self.pad_kind), lost.name()));
                            }
                            self.binds.save();
                            self.listening = None;
                        }
                        continue;
                    }
                    if let Some(a) = self.binds.pad_action(self.context(), p) {
                        actions.push(a);
                        if a.repeats() {
                            self.repeater.press(p);
                        }
                    }
                    self.held_pads.push(p);
                }
                PadEvent::Up(p) => {
                    self.repeater.release(p);
                    self.held_pads.retain(|h| *h != p);
                }
            }
        }
        let (due, next) = self.repeater.due();
        for p in due {
            match self.binds.pad_action(self.context(), p) {
                Some(a) if a.repeats() => actions.push(a),
                _ => self.repeater.release(p),
            }
        }
        if let Some(n) = next {
            ctx.request_repaint_after(n);
        }
        actions
    }

    pub(crate) fn dispatch(&mut self, ctx: &egui::Context, a: Action) {
        use Action::*;
        // Global actions first.
        match a {
            Fullscreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
                return;
            }
            Quit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                return;
            }
            NextTheme | PrevTheme => {
                let n = theme::count();
                let i = if a == NextTheme { self.theme_idx + 1 } else { self.theme_idx + n - 1 };
                self.set_theme(ctx, i);
                self.toast(format!("Color scheme: {}", theme::name_at(self.theme_idx)));
                return;
            }
            ToggleHints => {
                self.cfg.show_hints = !self.cfg.show_hints;
                self.cfg.save();
                return;
            }
            Palette => {
                if self.overlay == Overlay::Palette {
                    self.overlay = Overlay::None;
                } else {
                    self.palette_from = self.screen;
                    self.palette_query.clear();
                    self.palette_focus = 0;
                    self.overlay = Overlay::Palette;
                }
                return;
            }
            Settings => {
                self.overlay = Overlay::Settings;
                self.settings_focus = 0;
                return;
            }
            _ => {}
        }

        match self.overlay {
            Overlay::Menu => return self.menu_action(ctx, a),
            Overlay::Palette => return self.palette_action(ctx, a),
            Overlay::Tracks => return self.tracks_action(a),
            Overlay::Settings => return self.settings_action(ctx, a),
            Overlay::None => {}
        }

        match self.screen {
            Screen::Browser => self.browser_action(a),
            Screen::Player => self.player_action(a),
        }
    }

    fn browser_action(&mut self, a: Action) {
        use Action::*;
        let n = self.items.len();
        let page = 10;
        match a {
            Up => self.focus = self.focus.saturating_sub(1),
            Down => self.focus = (self.focus + 1).min(n.saturating_sub(1)),
            PageUp => self.focus = self.focus.saturating_sub(page),
            PageDown => self.focus = (self.focus + page).min(n.saturating_sub(1)),
            Home => self.focus = 0,
            End => self.focus = n.saturating_sub(1),
            Accept => self.activate(false),
            Options => self.activate(true),
            Right => {
                if self.items.get(self.focus).is_some_and(|i| i.entry.is_dir) {
                    self.activate(false);
                }
            }
            Back | Left => self.go_up(),
            Menu => {
                self.overlay = Overlay::Menu;
                self.menu_focus = self.menu_current();
            }
            _ => {}
        }
    }

    fn player_action(&mut self, a: Action) {
        use Action::*;
        let small = self.cfg.seek_small_secs;
        let large = self.cfg.seek_large_secs;
        let was_visible = self.osd_visible_now();
        if !matches!(a, VolumeUp | VolumeDown | BrightnessUp | BrightnessDown) {
            self.show_osd();
        }
        match a {
            // First press shows the controls, second goes back to the browser.
            Back => {
                if was_visible {
                    self.stop();
                }
                return;
            }
            Stop => return self.stop(),
            VolumeUp => return self.set_volume(self.vol + 5.0, true),
            VolumeDown => return self.set_volume(self.vol - 5.0, true),
            Mute => return self.toggle_mute(),
            BrightnessUp => return self.change_brightness(0.05),
            BrightnessDown => return self.change_brightness(-0.05),
            Tracks => {
                self.overlay = Overlay::Tracks;
                self.tracks_focus = 0;
                return;
            }
            Menu => {
                self.overlay = Overlay::Tracks;
                return;
            }
            RescueMode => {
                self.cfg.rescue_mode = !self.cfg.rescue_mode;
                self.cfg.save();
                if let Some(p) = &self.player {
                    p.set_rescue(self.cfg.rescue_mode);
                }
                self.toast(if self.cfg.rescue_mode { "Rescue mode on: faster, lower quality" } else { "Rescue mode off" });
                return;
            }
            _ => {}
        }
        let Some(p) = self.player.as_ref() else { return };
        match a {
            PlayPause | Accept => p.toggle_pause(),
            SeekBack | Left => p.seek_relative(-small),
            SeekForward | Right => p.seek_relative(small),
            SeekBackLarge => p.seek_relative(-large),
            SeekForwardLarge => p.seek_relative(large),
            PrevChapter => p.add_chapter(-1),
            NextChapter => p.add_chapter(1),
            SpeedUp => p.set_speed(p.s.speed + 0.25),
            SpeedDown => p.set_speed(p.s.speed - 0.25),
            SpeedReset => p.set_speed(1.0),
            NextSubtitle => p.cycle_track(TrackKind::Sub),
            SubtitlesOff => p.select_track(TrackKind::Sub, None),
            NextAudio => p.cycle_track(TrackKind::Audio),
            SubDelayUp => p.add("sub-delay", 0.1),
            SubDelayDown => p.add("sub-delay", -0.1),
            SubSizeUp => p.add("sub-scale", 0.1),
            SubSizeDown => p.add("sub-scale", -0.1),
            _ => {}
        }
    }

    pub(crate) fn osd_visible_now(&self) -> bool {
        let paused = self.player.as_ref().is_some_and(|p| p.s.paused);
        paused || Instant::now() < self.osd_until || self.overlay != Overlay::None
    }

    // ------------------------------------------------------------ hints

    /// Key cap label + color for an action in a context, following the last-used device.
    pub(crate) fn glyph(&self, ctx: Context, a: Action) -> Option<(String, Option<Color32>)> {
        if self.last_was_pad {
            self.binds.pad_for(ctx, a).map(|p| (p.label(self.pad_kind).to_string(), p.color(self.pad_kind)))
        } else {
            self.binds.key_for(ctx, a).map(|k| (k.label(), None))
        }
    }

    /// Draws a row of `[cap] label` hints starting at `pos` (left-center). Clicking a hint runs it.
    pub(crate) fn hints(&mut self, ui: &egui::Ui, pos: egui::Pos2, ctx_: Context, list: &[(Action, &str)], size: f32, gap: f32, fg: Color32, cap_bg: Color32) -> Vec<Action> {
        let p = ui.painter();
        let mut x = pos.x;
        let mut clicked = Vec::new();
        for (a, label) in list {
            let Some((cap, color)) = self.glyph(ctx_, *a) else { continue };
            let r = draw::chip(p, egui::pos2(x, pos.y), &cap, size * 0.85, color.unwrap_or(fg), cap_bg);
            let t = draw::text(p, egui::pos2(r.right() + size * 0.45, pos.y), egui::Align2::LEFT_CENTER, label, size, fg);
            let hit = r.union(t);
            if ui.interact(hit, ui.scope_id().with(("hint", *a, label)), egui::Sense::click()).clicked() {
                clicked.push(*a);
            }
            x = t.right() + gap;
        }
        clicked
    }

    // ------------------------------------------------------------ test steps

    fn run_steps(&mut self, ctx: &egui::Context) -> Vec<Action> {
        let mut out = Vec::new();
        while let Some(step) = self.steps.first().cloned() {
            if Instant::now() < self.step_at {
                ctx.request_repaint_after(self.step_at - Instant::now());
                break;
            }
            self.steps.remove(0);
            match step {
                Step::Act(a) => {
                    out.push(a);
                    self.step_at = Instant::now() + Duration::from_millis(120);
                    ctx.request_repaint_after(Duration::from_millis(120));
                    break;
                }
                Step::Wait(ms) => {
                    self.step_at = Instant::now() + Duration::from_millis(ms);
                    ctx.request_repaint_after(Duration::from_millis(ms));
                    break;
                }
                Step::Screenshot(path) => {
                    let cb = egui::ScreenshotCallback::new(move |img: std::sync::Arc<egui::ColorImage>| {
                        if let Err(e) = save_png(&path, &img) {
                            eprintln!("screenshot: {e}");
                        } else {
                            eprintln!("saved {}", path.display());
                        }
                    });
                    ctx.send_viewport_cmd(egui::ViewportCommand::Screenshot(cb));
                    self.step_at = Instant::now() + Duration::from_millis(400);
                    ctx.request_repaint_after(Duration::from_millis(400));
                    break;
                }
                Step::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            }
        }
        out
    }
}

fn save_png(path: &Path, img: &egui::ColorImage) -> anyhow::Result<()> {
    let file = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(file, img.size[0] as u32, img.size[1] as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut w = enc.write_header()?;
    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| c.to_srgba_unmultiplied()).collect();
    w.write_image_data(&bytes)?;
    Ok(())
}

fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    // One monospace look everywhere (Hack ships with egui).
    if let Some(prop) = fonts.families.get_mut(&egui::FontFamily::Proportional) {
        prop.insert(0, "Hack".to_owned());
    }
    ctx.set_fonts(fonts);
}

#[allow(unused_variables)]
fn display_handle(cc: &eframe::CreationContext<'_>) -> Option<DisplayHandle> {
    #[cfg(target_os = "linux")]
    {
        use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
        if let Ok(h) = cc.display_handle() {
            match h.as_raw() {
                RawDisplayHandle::Wayland(w) => return Some(DisplayHandle::Wayland(w.display.as_ptr() as *const _)),
                RawDisplayHandle::Xlib(x) => {
                    return x.display.map(|d| DisplayHandle::X11(d.as_ptr() as *const _));
                }
                _ => {}
            }
        }
    }
    None
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        if !self.render_ready {
            self.render_ready = true;
            if let (Some(p), Some(gpa)) = (self.player.as_mut(), self.gpa.clone()) {
                if let Err(e) = p.init_render(gpa, self.display) {
                    log::error!("{e}");
                    self.player_error = Some(e.to_string());
                }
            }
            if self.fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            }
        }

        if let Some(l) = self.scanner.poll() {
            if Source::Dir(l.dir.clone()) == self.source {
                self.loading = false;
                self.list_error = l.error.clone();
                let items = l
                    .entries
                    .into_iter()
                    .map(|e| {
                        let hist = self.hist.get(&e.path).cloned();
                        Item { entry: e, hist }
                    })
                    .collect();
                self.set_items(items);
            }
        }

        let events = self.player.as_mut().map(|p| p.poll()).unwrap_or_default();
        for ev in events {
            self.on_player_event(ev);
        }
        self.sync_system();

        let mut actions = self.collect_input(&ctx);
        actions.extend(self.run_steps(&ctx));
        for a in actions {
            self.dispatch(&ctx, a);
        }

        // Periodically remember the playback position (in memory every frame-ish, on disk every 30 s).
        if self.screen == Screen::Player {
            self.record_position();
            if self.last_hist_write.elapsed() > Duration::from_secs(30) {
                self.last_hist_write = Instant::now();
                self.hist.save_if_dirty();
            }
            let paused = self.player.as_ref().is_some_and(|p| p.s.paused);
            self.sys.inhibit(!paused);
        }

        let rect = ui.max_rect();
        let mut clicked = Vec::new();
        match self.screen {
            Screen::Browser => clicked.extend(self.draw_browser(ui, rect)),
            Screen::Player => clicked.extend(self.draw_player(ui, rect)),
        }
        match self.overlay {
            Overlay::None => {}
            Overlay::Menu => clicked.extend(self.draw_menu(ui, rect)),
            Overlay::Palette => clicked.extend(self.draw_palette(ui, rect)),
            Overlay::Tracks => clicked.extend(self.draw_tracks(ui, rect)),
            Overlay::Settings => clicked.extend(self.draw_settings(ui, rect)),
        }
        self.draw_toast(ui, rect);
        for a in clicked {
            self.dispatch(&ctx, a);
        }

        // Wake up for timers (OSD hide, HUD, toast).
        let now = Instant::now();
        for t in [Some(self.osd_until), self.hud.map(|h| h.1), self.toast.as_ref().map(|t| t.1)].into_iter().flatten() {
            if t > now {
                ctx.request_repaint_after(t - now + Duration::from_millis(20));
            }
        }
        // Clock in the corner.
        if self.screen == Screen::Browser {
            ctx.request_repaint_after(Duration::from_secs(20));
        }
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.record_position();
        self.hist.save_if_dirty();
        self.sys.inhibit(false);
        if let Some(p) = self.player.as_mut() {
            p.shutdown();
        }
    }
}

impl App {
    fn draw_toast(&mut self, ui: &egui::Ui, rect: Rect) {
        let Some((msg, until)) = self.toast.clone() else { return };
        if Instant::now() > until {
            self.toast = None;
            return;
        }
        let p = ui.painter();
        let size = 14.0;
        let w = draw::measure(p, &msg, size).x + 28.0;
        let r = Rect::from_center_size(egui::pos2(rect.center().x, rect.top() + 70.0), egui::vec2(w, 32.0));
        let (bg, fg) = if self.screen == Screen::Player { (draw::black(0.7), draw::white(0.75)) } else { (self.pal.raise, self.pal.text) };
        p.rect_filled(r, 0.0, bg);
        draw::text(p, r.center(), egui::Align2::CENTER_CENTER, msg, size, fg);
    }

    pub(crate) fn hud_state(&mut self) -> Option<(bool, f32)> {
        let (kind, until) = self.hud?;
        if Instant::now() > until {
            self.hud = None;
            return None;
        }
        Some(match kind {
            Hud::Volume => (true, self.vol as f32),
            Hud::Brightness => (false, self.sys.snapshot().backlight.unwrap_or(0.0) * 100.0),
        })
    }

    pub(crate) fn system_backlight(&self) -> Option<f32> {
        self.sys.snapshot().backlight
    }
}
