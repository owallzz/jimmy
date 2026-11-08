//! The app: state, input → action dispatch, and the frame loop.

mod browser;
pub mod draw;
mod grid;
mod osk;
mod options;
mod overlays;
mod player_view;
mod library_ui;
mod playlists;
mod settings;
mod subs_ui;

use crate::action::{Action, Context};
use crate::config::{Config, History, HistoryEntry};
use crate::input::{Bindings, KeyChord, PadEvent, PadInput, PadKind, Repeater};
use crate::library::{Entry, Scanner};
use crate::meta::{MetaCache, ProbeResult, Prober};
use std::collections::HashSet;
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
    /// "Add to playlist" picker.
    AddToPlaylist,
    /// Fix Match: search TVmaze (shows) or Wikipedia (movies) and pick the right one.
    FixMatch,
    /// "Movies or TV Shows?" when adding a library folder.
    LibraryKind,
    /// OpenSubtitles results for the playing video.
    SubDownload,
    /// The selected row's Options menu.
    Options,
}

/// Fix Match panel state.
#[derive(Clone, Debug, Default)]
pub(crate) struct FixMatch {
    /// Database key of the show/movie being fixed.
    pub key: String,
    pub show: bool,
    pub query: String,
    /// Query of the search in flight / last finished.
    pub searched: String,
    pub results: Vec<crate::scrape::Candidate>,
    pub error: Option<String>,
    pub busy: bool,
    /// 0 = the search box, then results.
    pub focus: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Source {
    Dir(PathBuf),
    /// "My media": everything in every library folder.
    All,
    Continue,
    Recent,
    /// Everything in the library that isn't an episode.
    Movies,
    /// One row per show.
    Shows,
    /// A show's episodes, one season at a time (key from `media::show_key`).
    Show(String),
    /// jimmy's playlists plus any found in library folders.
    Playlists,
    /// One playlist's entries.
    Playlist(PathBuf),
    /// Newest files in the libraries.
    Added,
}

impl Source {
    /// Views built from the "My media" scan.
    pub fn from_catalog(&self) -> bool {
        matches!(self, Source::Movies | Source::Shows | Source::Show(_))
    }
}

/// A show as one browser row (TV shows tab, My media).
pub(crate) fn show_item(show: &crate::media::Show) -> Item {
    let entry = Entry {
        name: show.name.clone(),
        path: show_path(&show.key),
        is_dir: true,
        size: show.episodes.iter().map(|(_, e)| e.size).sum(),
        modified: show.episodes.iter().filter_map(|(_, e)| e.modified).max(),
        subs: vec![],
    };
    Item { entry, hist: None, meta: None, show: Some(show.key.clone()), ep: None }
}

/// Stand-in path for a show row (rows are keyed by path).
pub(crate) fn show_path(key: &str) -> PathBuf {
    PathBuf::from(format!("jimmy:show/{key}"))
}

/// One row in the browser.
#[derive(Clone, Debug)]
pub struct Item {
    pub entry: Entry,
    pub hist: Option<HistoryEntry>,
    /// Probed metadata (duration, codecs), once known.
    pub meta: Option<crate::meta::Meta>,
    /// Set on show rows (TV shows tab): the show's key.
    pub show: Option<String>,
    /// Set when the file name looks like an episode.
    pub ep: Option<crate::media::Episode>,
}

impl Item {
    /// Best known length: from playback, else from probing.
    pub fn duration(&self) -> f64 {
        self.hist.as_ref().map(|h| h.duration).filter(|d| *d > 0.0).or_else(|| self.meta.as_ref().map(|m| m.duration).filter(|d| *d > 0.0)).unwrap_or(0.0)
    }
}

/// Holding the Subs button: after a moment a picker opens; releasing selects.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SubHold {
    pub input: PadInput,
    pub since: Instant,
    pub open: bool,
    pub idx: usize,
}

pub(crate) const HOLD_DELAY: Duration = Duration::from_millis(300);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Hud {
    Volume,
    Brightness,
}

/// Command-line driven test steps (`--actions`), used for automated screenshots.
#[derive(Clone, Debug)]
pub enum Step {
    Act(Action),
    /// A gamepad button press, e.g. `pad:a`, `pad:dpad_down`.
    Pad(PadInput),
    /// Press or release separately, to test holds: `pad_down:x`, `pad_up:x`.
    PadDown(PadInput),
    PadUp(PadInput),
    /// A real key press, e.g. `key:j` or `key:Shift+G`.
    Key(KeyChord),
    /// Typed text, e.g. `type:moon`.
    Type(String),
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
    /// Typing goes into the filter (entered with `/`).
    pub(crate) filtering: bool,
    /// Browsing the whole computer (from "Browse computer…"): Back always goes up a folder.
    pub(crate) exploring: bool,
    /// On-screen keyboard for controllers.
    pub(crate) osk: Option<osk::Osk>,
    pub(crate) sub_hold: Option<SubHold>,
    fake_pad: Vec<PadEvent>,
    pending_g: Option<Instant>,
    count: Option<u32>,
    pub(crate) focus: usize,
    pub(crate) scroll: f32,
    pub(crate) scroll_focus: usize,
    /// Selection to restore when coming back up to a folder.
    remembered: Vec<(PathBuf, PathBuf)>,
    /// Last "My media" listing, shown instantly while a fresh scan runs.
    pub(crate) all_cache: Option<Vec<Entry>>,
    /// My media split into shows and movies (rebuilt with each scan).
    pub(crate) catalog: crate::media::Catalog,
    /// Season shown on a show's page.
    pub(crate) season: u32,
    /// Where a show's page was opened from, for Back.
    show_return: Option<Source>,
    /// Options menu entries and selection.
    pub(crate) opt_list: Vec<(String, options::Opt)>,
    pub(crate) opt_focus: usize,
    /// Files waiting for the "Add to playlist" picker, and its selection.
    pub(crate) pl_files: Vec<PathBuf>,
    pub(crate) pl_focus: usize,
    /// Columns in the poster grid (last frame), for up/down moves.
    pub(crate) grid_cols: usize,
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
    /// What plays next: a show's episodes, a playlist, or just the one file.
    pub(crate) queue: Vec<PathBuf>,
    pub(crate) queue_idx: usize,
    /// Shown in the player ("Breaking Bad", "Road trip"); empty for a lone file.
    pub(crate) queue_name: String,
    /// "Up next" countdown: the next item starts at this instant.
    pub(crate) up_next: Option<Instant>,
    /// The video ended and the player stayed open (exit-on-end off, or nothing next).
    pub(crate) at_end: bool,
    /// Settings → Clear watch history needs a second press.
    pub(crate) confirm_clear: Option<Instant>,

    sys: System,
    /// The matched library: shows, movies, files, artwork (see libdb.rs).
    pub(crate) db: crate::libdb::Db,
    scraper: crate::scrape::Scraper,
    /// The last library scan, by path (for building rows from the database).
    entry_map: std::collections::HashMap<PathBuf, Entry>,
    last_scan: Instant,
    /// Fix Match panel state.
    pub(crate) fix: Option<FixMatch>,
    /// Folder waiting for "Movies or TV Shows?" when adding a library.
    pub(crate) lib_pending: Option<PathBuf>,
    pub(crate) lib_focus: usize,
    /// Subtitle download panel state.
    pub(crate) sub_dl: Option<subs_ui::SubDl>,
    last_scrape_write: Instant,
    /// Poster textures, most recently used last (a handful at most).
    posters: Vec<(PathBuf, egui::TextureHandle)>,
    meta_cache: MetaCache,
    prober: Prober,
    /// File waiting out the hover pause before it gets probed.
    probe_wait: Option<(PathBuf, Instant)>,
    pub(crate) probe_requested: HashSet<PathBuf>,
    pub(crate) probe_failed: HashSet<PathBuf>,
    last_meta_write: Instant,
    /// Row under the mouse in the browser (last frame).
    pub(crate) hover_item: Option<usize>,
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
    startup_play: Option<PathBuf>,
    injected: Vec<egui::Event>,
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
        draw::set_clock_24h(cfg.clock_24h);
        setup_fonts(&ctx);

