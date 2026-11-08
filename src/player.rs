//! Playback through libmpv's render API: mpv draws each video frame into jimmy's OpenGL
//! framebuffer, then egui draws the UI on top in the same frame.
//!
//! All mpv calls live in this module, so a statically linked mpv build can be swapped in later.

use crate::config::Config;
use egui_glow::glow::{self, HasContext};
use libmpv2::Mpv;
use libmpv2::events::{Event, PropertyData};

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub use crate::mpv_render::{DisplayHandle, GetProcAddress};
use crate::mpv_render::Renderer;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
    Sub,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: i64,
    pub kind: TrackKind,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub codec: String,
    pub external: bool,
    pub selected: bool,
    pub channels: Option<i64>,
}

impl Track {
    pub fn label(&self) -> String {
        let lang = self.lang.as_deref().map(language_name);
        // mpv titles external subtitle tracks with their file name; that's noise here.
        let title = self.title.as_deref().filter(|t| {
            let lower = t.to_ascii_lowercase();
            !crate::library::SUB_EXTS.iter().any(|e| lower.ends_with(&format!(".{e}")))
        });
        match (lang, title) {
            (Some(l), Some(t)) if !t.eq_ignore_ascii_case(&l) => format!("{l} · {t}"),
            (Some(l), _) => l,
            (None, Some(t)) => t.to_string(),
            (None, None) => format!("Track {}", self.id),
        }
    }

    pub fn meta(&self) -> String {
        let codec = match self.codec.as_str() {
            "subrip" => "SRT".to_string(),
            "hdmv_pgs_subtitle" => "PGS".to_string(),
            "dvd_subtitle" => "VobSub".to_string(),
            "eia_608" => "EIA-608".to_string(),
            "webvtt" => "VTT".to_string(),
            c => c.to_ascii_uppercase(),
        };
        let mut s = codec;
        if let Some(ch) = self.channels {
            s.push_str(&match ch {
                1 => " mono".to_string(),
                2 => " stereo".to_string(),
                6 => " 5.1".to_string(),
                8 => " 7.1".to_string(),
                n => format!(" {n}ch"),
            });
        }
        s.push_str(if self.external { " · external" } else { " · embedded" });
        s
    }
}

fn language_name(code: &str) -> String {
    match code {
        "en" | "eng" => "English",
        "es" | "spa" => "Español",
        "fr" | "fre" | "fra" => "Français",
        "de" | "ger" | "deu" => "Deutsch",
        "it" | "ita" => "Italiano",
        "pt" | "por" => "Português",
        "ja" | "jpn" => "日本語",
        "zh" | "chi" | "zho" => "中文",
        "ko" | "kor" => "한국어",
        "ru" | "rus" => "Русский",
        "nl" | "dut" | "nld" => "Nederlands",
        "sv" | "swe" => "Svenska",
        "pl" | "pol" => "Polski",
        other => return other.to_string(),
    }
    .to_string()
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub path: Option<PathBuf>,
    pub loaded: bool,
    pub time_pos: f64,
    pub duration: f64,
    pub paused: bool,
    /// mpv's own volume (100 = unchanged, up to 200 for boost).
    pub volume: f64,
    pub mute: bool,
    pub chapter: i64,
    pub chapters: i64,
    pub cache_secs: f64,
    pub speed: f64,
    pub sub_delay: f64,
    pub sub_scale: f64,
    pub eof: bool,
    pub hwdec: String,
    pub video_codec: String,
    pub width: i64,
    pub height: i64,
    pub tracks: Vec<Track>,
    pub error: Option<String>,
}

impl State {
    pub fn tracks_of(&self, kind: TrackKind) -> impl Iterator<Item = &Track> {
        self.tracks.iter().filter(move |t| t.kind == kind)
    }

    pub fn selected(&self, kind: TrackKind) -> Option<&Track> {
        self.tracks_of(kind).find(|t| t.selected)
    }

    pub fn remaining(&self) -> f64 {
        (self.duration - self.time_pos).max(0.0) / self.speed.max(0.01)
    }
}

pub enum PlayerEvent {
    FileLoaded,
    /// Playback ended. `eof` is true when the file played to the end.
    Ended { eof: bool },
}

const OBSERVED: &[(&str, libmpv2::Format)] = {
    use libmpv2::Format::*;
    &[
        ("time-pos", Double),
        ("duration", Double),
        ("pause", Flag),
        ("volume", Double),
        ("mute", Flag),
        ("chapter", Int64),
        ("chapters", Int64),
        ("demuxer-cache-duration", Double),
        ("speed", Double),
        ("sub-delay", Double),
        ("sub-scale", Double),
        ("eof-reached", Flag),
        ("hwdec-current", String),
        ("video-codec", String),
        ("width", Int64),
        ("height", Int64),
        ("track-list/count", Int64),
        ("aid", String),
        ("sid", String),
    ]
};

pub struct Player {
    mpv: &'static Mpv,
    renderer: Arc<Mutex<Option<Renderer>>>,
    pub s: State,
    ctx: egui::Context,
    tracks_dirty: bool,
    sub_lifted: bool,
}

impl Player {
    pub fn new(ctx: egui::Context, cfg: &Config) -> anyhow::Result<Self> {
        let mut mpv = Mpv::with_initializer(|init| {
            let opts: &[(&str, &str)] = &[
                ("vo", "libmpv"),
                ("config", "no"),
                ("load-scripts", "no"),
                // mpv's built-in Lua scripts each run their own interpreter thread; jimmy uses none.
                ("load-stats-overlay", "no"),
                ("load-console", "no"),
                ("load-osd-console", "no"),
                ("load-select", "no"),
                ("load-positioning", "no"),
                ("load-context-menu", "no"),
                ("load-commands", "no"),
                ("load-auto-profiles", "no"),
                ("ytdl", "no"),
                ("terminal", "no"),
                ("input-default-bindings", "no"),
                ("input-vo-keyboard", "no"),
                ("osc", "no"),
                ("osd-level", "0"),
                ("idle", "yes"),
                ("keep-open", "yes"),
                ("stop-screensaver", "no"),
                ("audio-client-name", "jimmy"),
                ("sub-auto", "fuzzy"),
                ("volume-max", "200"),
            ];
            for (k, v) in opts {
                if let Err(e) = init.set_option(k, *v) {
                    log::warn!("mpv option {k}={v}: {e}");
                }
            }
            Ok(())
        })
        .map_err(|e| anyhow::anyhow!("starting mpv: {e}"))?;

        let c = ctx.clone();
        mpv.set_wakeup_callback(move || c.request_repaint());
        let mpv: &'static Mpv = Box::leak(Box::new(mpv));
        for (i, (name, fmt)) in OBSERVED.iter().enumerate() {
            if let Err(e) = mpv.observe_property(name, *fmt, i as u64 + 1) {
                log::warn!("observe {name}: {e}");
            }
        }
        let mut p = Self {
            mpv,
            renderer: Arc::new(Mutex::new(None)),
            s: State { speed: 1.0, volume: 100.0, sub_scale: 1.0, ..Default::default() },
            ctx,
            tracks_dirty: false,
            sub_lifted: false,
        };
        p.apply_config(cfg);
        Ok(p)
    }

    fn set(&self, name: &str, value: &str) {
        if let Err(e) = self.mpv.set_property(name, value) {
            log::debug!("mpv set {name}={value}: {e}");
        }
    }

    fn cmd(&self, name: &str, args: &[&str]) {
        if let Err(e) = self.mpv.command(name, args) {
            log::debug!("mpv {name} {args:?}: {e}");
        }
    }

    /// Low-end-friendly defaults plus the user's settings. Safe to call at any time.
    pub fn apply_config(&mut self, cfg: &Config) {
        let ahead = cfg.readahead_secs.to_string();
        let opts: Vec<(&str, String)> = vec![
            ("hwdec", cfg.hwdec.clone()),
            ("video-sync", "audio".into()),
            ("interpolation", "no".into()),
            ("scale", "bilinear".into()),
            ("dscale", "bilinear".into()),
            ("cscale", "bilinear".into()),
            ("deband", "no".into()),
            ("dither-depth", "no".into()),
            ("correct-downscaling", "no".into()),
            ("linear-downscaling", "no".into()),
            ("sigmoid-upscaling", "no".into()),
            ("hdr-compute-peak", "no".into()),
            ("cache", "yes".into()),
            // The read-ahead cache is the biggest memory user: cap it by size, not just time.
            ("demuxer-max-bytes", format!("{}MiB", cfg.readahead_mb)),
            ("demuxer-max-back-bytes", format!("{}MiB", (cfg.readahead_mb / 6).max(4))),
            ("demuxer-donate-buffer", "yes".into()),
            ("demuxer-readahead-secs", ahead.clone()),
            ("cache-secs", ahead),
            ("sub-scale", format!("{}", cfg.sub_scale)),
            ("sub-border-style", if cfg.sub_background_box { "background-box" } else { "outline-and-shadow" }.into()),
            ("sub-back-color", "#B8000000".into()),
            ("vd-lavc-skiploopfilter", if cfg.rescue_mode { "all" } else { "default" }.into()),
            ("vd-lavc-fast", if cfg.rescue_mode { "yes" } else { "no" }.into()),
            ("framedrop", if cfg.rescue_mode { "decoder+vo" } else { "vo" }.into()),
        ];
        for (k, v) in &opts {
            self.set(k, v);
        }
        for (k, v) in &cfg.mpv_options {
            self.set(k, v);
        }
    }