        let no_mpv = std::env::var_os("JIMMY_BENCH_NOMPV").is_some(); // benchmark switch
        let player = match if no_mpv { Err(anyhow::anyhow!("disabled")) } else { Player::new(ctx.clone(), &cfg) } {
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
            filtering: false,
            exploring: false,
            osk: None,
            sub_hold: None,
            fake_pad: vec![],
            pending_g: None,
            count: None,
            focus: 0,
            scroll: 0.0,
            scroll_focus: usize::MAX,
            remembered: vec![],
            all_cache: None,
            catalog: Default::default(),
            season: 1,
            show_return: None,
            opt_list: vec![],
            opt_focus: 0,
            pl_files: vec![],
            pl_focus: 0,
            grid_cols: 1,
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
            queue: vec![],
            queue_idx: 0,
            queue_name: String::new(),
            up_next: None,
            at_end: false,
            confirm_clear: None,
            sys: System::new(ctx.clone()),
            db: crate::libdb::Db::load(),
            scraper: crate::scrape::Scraper::new(ctx.clone()),
            entry_map: Default::default(),
            last_scan: Instant::now(),
            fix: None,
            lib_pending: None,
            lib_focus: 0,
            sub_dl: None,
            last_scrape_write: Instant::now(),
            posters: vec![],
            meta_cache: MetaCache::load(),
            prober: Prober::new(ctx.clone()),
            probe_wait: None,
            probe_requested: HashSet::new(),
            probe_failed: HashSet::new(),
            last_meta_write: Instant::now(),
            hover_item: None,
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
            tracks_tab: 1,
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
            startup_play: None,
            injected: vec![],
            fullscreen: startup.fullscreen,
            binds: Bindings::load(),
            theme_idx,
            pal,
            cfg,
            hist,
        };

        // Where to start: a folder/file from the command line, the last folder, or the first root.
        // Start on "My media", unless a file or folder was passed on the command line.
        let start_dir = match &startup.open {
            Some(p) if p.is_dir() => Some(p.clone()),
            Some(p) => p.parent().map(Path::to_path_buf),
            None => None,
        };
        // For screenshots/testing without a controller: JIMMY_FAKE_PAD=xbox|ps
        if let Ok(k) = std::env::var("JIMMY_FAKE_PAD") {
            app.last_was_pad = true;
            app.pad_kind = if k == "ps" { PadKind::PlayStation } else { PadKind::Xbox };
        }
        match start_dir {
            Some(d) => app.open_dir(d, None),
            None => app.open_source(Source::All),
        }
        // Played on the first frame, once mpv's video renderer exists (loading earlier can
        // leave mpv without video output).
        if let Some(p) = startup.open.filter(|p| p.is_file()) {
            app.want_focus = Some(p.clone());
            app.startup_play = Some(p);
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
        self.filtering = false;
    }

    pub(crate) fn open_source(&mut self, src: Source) {
        self.exploring = false;
        match src {
            Source::Dir(d) => self.open_dir(d, None),
            Source::All => {
                self.remember_focus();
                self.source = Source::All;
                self.filter.clear();
                self.list_error = None;
                self.loading = true;
                self.scanner.request_all(&self.cfg.roots);
                let cached = self.all_cache.clone();
                if let Some(entries) = cached {
                    let items = self.my_media_rows(entries);
                    self.set_items(items);
                } else {
                    self.set_items(vec![]);
                }
            }
            Source::Added => {
                self.remember_focus();
                self.source = src;
                self.filter.clear();
                self.list_error = None;
                if self.all_cache.is_none() {
                    self.loading = true;
                    self.scanner.request_all(&self.cfg.roots);
                }
                self.rebuild_added_view();
            }
            Source::Playlists | Source::Playlist(_) => {
                self.remember_focus();
                self.source = src;
                self.filter.clear();
                self.list_error = None;
                self.loading = false;
                self.rebuild_playlist_view();
            }
            cat if cat.from_catalog() => {
                self.remember_focus();
                if let Source::Show(key) = &cat {
                    if let Some(show) = self.catalog.show(key) {
                        let i = self.next_up(show);
                        self.season = show.episodes[i].0.season;
                        self.want_focus = Some(show.episodes[i].1.path.clone());
                    }
                }
                self.source = cat;
                self.filter.clear();
                self.list_error = None;
                if self.all_cache.is_none() {
                    self.loading = true;
                    self.scanner.request_all(&self.cfg.roots);
                }
                self.rebuild_catalog_view();
            }
            virt => {
                self.remember_focus();
                self.source = virt;
                self.filter.clear();
                self.rebuild_virtual();
            }
        }
    }

    /// Every playlist: jimmy's own first, then any found in library folders.
    pub(crate) fn all_playlists(&self) -> Vec<PathBuf> {
        let mut v = crate::playlist::list_own();
        if let Some(c) = &self.all_cache {
            v.extend(c.iter().filter(|e| crate::playlist::is_playlist(&e.path)).map(|e| e.path.clone()));
        }
        v
    }

    pub(crate) fn rebuild_playlist_view(&mut self) {
        let entry_for = |path: PathBuf| {
            let meta = std::fs::metadata(&path).ok();
            Entry {
                name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                is_dir: false,
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified: meta.and_then(|m| m.modified().ok()),
                subs: vec![],
                path,
            }
        };
        let items: Vec<Item> = match self.source.clone() {
            Source::Playlists => self.all_playlists().into_iter().map(|p| self.item(entry_for(p))).collect(),
            Source::Playlist(p) => crate::playlist::read(&p).into_iter().map(|f| self.item(entry_for(f))).collect(),
            _ => return,
        };
        self.set_items(items);
    }

    /// Add to playlist (P / RS): a file, every episode of a show row, or a playlist's entries.
    pub(crate) fn open_add_to_playlist(&mut self, files: Vec<PathBuf>) {
        if files.is_empty() {
            self.toast("Nothing to add here");
            return;
        }
        self.pl_files = files;
        self.pl_focus = 0;
        self.overlay = Overlay::AddToPlaylist;
    }

    /// What Add to playlist adds for the selected row.
    fn files_for_playlist(&self) -> Vec<PathBuf> {
        let Some(item) = self.items.get(self.focus) else { return vec![] };
        if let Some(key) = &item.show {
            return self.catalog.show(key).map(|s| s.episodes.iter().map(|(_, f)| f.path.clone()).collect()).unwrap_or_default();
        }
        if crate::playlist::is_playlist(&item.entry.path) {
            return crate::playlist::read(&item.entry.path);
        }
        if item.entry.is_dir {
            return vec![];
        }
        vec![item.entry.path.clone()]
    }

    pub(crate) fn add_to_playlist(&mut self, target: Option<PathBuf>) {
        let files = std::mem::take(&mut self.pl_files);
        self.overlay = Overlay::None;
        let target = target.unwrap_or_else(|| {
            // Name a new playlist after the show, or the folder the first file is in.
            let base = files
                .first()
                .and_then(|f| self.episode_of(f).map(|e| e.show).or_else(|| f.parent().and_then(|d| d.file_name()).map(|n| n.to_string_lossy().into_owned())))
                .unwrap_or_else(|| "Playlist".into());
            crate::playlist::new_path(&base)
        });
        match crate::playlist::add(&target, &files) {
            Ok(n) => {
                let what = if n == 1 { "1 video".to_string() } else { format!("{n} videos") };
                self.toast(format!("Added {what} to {}", crate::playlist::name(&target)));
            }
            Err(e) => self.toast(format!("Couldn't save playlist: {e}")),
        }
        if matches!(self.source, Source::Playlists | Source::Playlist(_)) {
            self.rebuild_playlist_view();
        }
    }