    /// Creates the render context. Needs the GL loader from eframe, so it runs after startup.
    pub fn init_render(&mut self, gpa: GetProcAddress, display: Option<DisplayHandle>) -> anyhow::Result<()> {
        let ctx = self.ctx.clone();
        let r = Renderer::new(self.mpv.ctx.as_ptr(), gpa, display, Box::new(move || ctx.request_repaint()))?;
        *self.renderer.lock().unwrap() = Some(r);
        Ok(())
    }

    /// Must be called with the GL context current (eframe's `on_exit`).
    pub fn shutdown(&mut self) {
        self.cmd("stop", &[]);
        if let Ok(mut r) = self.renderer.lock() {
            r.take();
        }
    }

    pub fn load(&mut self, path: &Path, start: Option<f64>) {
        self.s = State {
            path: Some(path.to_path_buf()),
            speed: self.s.speed,
            volume: self.s.volume,
            mute: self.s.mute,
            sub_scale: self.s.sub_scale,
            ..Default::default()
        };
        self.set("start", &start.map(|s| format!("{s:.1}")).unwrap_or_else(|| "none".into()));
        self.set("pause", "no");
        self.cmd("loadfile", &[&path.to_string_lossy()]);
    }

    pub fn stop(&mut self) {
        self.cmd("stop", &[]);
        self.s.loaded = false;
        self.s.path = None;
    }

    pub fn toggle_pause(&self) {
        self.cmd("cycle", &["pause"]);
    }

    pub fn seek_relative(&self, secs: f64) {
        self.cmd("seek", &[&format!("{secs}"), "relative+exact"]);
    }

    pub fn seek_absolute(&self, secs: f64) {
        self.cmd("seek", &[&format!("{secs}"), "absolute"]);
    }

    pub fn add_chapter(&self, n: i64) {
        if self.s.chapters > 0 {
            self.cmd("add", &["chapter", &n.to_string()]);
        } else {
            self.seek_relative(n as f64 * 300.0);
        }
    }

    pub fn set_volume(&self, v: f64) {
        self.set("volume", &format!("{:.0}", v.clamp(0.0, 200.0)));
    }

    pub fn set_mute(&self, m: bool) {
        self.set("mute", if m { "yes" } else { "no" });
    }

    pub fn set_speed(&self, s: f64) {
        self.set("speed", &format!("{:.2}", s.clamp(0.25, 4.0)));
    }

    /// Lift subtitles above jimmy's controls while they're visible.
    pub fn set_sub_lift(&mut self, lifted: bool) {
        if self.sub_lifted != lifted {
            self.sub_lifted = lifted;
            self.set("sub-margin-y", if lifted { "70" } else { "22" });
        }
    }

    pub fn add(&self, prop: &str, by: f64) {
        self.cmd("add", &[prop, &format!("{by}")]);
    }

    pub fn select_track(&self, kind: TrackKind, id: Option<i64>) {
        let prop = match kind {
            TrackKind::Video => "vid",
            TrackKind::Audio => "aid",
            TrackKind::Sub => "sid",
        };
        self.set(prop, &id.map(|i| i.to_string()).unwrap_or_else(|| "no".into()));
    }

    /// Cycle to the next track of a kind (subtitles also pass through "off").
    pub fn cycle_track(&self, kind: TrackKind) {
        let ids: Vec<i64> = self.s.tracks_of(kind).map(|t| t.id).collect();
        if ids.is_empty() {
            return;
        }
        let cur = self.s.selected(kind).map(|t| t.id);
        let next = match cur.and_then(|c| ids.iter().position(|i| *i == c)) {
            Some(i) if i + 1 < ids.len() => Some(ids[i + 1]),
            Some(_) if kind == TrackKind::Sub => None,
            Some(_) => Some(ids[0]),
            None => Some(ids[0]),
        };
        self.select_track(kind, next);
    }

    /// One frame forward / back (mpv pauses on the new frame).
    pub fn frame_step(&self, forward: bool) {
        self.cmd(if forward { "frame-step" } else { "frame-back-step" }, &[]);
    }

    /// Loop the current file forever (mpv `loop-file`).
    pub fn set_loop_file(&self, on: bool) {
        self.set("loop-file", if on { "inf" } else { "no" });
    }

    /// Back to the start and playing (after the end, or any time).
    pub fn replay(&mut self) {
        self.s.eof = false;
        self.cmd("seek", &["0", "absolute"]);
        self.set("pause", "no");
    }

    pub fn set_rescue(&self, on: bool) {
        self.set("vd-lavc-skiploopfilter", if on { "all" } else { "default" });
        self.set("vd-lavc-fast", if on { "yes" } else { "no" });
        self.set("framedrop", if on { "decoder+vo" } else { "vo" });
    }

    fn refresh_tracks(&mut self) {
        let n: i64 = self.mpv.get_property("track-list/count").unwrap_or(0);
        let mut tracks = Vec::with_capacity(n as usize);
        for i in 0..n {
            let g = |k: &str| self.mpv.get_property::<String>(&format!("track-list/{i}/{k}")).ok();
            let kind = match g("type").as_deref() {
                Some("video") => TrackKind::Video,
                Some("audio") => TrackKind::Audio,
                Some("sub") => TrackKind::Sub,
                _ => continue,
            };
            let flag = |k: &str| self.mpv.get_property::<bool>(&format!("track-list/{i}/{k}")).unwrap_or(false);
            tracks.push(Track {
                id: self.mpv.get_property(&format!("track-list/{i}/id")).unwrap_or(0),
                kind,
                title: g("title"),
                lang: g("lang"),
                codec: g("codec").unwrap_or_default(),
                external: flag("external"),
                selected: flag("selected"),
                channels: self.mpv.get_property(&format!("track-list/{i}/demux-channel-count")).ok(),
            });
        }
        self.s.tracks = tracks;
    }

    /// Drain mpv events. Call once per frame.
    pub fn poll(&mut self) -> Vec<PlayerEvent> {
        let mut out = Vec::new();
        while let Some(ev) = self.mpv.wait_event(0.0) {
            match ev {
                Ok(Event::FileLoaded) => {
                    self.s.loaded = true;
                    self.s.error = None;
                    self.tracks_dirty = true;
                    self.set("start", "none");
                    out.push(PlayerEvent::FileLoaded);
                }
                Ok(Event::EndFile(reason)) => {
                    // 0 = EOF, 2 = stop, 3 = quit, 4 = error, 5 = redirect
                    let eof = reason == 0;
                    if reason == 4 {
                        self.s.error = Some("mpv could not play this file".into());
                    }
                    if self.s.path.is_some() || eof {
                        out.push(PlayerEvent::Ended { eof });
                    }
                }
                Ok(Event::PropertyChange { name, change, .. }) => {
                    let s = &mut self.s;
                    match (name, change) {
                        ("time-pos", PropertyData::Double(v)) => s.time_pos = v,
                        ("duration", PropertyData::Double(v)) => s.duration = v,
                        ("pause", PropertyData::Flag(v)) => s.paused = v,
                        ("volume", PropertyData::Double(v)) => s.volume = v,
                        ("mute", PropertyData::Flag(v)) => s.mute = v,
                        ("chapter", PropertyData::Int64(v)) => s.chapter = v,
                        ("chapters", PropertyData::Int64(v)) => s.chapters = v,
                        ("demuxer-cache-duration", PropertyData::Double(v)) => s.cache_secs = v,
                        ("speed", PropertyData::Double(v)) => s.speed = v,
                        ("sub-delay", PropertyData::Double(v)) => s.sub_delay = v,
                        ("sub-scale", PropertyData::Double(v)) => s.sub_scale = v,
                        ("eof-reached", PropertyData::Flag(v)) => {
                            if v && !s.eof {
                                out.push(PlayerEvent::Ended { eof: true });
                            }
                            s.eof = v;
                        }
                        ("hwdec-current", PropertyData::Str(v)) => s.hwdec = v.to_string(),
                        ("video-codec", PropertyData::Str(v)) => s.video_codec = v.to_string(),
                        ("width", PropertyData::Int64(v)) => s.width = v,
                        ("height", PropertyData::Int64(v)) => s.height = v,
                        ("track-list/count" | "aid" | "sid", _) => self.tracks_dirty = true,
                        _ => {}
                    }
                }
                Ok(_) => {}
                Err(e) => log::debug!("mpv event error: {e}"),
            }
        }
        if self.tracks_dirty {
            self.tracks_dirty = false;
            self.refresh_tracks();
        }
        out
    }

    /// Queue drawing the current video frame over `rect` (the whole window).
    pub fn paint(&self, painter: &egui::Painter, rect: egui::Rect) {
        let renderer = self.renderer.clone();
        let cb = egui_glow::CallbackFn::new(move |info, p| {
            let Ok(guard) = renderer.lock() else { return };
            let Some(r) = guard.as_ref() else { return };
            let fbo = unsafe { p.gl().get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING) };
            let [w, h] = info.screen_size_px;
            r.render(fbo, w as i32, h as i32);
        });
        painter.add(egui::PaintCallback { rect, callback: Arc::new(cb) });
    }
}