    /// My media: movies and other videos as they are, but each TV show as one row (opening
    /// it goes to the show's page). Sorted by name.
    pub(crate) fn my_media_rows(&self, entries: Vec<Entry>) -> Vec<Item> {
        let mut shown = std::collections::HashSet::new();
        let mut rows: Vec<Item> = Vec::new();
        for e in entries {
            if crate::playlist::is_playlist(&e.path) {
                continue;
            }
            let rec = self.db.file(&e.path);
            let show_key = rec.filter(|f| f.ep.is_some()).map(|f| f.item.clone());
            match show_key.and_then(|k| self.catalog.show(&k)) {
                Some(show) => {
                    if shown.insert(show.key.clone()) {
                        rows.push(show_item(show));
                    }
                }
                // A split movie (cd1/cd2) is one row: its first part.
                None => match rec.filter(|f| f.part.is_some()) {
                    Some(f) => {
                        if shown.insert(f.item.clone()) {
                            let first = self.db.movie_parts(&e.path).into_iter().next().unwrap_or(e.path.clone());
                            let entry = self.entry_map.get(&first).cloned().unwrap_or(e);
                            rows.push(self.item(entry));
                        }
                    }
                    None => rows.push(self.item(e)),
                },
            }
        }
        rows.sort_by_cached_key(|r| {
            let name = if r.show.is_some() { r.entry.name.clone() } else { self.row_name(r) };
            crate::library::natural_key(name.trim_start_matches("The "))
        });
        rows
    }

    /// The episode to offer next: the one in progress, else the first unwatched one after the
    /// last one played, else the first unwatched, else the first.
    pub(crate) fn next_up(&self, show: &crate::media::Show) -> usize {
        let hist = |i: usize| self.hist.get(&show.episodes[i].1.path);
        let last = (0..show.episodes.len()).filter(|&i| hist(i).is_some_and(|h| h.last_played > 0)).max_by_key(|&i| hist(i).map(|h| h.last_played));
        // Specials (season 0) only come up once the regular seasons are all watched.
        let regular = |i: usize| show.episodes[i].0.season > 0;
        let unwatched = |from: usize| {
            (from..show.episodes.len())
                .find(|&i| regular(i) && !hist(i).is_some_and(|h| h.watched))
                .or_else(|| (from..show.episodes.len()).find(|&i| !hist(i).is_some_and(|h| h.watched)))
        };
        let first_regular = (0..show.episodes.len()).find(|&i| regular(i)).unwrap_or(0);
        match last {
            Some(i) if hist(i).is_some_and(|h| h.resumable()) => i,
            Some(i) => unwatched(i + 1).or_else(|| unwatched(0)).unwrap_or(first_regular),
            None => unwatched(0).unwrap_or(first_regular),
        }
    }

    /// Rows for Movies, TV shows and a show's page, from the last My media scan.
    pub(crate) fn rebuild_catalog_view(&mut self) {
        let items: Vec<Item> = match self.source.clone() {
            Source::Movies => self.catalog.movies.clone().into_iter().map(|e| self.item(e)).collect(),
            Source::Shows => self.catalog.shows.iter().map(show_item).collect(),
            Source::Show(key) => {
                let Some(show) = self.catalog.show(&key).cloned() else {
                    self.set_items(vec![]);
                    return;
                };
                if !show.seasons().contains(&self.season) {
                    self.season = show.seasons().first().copied().unwrap_or(1);
                }
                show.episodes.into_iter().filter(|(e, _)| e.season == self.season).map(|(_, f)| self.item(f)).collect()
            }
            _ => return,
        };
        if self.all_cache.is_some() {
            self.loading = false;
        }
        self.set_items(items);
    }

    /// Season tabs on a show's page: move by `d`. Returns false at either end.
    fn change_season(&mut self, d: i32) -> bool {
        let Source::Show(key) = &self.source else { return false };
        let Some(show) = self.catalog.show(key) else { return false };
        let seasons = show.seasons();
        let i = seasons.iter().position(|s| *s == self.season).unwrap_or(0) as i32 + d;
        if i < 0 || i >= seasons.len() as i32 {
            return false;
        }
        self.season = seasons[i as usize];
        self.focus = 0;
        self.rebuild_catalog_view();
        self.focus = self.first_unwatched();
        true
    }

    /// "Browse computer…": the file explorer, starting at the home folder.
    pub(crate) fn browse_computer(&mut self) {
        let home = directories::UserDirs::new().map(|u| u.home_dir().to_path_buf()).unwrap_or_else(|| PathBuf::from("/"));
        self.open_dir(home, None);
        self.exploring = true;
    }

    /// Add a folder to the library, or remove it if it's already there.
    pub(crate) fn toggle_library(&mut self, dir: &Path) {
        if self.cfg.roots.iter().any(|r| r == dir) {
            self.cfg.remove_library(dir);
            self.toast(format!("Removed {} from your library", dir.display()));
            self.library_changed();
        } else {
            // Ask what it holds first (Overlay::LibraryKind).
            self.lib_pending = Some(dir.to_path_buf());
            self.lib_focus = if crate::config::guess_kind(dir) == crate::config::LibraryKind::Shows { 1 } else { 0 };
            self.overlay = Overlay::LibraryKind;
        }
    }

    pub(crate) fn add_library(&mut self, dir: &Path, kind: crate::config::LibraryKind) {
        self.cfg.add_library(dir, kind);
        self.toast(format!("Added {} as {}", dir.display(), kind.label()));
        self.library_changed();
    }

    /// Library folders changed: rescan everything.
    pub(crate) fn library_changed(&mut self) {
        self.all_cache = None;
        self.scanner.request_all(&self.cfg.roots);
        self.loading = true;
    }

    fn item(&self, entry: Entry) -> Item {
        let hist = self.hist.get(&entry.path).cloned();
        let meta = self.meta_cache.get(&entry.path, entry.size, entry.modified).cloned();
        let ep = if entry.is_dir { None } else { self.episode_of(&entry.path) };
        Item { entry, hist, meta, show: None, ep }
    }

    /// Only files inside the user's library folders get probed.
    pub(crate) fn in_library(&self, path: &Path) -> bool {
        self.cfg.roots.iter().any(|r| path.starts_with(r))
    }

    /// Probe the file the user is lingering on (selected or under the mouse) after a short pause.
    fn schedule_probe(&mut self, ctx: &egui::Context) {
        const PAUSE: Duration = Duration::from_millis(400);
        if self.screen != Screen::Browser {
            self.probe_wait = None;
            return;
        }
        // The selected item first (controller/keyboard), then whatever the mouse rests on.
        let needs = |i: &&Item| {
            !i.entry.is_dir && i.meta.is_none() && !self.probe_requested.contains(&i.entry.path) && self.in_library(&i.entry.path)
        };
        let candidate = [Some(self.focus), self.hover_item]
            .into_iter()
            .flatten()
            .filter_map(|idx| self.items.get(idx))
            .find(needs);
        let Some(path) = candidate.map(|i| i.entry.path.clone()) else {
            self.probe_wait = None;
            return;
        };
        match &self.probe_wait {
            Some((p, since)) if *p == path => {
                if since.elapsed() >= PAUSE {
                    self.prober.request(&path);
                    self.probe_requested.insert(path);
                    self.probe_wait = None;
                } else {
                    ctx.request_repaint_after(PAUSE - since.elapsed());
                }
            }
            _ => {
                self.probe_wait = Some((path, Instant::now()));
                ctx.request_repaint_after(PAUSE);
            }
        }
    }

    fn take_probe_results(&mut self) {
        for r in self.prober.poll() {
            match r {
                ProbeResult::Ok(path, meta) => {
                    self.meta_cache.insert(&path, meta.clone());
                    for list in [&mut self.all_items, &mut self.items] {
                        for it in list.iter_mut().filter(|i| i.entry.path == path) {
                            it.meta = Some(meta.clone());
                        }
                    }
                }
                ProbeResult::Failed(path) => {
                    self.probe_failed.insert(path);
                }
            }
        }
        if self.last_meta_write.elapsed() > Duration::from_secs(30) {
            self.last_meta_write = Instant::now();
            self.meta_cache.save_if_dirty();
        }
    }

    fn scrape_settings(&self) -> crate::scrape::Settings {
        crate::scrape::Settings { online: self.cfg.online_metadata }
    }

    /// The episode a file is: from the library database, else guessed from its name.
    pub(crate) fn episode_of(&self, path: &Path) -> Option<crate::media::Episode> {
        match self.db.file(path) {
            Some(f) => f.ep.clone(),
            None => crate::media::episode(path),
        }
    }

    /// Database key of the show or movie a row is.
    pub(crate) fn key_of(&self, item: &Item) -> Option<String> {
        if let Some(k) = &item.show {
            return Some(k.clone());
        }
        self.db.file(&item.entry.path).map(|f| f.item.clone())
    }

    pub(crate) fn info_for_path(&self, path: &Path) -> Option<&crate::scrape::Info> {
        let f = self.db.file(path)?;
        self.db.item(&f.item).map(|i| &i.info).filter(|i| !i.title.is_empty() || i.poster.is_some())
    }

    pub(crate) fn info_for(&self, item: &Item) -> Option<&crate::scrape::Info> {
        let key = self.key_of(item)?;
        self.db.item(&key).map(|i| &i.info).filter(|i| !i.title.is_empty() || i.poster.is_some())
    }

    /// What the matcher needs to know about a show/movie.
    fn match_req(&self, key: &str) -> Option<crate::scrape::MatchReq> {
        let it = self.db.item(key)?;
        let show = self.db.is_show(key);
        let file = if show { None } else { self.db.files.iter().find(|(_, f)| f.item == key).map(|(p, _)| p.clone()) };
        Some(crate::scrape::MatchReq {
            key: key.to_string(),
            show,
            title: it.title.clone(),
            year: it.year,
            tmdb: it.hint_tmdb,
            tvdb: it.hint_tvdb,
            imdb: it.hint_imdb.clone(),
            folder: it.folders.first().cloned().filter(|f| show || f.is_dir()),
            file,
        })
    }

    /// Queue every unmatched show/movie (one at a time on the matcher thread).
    fn match_pending(&mut self) {
        let settings = self.scrape_settings();
        for key in self.db.unmatched() {
            if let Some(req) = self.match_req(&key) {
                self.scraper.request(req, settings.clone());
            }
        }
    }

    /// A library scan finished: update the database, the Movies/TV views and start matching.
    fn on_library_scan(&mut self, entries: &[Entry]) {
        self.entry_map = entries.iter().map(|e| (e.path.clone(), e.clone())).collect();
        self.db.merge(entries, &self.cfg.libraries, crate::config::now_unix());
        self.catalog = self.db.catalog(&self.entry_map);
        self.last_scan = Instant::now();
        self.match_pending();
    }

    fn take_scrape_results(&mut self) {
        let mut changed = false;
        for done in self.scraper.poll() {
            match done {
                crate::scrape::Done::Matched { key, id, info, manual } => {
                    let new_key = self.db.apply_match(&key, id, info, manual);
                    if let Source::Show(k) = &self.source {
                        if *k == key {
                            self.source = Source::Show(new_key.clone());
                        }
                    }
                    if self.fix.as_ref().is_some_and(|f| f.key == key) {
                        self.fix = None;
                        self.overlay = Overlay::None;
                        let name = self.db.item(&new_key).map(|i| i.name().to_string()).unwrap_or_default();
                        self.toast(format!("Matched: {name}"));
                    }
                    changed = true;
                }
                crate::scrape::Done::Results { query, list, error } => {
                    if let Some(f) = self.fix.as_mut().filter(|f| f.searched == query) {
                        f.busy = false;
                        f.results = list;
                        f.error = error;
                        f.focus = if f.results.is_empty() { 0 } else { 1 };
                    }
                }
                crate::scrape::Done::SubResults { video, result } => self.on_sub_results(video, result),
                crate::scrape::Done::SubSaved { video, result } => self.on_sub_saved(video, result),
                crate::scrape::Done::Image { src, path } => {
                    if let Some(p) = path {
                        self.posters.retain(|(pp, _)| *pp != p);
                        self.db.images.insert(src, p);
                        self.db.touch();
                    }
                }
            }
        }
        if changed {
            self.catalog = self.db.catalog(&self.entry_map);
            if self.source.from_catalog() {
                self.rebuild_catalog_view();
            }
        }
        if self.last_scrape_write.elapsed() > Duration::from_secs(30) {
            self.last_scrape_write = Instant::now();
            self.db.save_if_dirty();
        }
    }

    /// Rescan the libraries now and then (new files show up without restarting).
    fn schedule_rescan(&mut self, ctx: &egui::Context) {
        const EVERY: Duration = Duration::from_secs(10 * 60);
        if self.screen == Screen::Browser && self.last_scan.elapsed() > EVERY && !self.loading {
            self.last_scan = Instant::now();
            self.scanner.request_all(&self.cfg.roots);
        }
        // No timer of its own: the browser clock already wakes the UI every 20 s.
        let _ = ctx;
    }

    pub(crate) fn poster_loaded(&self, path: &Path) -> bool {
        self.posters.iter().any(|(p, _)| p == path)
    }

    /// Texture for an image source (URL or local file): downloads/shrinks it first if needed.
    pub(crate) fn art(&mut self, ctx: &egui::Context, src: &str, kind: crate::scrape::ImgKind) -> Option<egui::TextureHandle> {
        match self.db.image(src).cloned() {
            Some(p) => self.poster(ctx, &p),
            None => {
                if self.cfg.online_metadata || !src.starts_with("http") {
                    self.scraper.image(src, kind);
                }
                None
            }
        }
    }

    /// Is the art for `src` ready to draw without a load this frame?
    pub(crate) fn art_ready(&self, src: &str) -> bool {
        self.db.image(src).is_some_and(|p| self.poster_loaded(p))
    }

    /// Poster texture, loaded on first use; keeps the last few around.
    pub(crate) fn poster(&mut self, ctx: &egui::Context, path: &Path) -> Option<egui::TextureHandle> {
        if let Some(i) = self.posters.iter().position(|(p, _)| p == path) {
            let e = self.posters.remove(i);
            let tex = e.1.clone();
            self.posters.push(e);
            return Some(tex);
        }
        let img = crate::scrape::load_thumb(path)?;
        let tex = ctx.load_texture(path.to_string_lossy(), img, egui::TextureOptions::LINEAR);
        self.posters.push((path.to_path_buf(), tex.clone()));
        // Enough for a screenful of grid tiles (~0.6 MB each on the GPU); a few in list view.
        let keep = if self.cfg.grid_view { 48 } else { 12 };
        while self.posters.len() > keep {
            drop(self.posters.remove(0));
        }
        Some(tex)
    }

    /// Settings → Library → Refresh posters and info: match everything again (except Fix
    /// Match picks) and re-download art.
    pub(crate) fn refresh_metadata(&mut self) {
        for it in self.db.shows.values_mut().chain(self.db.movies.values_mut()) {
            if it.state != crate::libdb::MatchState::Manual {
                it.state = crate::libdb::MatchState::New;
            }
        }
        self.db.images.clear();
        self.db.touch();
        self.scraper.forget();
        self.posters.clear();
        self.match_pending();
        self.toast("Matching your library again");
    }

    fn remember_focus(&mut self) {
        if let (Source::Dir(d), Some(item)) = (&self.source, self.items.get(self.focus)) {
            self.remembered.retain(|(rd, _)| rd != d);
            self.remembered.push((d.clone(), item.entry.path.clone()));
        }
    }

    fn rebuild_virtual(&mut self) {
        let only_unfinished = self.source == Source::Continue;
        let entries: Vec<Entry> = self
            .hist
            .recent(only_unfinished)
            .into_iter()
            .take(200)
            .map(|(path, _)| {
                let meta = std::fs::metadata(&path).ok();
                Entry {
                    name: path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                    is_dir: false,
                    size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                    modified: meta.and_then(|m| m.modified().ok()),
                    subs: vec![],
                    path,
                }
            })
            .collect();
        let entries: Vec<Item> = entries.into_iter().map(|e| self.item(e)).collect();
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
        } else if self.source == Source::Shows {
            self.rebuild_catalog_view();
        }
    }

    fn go_up(&mut self) {
        if !self.filter.is_empty() {
            self.filter.clear();
            self.apply_filter();
            return;
        }
        // Back never switches tabs (only the menu does): inside a folder it goes up one level, and
        // at a tab's top level (My media, Continue watching, Recent, a library's root, or the
        // filesystem root when browsing) it opens the menu.
        if let Source::Playlist(pl) = self.source.clone() {
            let from_folder = !crate::playlist::is_own(&pl) && matches!(self.remembered.last(), Some((d, _)) if pl.parent() == Some(d.as_path()));
            match pl.parent().filter(|_| from_folder) {
                Some(dir) => self.open_dir(dir.to_path_buf(), Some(pl.clone())),
                None => {
                    self.open_source(Source::Playlists);
                    if let Some(i) = self.items.iter().position(|i| i.entry.path == pl) {
                        self.focus = i;
                    }
                }
            }
            return;
        }
        // A show's page goes back to where it was opened from (My media, Recently added…),
        // else the list of shows.
        if let Source::Show(key) = &self.source {
            let focus = show_path(key);
            let back = self.show_return.take().unwrap_or(Source::Shows);
            self.open_source(back);
            if let Some(i) = self.items.iter().position(|i| i.entry.path == focus) {
                self.focus = i;
            }
            return;
        }
        let parent = match &self.source {
            Source::Dir(d) if self.exploring || !self.cfg.roots.contains(d) => d.parent().map(|p| (p.to_path_buf(), d.clone())),
            _ => None,
        };
        match parent {
            Some((parent, child)) => self.open_dir(parent, Some(child)),
            None => {
                self.overlay = Overlay::Menu;
                self.menu_focus = self.menu_current();
            }
        }
    }

    fn activate(&mut self, from_start: bool) {
        let Some(item) = self.items.get(self.focus).cloned() else { return };
        if let Some(key) = item.show {
            if !matches!(self.source, Source::Shows | Source::Show(_)) {
                self.show_return = Some(self.source.clone());
            }
            self.open_source(Source::Show(key));
        } else if crate::playlist::is_playlist(&item.entry.path) {
            if from_start {
                // Options on a playlist: play it all from the top.
                let list = crate::playlist::read(&item.entry.path);
                self.play_queue(list, 0, crate::playlist::name(&item.entry.path), true);
            } else {
                self.open_source(Source::Playlist(item.entry.path));
            }
        } else if let Source::Playlist(pl) = self.source.clone() {
            // Inside a playlist, the rest of it plays next.
            let list: Vec<PathBuf> = self.all_items.iter().map(|i| i.entry.path.clone()).collect();
            let idx = list.iter().position(|p| *p == item.entry.path).unwrap_or(0);
            self.play_queue(list, idx, crate::playlist::name(&pl), from_start);
        } else if item.entry.is_dir {
            self.open_dir(item.entry.path, None);
        } else {
            self.play_with_queue(&item.entry.path, from_start);
        }
    }

    /// Play a file; an episode queues up the rest of its show so the next one autoplays.
    pub(crate) fn play_with_queue(&mut self, path: &Path, from_start: bool) {
        let show = self.episode_of(path).and_then(|e| {
            let key = if e.key.is_empty() { crate::media::show_key(&e.show) } else { e.key.clone() };
            self.catalog.show(&key).cloned()
        });
        // A split movie plays all its parts in a row.
        let parts = self.db.movie_parts(path);
        if show.is_none() && parts.len() > 1 {
            let idx = parts.iter().position(|p| p == path).unwrap_or(0);
            let name = self.title_of(path);
            return self.play_queue(parts, idx, name, from_start);
        }
        match show {
            Some(show) if show.episodes.iter().any(|(_, f)| f.path == path) => {
                let list: Vec<PathBuf> = show.episodes.iter().map(|(_, f)| f.path.clone()).collect();
                let idx = list.iter().position(|p| p == path).unwrap_or(0);
                self.play_queue(list, idx, show.name.clone(), from_start);
            }
            _ => self.play(path, from_start),
        }
    }

    /// Options (X) on a show row: straight to its next episode.
    fn play_next_up(&mut self, key: &str) {
        let Some(show) = self.catalog.show(key).cloned() else { return };
        let i = self.next_up(&show);
        let list: Vec<PathBuf> = show.episodes.iter().map(|(_, f)| f.path.clone()).collect();
        self.play_queue(list, i, show.name.clone(), false);
    }

    // ------------------------------------------------------------ playback

    /// Play one file. If it's part of the current queue, the queue carries on from there.
    pub(crate) fn play(&mut self, path: &Path, from_start: bool) {
        match self.queue.iter().position(|p| p == path) {
            Some(i) => self.queue_idx = i,
            None => {
                self.queue = vec![path.to_path_buf()];
                self.queue_idx = 0;
                self.queue_name.clear();
            }
        }
        self.load(path, from_start);
    }

    /// Play `list[idx]`, then the rest of `list` (autoplay, next/previous).
    pub(crate) fn play_queue(&mut self, list: Vec<PathBuf>, idx: usize, name: String, from_start: bool) {
        let Some(path) = list.get(idx).cloned() else { return };
        self.queue = list;
        self.queue_idx = idx;
        self.queue_name = name;
        self.load(&path, from_start);
    }

    pub(crate) fn queue_next(&self) -> Option<usize> {
        let n = self.queue.len();
        if self.queue_idx + 1 < n {
            Some(self.queue_idx + 1)
        } else if self.cfg.repeat == crate::config::Repeat::All && n > 1 {
            Some(0)
        } else {
            None
        }
    }

    /// Jump within the queue (next/previous episode). Saves the current position first.
    pub(crate) fn play_queue_at(&mut self, idx: usize) {
        let Some(path) = self.queue.get(idx).cloned() else { return };
        self.record_position();
        self.queue_idx = idx;
        self.load(&path, false);
    }

    fn load(&mut self, path: &Path, from_start: bool) {
        self.up_next = None;
        self.at_end = false;
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
        player.set_loop_file(self.cfg.repeat == crate::config::Repeat::One);
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
        self.up_next = None;
        self.at_end = false;
        self.record_position();
        if let Some(p) = self.player.as_mut() {
            p.stop();
        }
        self.want_focus = self.playing_path.take();
        self.screen = Screen::Browser;
        self.overlay = Overlay::None;
        self.hist.save_if_dirty();
        self.refresh_hist_in_items();
        crate::mem::trim();
        if let Some(f) = self.want_focus.take() {
            if let Some(i) = self.items.iter().position(|i| i.entry.path == f) {
                self.focus = i;
            }
        }
    }

    fn on_player_event(&mut self, ev: PlayerEvent) {
        match ev {
            PlayerEvent::FileLoaded => {
                // Subtitles downloaded earlier for this video come back automatically.
                if let (Some(p), Some(path)) = (self.player.as_ref(), self.playing_path.as_ref()) {
                    for s in crate::subs::cached(path) {
                        p.add_sub(&s, false);
                    }
                }
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
                    self.on_end_of_file();
                } else if let Some(err) = self.player.as_ref().and_then(|p| p.s.error.clone()) {
                    self.toast(err);
                    self.playing_path = None;
                    self.screen = Screen::Browser;
                }
            }
        }
    }

    /// The video played to the end: autoplay the next one, or go back, or stay on the last frame.
    fn on_end_of_file(&mut self) {
        if self.at_end || self.up_next.is_some() {
            return;
        }
        if let Some(path) = self.playing_path.clone() {
            let e = self.hist.entry(&path);
            e.watched = true;
            e.pos = 0.0;
            self.hist.save_if_dirty();
        }
        if self.cfg.autoplay && self.queue_next().is_some() {
            self.up_next = Some(Instant::now() + Duration::from_secs(self.cfg.autoplay_countdown as u64));
            self.show_osd();
        } else {
            self.finish_without_next();
        }
    }

    /// Nothing (more) to autoplay: leave the player, or stay with Replay / Back on screen.
    fn finish_without_next(&mut self) {
        self.up_next = None;
        if self.cfg.exit_on_end {
            if let Some(p) = self.player.as_mut() {
                p.stop();
            }
            self.want_focus = self.playing_path.take();
            self.screen = Screen::Browser;
            self.overlay = Overlay::None;
            self.at_end = false;
            self.refresh_hist_in_items();
        } else {
            self.at_end = true;
            self.show_osd();
        }
    }

    /// Runs the "Up next" countdown.
    fn tick_up_next(&mut self, ctx: &egui::Context) {
        let Some(at) = self.up_next else { return };
        let now = Instant::now();
        if now >= at {
            if let Some(i) = self.queue_next() {
                self.play_queue_at(i);
            } else {
                self.up_next = None;
            }
        } else {
            ctx.request_repaint_after((at - now).min(Duration::from_millis(250)));
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
        let browser_filtering = self.filtering && self.context() == Context::Browser;
        let text_mode = self.overlay == Overlay::Palette || self.overlay == Overlay::FixMatch || browser_filtering;
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
                    // Fix Match search box: typing, Backspace; Enter/arrows/Esc stay actions.
                    if self.overlay == Overlay::FixMatch && key == egui::Key::Backspace && self.osk.is_none() {
                        self.fix_backspace();
                        continue;
                    }
                    if browser_filtering {
                        match key {
                            egui::Key::Enter => {
                                self.filtering = false;
                                continue;
                            }
                            egui::Key::Escape => {
                                self.filtering = false;
                                self.filter.clear();
                                self.apply_filter();
                                continue;
                            }
                            egui::Key::Backspace => {
                                if self.filter.pop().is_none() {
                                    self.filtering = false;
                                }
                                self.apply_filter();
                                continue;
                            }
                            _ => {}
                        }
                    }
                    if text_mode
                        && (chord.is_plain_text()
                            || matches!(key, egui::Key::Backspace | egui::Key::Delete | egui::Key::ArrowLeft | egui::Key::ArrowRight | egui::Key::Home | egui::Key::End))
                    {
                        continue;
                    }
                    let ctxt = self.context();
                    // vim: `gg` jumps to the top, digits are a count for the next motion (5j).
                    if matches!(ctxt, Context::Browser | Context::List) && !chord.ctrl && !chord.alt && !chord.shift {
                        if key == egui::Key::G {
                            if self.pending_g.is_some_and(|t| t.elapsed() < Duration::from_millis(800)) {
                                self.pending_g = None;
                                actions.push(Action::Home);
                            } else {
                                self.pending_g = Some(Instant::now());
                            }
                            continue;
                        }
                        if let Some(d) = digit(key) {
                            if d > 0 || self.count.is_some() {
                                self.count = Some((self.count.unwrap_or(0) * 10 + d).min(999));
                                continue;
                            }
                        }
                    }
                    self.pending_g = None;
                    let count = self.count.take().unwrap_or(1);
                    if let Some(a) = self.binds.key_action(ctxt, chord) {
                        let n = if a.repeats() { count.min(200) } else { 1 };
                        for _ in 0..n {
                            actions.push(a);
                        }
                    }
                }
                egui::Event::Text(t) => {
                    if self.overlay == Overlay::FixMatch {
                        let mods = ctx.input(|i| i.modifiers);
                        if !mods.ctrl && !mods.mac_cmd && !mods.alt {
                            self.fix_type(&t);
                        }
                    } else if browser_filtering && self.listening.is_none() {
                        let mods = ctx.input(|i| i.modifiers);
                        if !mods.ctrl && !mods.mac_cmd && !mods.alt {
                            self.filter.push_str(&t);
                            self.apply_filter();
                            self.focus = 0;
                        }
                    }
                }
                egui::Event::PointerMoved(_) => self.pointer_moved = Instant::now(),
                _ => {}
            }
        }

        let mut pad_events: Vec<PadEvent> = self.pad_rx.try_iter().collect();
        pad_events.append(&mut self.fake_pad);
        for ev in pad_events {
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
                    if let Some(h) = self.sub_hold {
                        if h.open {
                            match p {
                                _ if is_direction(p) => {
                                    self.sub_hold_move(p);
                                    self.repeater.press(p);
                                }
                                PadInput::B => self.sub_hold = None,
                                _ => {}
                            }
                        }
                        self.held_pads.push(p);
                        continue;
                    }
                    if self.context() == Context::Player
                        && self.binds.pad_action(Context::Player, p) == Some(Action::NextSubtitle)
                        && self.player.as_ref().is_some_and(|pl| pl.s.loaded)
                    {
                        self.sub_hold = Some(SubHold { input: p, since: Instant::now(), open: false, idx: 0 });
                        self.held_pads.push(p);
                        continue;
                    }
                    if self.osk.is_some() {
                        self.osk_pad(p);
                        if is_direction(p) {
                            self.repeater.press(p);
                        }
                        self.held_pads.push(p);
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
                    if let Some(h) = self.sub_hold.filter(|h| h.input == p) {
                        self.sub_hold = None;
                        if h.open {
                            self.sub_hold_select(h.idx);
                        } else {
                            // Short tap: cycle like before.
                            actions.push(Action::NextSubtitle);
                        }
                    }
                    self.repeater.release(p);
                    self.held_pads.retain(|h| *h != p);
                }
            }
        }
        let (due, next) = self.repeater.due();
        for p in due {
            if self.sub_hold.is_some_and(|h| h.open) && is_direction(p) {
                self.sub_hold_move(p);
                continue;
            }
            if self.osk.is_some() {
                if is_direction(p) {
                    self.osk_pad(p);
                } else {
                    self.repeater.release(p);
                }
                continue;
            }
            match self.binds.pad_action(self.context(), p) {
                Some(a) if a.repeats() => actions.push(a),
                _ => self.repeater.release(p),
            }
        }
        if let Some(n) = next {
            ctx.request_repaint_after(n);
        }
        if let Some(h) = self.sub_hold.as_mut().filter(|h| !h.open) {
            if self.screen != Screen::Player {
                self.sub_hold = None;
            } else if h.since.elapsed() >= HOLD_DELAY {
                h.open = true;
                let cur = self.player.as_ref().and_then(|p| p.s.selected(TrackKind::Sub).map(|t| t.id));
                let opts = self.sub_options();
                if let Some(h) = self.sub_hold.as_mut() {
                    h.idx = opts.iter().position(|o| o.0 == cur).unwrap_or(0);
                }
                self.show_osd();
            } else {
                ctx.request_repaint_after(HOLD_DELAY - h.since.elapsed());
            }
        }
        actions
    }

    /// Subtitle choices for the hold picker: Off, then every track.
    pub(crate) fn sub_options(&self) -> Vec<(Option<i64>, String, String)> {
        let mut v = vec![(None, "Off".to_string(), String::new())];
        if let Some(p) = &self.player {
            v.extend(p.s.tracks_of(TrackKind::Sub).map(|t| (Some(t.id), t.label(), t.meta())));
        }
        v.push((Some(subs_ui::DOWNLOAD_ID), "+ Download subtitles…".into(), "OpenSubtitles".into()));
        v
    }

    fn sub_hold_move(&mut self, p: PadInput) {
        use PadInput::*;
        let n = self.sub_options().len().max(1);
        if let Some(h) = self.sub_hold.as_mut() {
            match p {
                DpadUp | LstickUp | DpadLeft | LstickLeft => h.idx = (h.idx + n - 1) % n,
                DpadDown | LstickDown | DpadRight | LstickRight => h.idx = (h.idx + 1) % n,
                _ => {}
            }
        }
    }

    fn sub_hold_select(&mut self, idx: usize) {
        let opts = self.sub_options();
        if opts.get(idx).is_some_and(|o| o.0 == Some(subs_ui::DOWNLOAD_ID)) {
            return self.open_sub_download();
        }
        if let (Some((id, label, _)), Some(p)) = (opts.get(idx), &self.player) {
            p.select_track(TrackKind::Sub, *id);
            self.toast(format!("Subtitles: {label}"));
        }
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
            Overlay::AddToPlaylist => return self.add_playlist_action(a),
            Overlay::FixMatch => return self.fix_action(a),
            Overlay::LibraryKind => return self.lib_kind_action(a),
            Overlay::SubDownload => return self.sub_dl_action(a),
            Overlay::Options => return self.options_action(a),
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
        if a == ToggleView {
            return self.toggle_grid();
        }
        if a == FixMatch {
            return self.open_fix_match();
        }
        if self.grid_active() && matches!(a, Up | Down | PageUp | PageDown | Left | Right) && self.grid_move(a) {
            return;
        }
        match a {
            Up => self.focus = self.focus.saturating_sub(1),
            Down => self.focus = (self.focus + 1).min(n.saturating_sub(1)),
            PageUp => self.focus = self.focus.saturating_sub(page),
            PageDown => self.focus = (self.focus + page).min(n.saturating_sub(1)),
            Home => self.focus = 0,
            End => self.focus = n.saturating_sub(1),
            Accept => self.activate(false),
            // On a folder, X adds it to the library (or removes it); on a file it plays from the start.
            AddToPlaylist => {
                let files = self.files_for_playlist();
                self.open_add_to_playlist(files);
            }
            // □ / X: the Options menu (play from start, playlists, watched, fix match…).
            Options => self.open_options(),
            PlayFromStart => self.activate(true),
            ToggleLibrary => {
                if let Some(i) = self.items.get(self.focus).cloned().filter(|i| i.entry.is_dir && i.show.is_none()) {
                    self.toggle_library(&i.entry.path);
                }
            }
            PlayNext => {
                if let Some(key) = self.items.get(self.focus).and_then(|i| i.show.clone()) {
                    self.play_next_up(&key);
                }
            }
            // On a show's page, left/right (and LT/RT, [ ]) switch seasons.
            PrevTab => {
                self.change_season(-1);
            }
            NextTab => {
                self.change_season(1);
            }
            Right => {
                if self.change_season(1) {
                } else if self.items.get(self.focus).is_some_and(|i| i.entry.is_dir) {
                    self.activate(false);
                }
            }
            Left => {
                if !self.change_season(-1) {
                    self.go_up();
                }
            }
            Back => self.go_up(),
            Filter => {
                if self.last_was_pad {
                    self.open_osk(osk::OskTarget::Filter);
                } else {
                    self.filtering = true;
                }
            }
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
        // "Up next" card: A plays now, B cancels the countdown.
        if self.up_next.is_some() {
            match a {
                Accept | PlayPause | NextItem => {
                    if let Some(i) = self.queue_next() {
                        self.play_queue_at(i);
                    }
                    return;
                }
                Back => return self.finish_without_next(),
                _ => {}
            }
        }
        if self.at_end && matches!(a, Accept | PlayPause) {
            return self.replay();
        }
        match a {
            // First press shows the controls, second goes back to the browser.
            Back => {
                if was_visible || self.at_end {
                    self.stop();
                }
                return;
            }
            NextItem => {
                match self.queue_next() {
                    Some(i) => self.play_queue_at(i),
                    None => self.toast("Nothing after this one"),
                }
                return;
            }
            PrevItem => {
                // Like a CD player: restart this one unless we're near its beginning.
                let pos = self.player.as_ref().map(|p| p.s.time_pos).unwrap_or(0.0);
                if pos > 10.0 || self.queue_idx == 0 {
                    self.replay();
                } else {
                    self.play_queue_at(self.queue_idx - 1);
                }
                return;
            }
            Replay => return self.replay(),
            FixMatch => return self.open_fix_match(),
            DownloadSubs => return self.open_sub_download(),
            CycleRepeat => {
                self.set_repeat(self.cfg.repeat.next());
                return;
            }
            Episodes => {
                self.overlay = Overlay::Tracks;
                self.tracks_tab = 0;
                self.tracks_focus = self.tracks_selected_index();
                return;
            }
            Stop => return self.stop(),
            VolumeUp => return self.set_volume(self.vol + 5.0, true),
            VolumeDown => return self.set_volume(self.vol - 5.0, true),
            Mute => return self.toggle_mute(),
            BrightnessUp => return self.change_brightness(0.05),
            BrightnessDown => return self.change_brightness(-0.05),
            Tracks | Menu => {
                self.overlay = Overlay::Tracks;
                self.tracks_focus = self.tracks_selected_index();
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
            FrameStep => p.frame_step(true),
            FrameBackStep => p.frame_step(false),
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
        paused || self.at_end || self.up_next.is_some() || Instant::now() < self.osd_until || self.overlay != Overlay::None
    }

    pub(crate) fn replay(&mut self) {
        self.at_end = false;
        self.up_next = None;
        if let Some(path) = self.playing_path.clone() {
            self.hist.entry(&path).last_played = crate::config::now_unix();
        }
        if let Some(p) = self.player.as_mut() {
            p.replay();
        }
        self.show_osd();
    }

    pub(crate) fn set_repeat(&mut self, r: crate::config::Repeat) {
        self.cfg.repeat = r;
        self.cfg.save();
        if let Some(p) = &self.player {
            p.set_loop_file(r == crate::config::Repeat::One);
        }
        self.toast(format!("Repeat: {}", r.label()));
    }

    /// Episode title: from metadata if known, else from the file name.
    pub(crate) fn episode_title(&self, path: &Path, e: &crate::media::Episode) -> Option<String> {
        self.info_for_path(path)
            .and_then(|i| i.episodes.get(&crate::scrape::ep_key(e.season, e.episode)))
            .map(|ep| ep.title.clone())
            .filter(|t| !t.is_empty())
            .or_else(|| e.title.clone())
    }

    /// The name shown in a browser row.
    pub(crate) fn row_name(&self, item: &Item) -> String {
        if item.show.is_some() {
            return item.entry.name.clone();
        }
        if item.entry.is_dir {
            return format!("{}/", item.entry.name);
        }
        if crate::playlist::is_playlist(&item.entry.path) && self.source == Source::Playlists {
            return crate::playlist::name(&item.entry.path);
        }
        match (&self.source, &item.ep) {
            // A show's page: "3  Title" (the show and season are already on screen).
            (Source::Show(_), Some(e)) => {
                let title = self.episode_title(&item.entry.path, e).unwrap_or_else(|| format!("Episode {}", e.episode));
                match e.episode_end {
                    Some(end) => format!("{}–{}  {title}", e.episode, end),
                    None => format!("{:>2}  {title}", e.episode),
                }
            }
            (Source::Dir(_), _) => item.entry.name.clone(),
            (_, Some(_)) => self.title_of(&item.entry.path),
            // Library movies in flat lists: their (matched) title, "Blade Runner (1982)".
            (Source::All | Source::Added | Source::Continue | Source::Recent, None) if self.db.file(&item.entry.path).is_some() => self.title_of(&item.entry.path),
            (Source::Movies, None) => match crate::media::movie_title(&item.entry.name) {
                (t, Some(y)) => format!("{t} ({y})"),
                (t, None) => t,
            },
            _ => item.entry.name.clone(),
        }
    }

    /// How a file is named in the UI: "Show · S01E02 · Title", a movie's title, or the file name.
    pub(crate) fn title_of(&self, path: &Path) -> String {
        if let Some(e) = self.episode_of(path) {
            return match self.episode_title(path, &e) {
                Some(t) => format!("{} · {} · {t}", e.show, e.code()),
                None => format!("{} · {}", e.show, e.code()),
            };
        }
        if let Some(i) = self.info_for_path(path).filter(|i| !i.title.is_empty()) {
            return match i.year {
                Some(y) => format!("{} ({y})", i.title),
                None => i.title.clone(),
            };
        }
        match path.file_name().map(|n| crate::media::movie_title(&n.to_string_lossy())) {
            Some((t, Some(y))) if !t.is_empty() => format!("{t} ({y})"),
            Some((t, None)) if !t.is_empty() => t,
            _ => path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
        }
    }

    /// Settings → Library → Clear watch history.
    pub(crate) fn clear_history(&mut self) {
        self.hist.clear();
        self.hist.save_if_dirty();
        self.refresh_hist_in_items();
        self.toast("Watch history cleared");
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
    /// Exact width `hints` will draw, for right-aligning a hint row.
    pub(crate) fn hints_width(&self, p: &egui::Painter, ctx_: Context, list: &[(Action, &str)], size: f32, gap: f32) -> f32 {
        let parts: Vec<f32> = list
            .iter()
            .filter_map(|(a, label)| {
                let (cap, _) = self.glyph(ctx_, *a)?;
                Some(draw::chip_width(p, &cap, size * 0.85) + size * 0.45 + draw::measure(p, label, size).x)
            })
            .collect();
        parts.iter().sum::<f32>() + gap * parts.len().saturating_sub(1) as f32
    }

    pub(crate) fn hints(&mut self, ui: &egui::Ui, pos: egui::Pos2, ctx_: Context, list: &[(Action, &str)], size: f32, gap: f32, fg: Color32, cap_bg: Color32) -> Vec<Action> {
        let p = ui.painter();
        let mut x = pos.x;
        let mut clicked = Vec::new();
        for (a, label) in list {
            let Some((cap, color)) = self.glyph(ctx_, *a) else { continue };
            let r = draw::chip(p, egui::pos2(x, pos.y), &cap, size * 0.85, color.unwrap_or(fg), cap_bg);
            let t = draw::text(p, egui::pos2(r.right() + size * 0.45, pos.y), egui::Align2::LEFT_CENTER, label, size, fg);
            let hit = r.union(t);
            if ui.interact(hit, ui.scope_id().with(("hint", ctx_, *a, label, pos.y as i32)), egui::Sense::CLICK).clicked() {
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
                Step::Pad(p) => {
                    self.fake_pad.push(PadEvent::Down(p));
                    self.fake_pad.push(PadEvent::Up(p));
                    self.step_at = Instant::now() + Duration::from_millis(120);
                    ctx.request_repaint();
                    break;
                }
                Step::PadDown(p) | Step::PadUp(p) => {
                    self.fake_pad.push(if matches!(step, Step::PadDown(_)) { PadEvent::Down(p) } else { PadEvent::Up(p) });
                    self.step_at = Instant::now() + Duration::from_millis(120);
                    ctx.request_repaint();
                    break;
                }
                Step::Key(chord) => {
                    let modifiers = egui::Modifiers { ctrl: chord.ctrl, shift: chord.shift, alt: chord.alt, command: chord.ctrl, ..Default::default() };
                    self.injected.push(egui::Event::Key { key: chord.key, physical_key: None, pressed: true, repeat: false, modifiers });
                    self.injected.push(egui::Event::Key { key: chord.key, physical_key: None, pressed: false, repeat: false, modifiers });
                    self.step_at = Instant::now() + Duration::from_millis(120);
                    ctx.request_repaint();
                    break;
                }
                Step::Type(t) => {
                    self.injected.push(egui::Event::Text(t));
                    self.step_at = Instant::now() + Duration::from_millis(120);
                    ctx.request_repaint();
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

fn is_direction(p: PadInput) -> bool {
    use PadInput::*;
    matches!(p, DpadUp | DpadDown | DpadLeft | DpadRight | LstickUp | LstickDown | LstickLeft | LstickRight)
}

fn digit(k: egui::Key) -> Option<u32> {
    use egui::Key::*;
    Some(match k {
        Num0 => 0,
        Num1 => 1,
        Num2 => 2,
        Num3 => 3,
        Num4 => 4,
        Num5 => 5,
        Num6 => 6,
        Num7 => 7,
        Num8 => 8,
        Num9 => 9,
        _ => return None,
    })
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
            if let Some(p) = self.startup_play.take() {
                self.play(&p, false);
            }
            if self.fullscreen {
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
            }
            // Keep the screen from blanking for as long as jimmy is open.
            self.sys.inhibit(true);
        }

        if let Some(l) = self.scanner.poll() {
            let wanted = if l.all { self.source == Source::All } else { Source::Dir(l.dir.clone()) == self.source };
            if l.all {
                self.on_library_scan(&l.entries);
                self.all_cache = Some(l.entries.clone());
                if self.source.from_catalog() {
                    self.loading = false;
                    self.list_error = l.error.clone();
                    self.rebuild_catalog_view();
                } else if self.source == Source::Added {
                    self.list_error = l.error.clone();
                    self.rebuild_added_view();
                }
            }
            if wanted {
                self.loading = false;
                self.list_error = l.error.clone();
                let items = if l.all { self.my_media_rows(l.entries) } else { l.entries.into_iter().map(|e| self.item(e)).collect() };
                self.set_items(items);
            }
        }

        let events = self.player.as_mut().map(|p| p.poll()).unwrap_or_default();
        for ev in events {
            self.on_player_event(ev);
        }
        self.tick_up_next(&ctx);
        self.sync_system();

        self.take_probe_results();
        self.schedule_probe(&ctx);
        self.take_scrape_results();
        self.schedule_rescan(&ctx);

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
            Overlay::AddToPlaylist => clicked.extend(self.draw_add_playlist(ui, rect)),
            Overlay::FixMatch => clicked.extend(self.draw_fix(ui, rect)),
            Overlay::LibraryKind => clicked.extend(self.draw_lib_kind(ui, rect)),
            Overlay::SubDownload => clicked.extend(self.draw_sub_dl(ui, rect)),
            Overlay::Options => clicked.extend(self.draw_options(ui, rect)),
        }
        self.osk_sync();
        if let Some(o) = self.osk {
            // Over the file list for search (results stay visible), under the palette for commands.
            let area = match o.target {
                osk::OskTarget::Filter => {
                    let detail_w = (rect.width() * 0.3).clamp(300.0, 460.0);
                    Rect::from_min_max(rect.min, egui::pos2(rect.right() - detail_w, rect.bottom() - 40.0))
                }
                osk::OskTarget::Palette | osk::OskTarget::FixMatch => rect,
            };
            self.draw_osk(ui, area);
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

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        raw.events.append(&mut self.injected);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.record_position();
        self.hist.save_if_dirty();
        self.meta_cache.save_if_dirty();
        self.db.save_if_dirty();
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
