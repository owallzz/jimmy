//! Settings: tabs across the top (LB/RB), one list of rows per tab.

use super::draw::*;
use super::{App, Overlay, Source};
use crate::action::{Action, Context};
use crate::input::Bindings;
use crate::theme;
use egui::{Align2, Rect, Sense, pos2, vec2};
use std::path::PathBuf;

pub(crate) const TABS: [&str; 8] = ["General", "Library", "Playback", "Subtitles", "Volume & brightness", "Appearance", "Controls", "About"];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Opt {
    StartFullscreen,
    UiScale,
    ShowHints,
    Clock24,
    OsdTimeout,
    Resume,
    Hwdec,
    Rescue,
    SeekSmall,
    SeekLarge,
    Readahead,
    ReadaheadMem,
    SubScale,
    SubBox,
    VolumeMax,
    ResetBoost,
}

#[derive(Clone, Debug)]
pub(crate) enum Row {
    Opt(Opt, &'static str),
    Root(PathBuf),
    AddRoot,
    Theme(usize),
    Bind(Context, Action),
    ResetBindings,
    Info(String, String),
}

const HWDEC: &[&str] = &["auto-safe", "auto-copy-safe", "vaapi", "vaapi-copy", "videotoolbox", "no"];

fn step<T: PartialEq + Copy>(list: &[T], cur: T, dir: i32) -> T {
    let i = list.iter().position(|v| *v == cur).unwrap_or(0) as i32;
    list[(i + dir).clamp(0, list.len() as i32 - 1) as usize]
}

impl App {
    fn opt_value(&self, o: Opt) -> String {
        let c = &self.cfg;
        let onoff = |b: bool| if b { "on".to_string() } else { "off".to_string() };
        match o {
            Opt::StartFullscreen => onoff(c.start_fullscreen),
            Opt::UiScale => format!("{:.0}%", c.ui_scale * 100.0),
            Opt::ShowHints => onoff(c.show_hints),
            Opt::Clock24 => if c.clock_24h { "24-hour".into() } else { "12-hour".into() },
            Opt::OsdTimeout => format!("{:.0} s", c.osd_timeout_ms as f64 / 1000.0),
            Opt::Resume => onoff(c.resume),
            Opt::Hwdec => c.hwdec.clone(),
            Opt::Rescue => onoff(c.rescue_mode),
            Opt::SeekSmall => format!("{:.0} s", c.seek_small_secs),
            Opt::SeekLarge => format!("{:.0} s", c.seek_large_secs),
            Opt::Readahead => format!("{} s", c.readahead_secs),
            Opt::ReadaheadMem => format!("{} MB", c.readahead_mb),
            Opt::SubScale => format!("{:.0}%", c.sub_scale * 100.0),
            Opt::SubBox => if c.sub_background_box { "box".into() } else { "outline".into() },
            Opt::VolumeMax => format!("{}%", c.volume_max),
            Opt::ResetBoost => onoff(c.reset_boost_on_new_file),
        }
    }

    fn opt_adjust(&mut self, ctx: &egui::Context, o: Opt, dir: i32) {
        let c = &mut self.cfg;
        match o {
            Opt::StartFullscreen => c.start_fullscreen = !c.start_fullscreen,
            Opt::UiScale => {
                c.ui_scale = (c.ui_scale + 0.05 * dir as f32).clamp(0.6, 2.5);
                ctx.set_zoom_factor(c.ui_scale);
            }
            Opt::ShowHints => c.show_hints = !c.show_hints,
            Opt::Clock24 => {
                c.clock_24h = !c.clock_24h;
                set_clock_24h(c.clock_24h);
            }
            Opt::OsdTimeout => c.osd_timeout_ms = step(&[1000, 2000, 3000, 4000, 5000, 8000, 10000], c.osd_timeout_ms, dir),
            Opt::Resume => c.resume = !c.resume,
            Opt::Hwdec => {
                let cur = HWDEC.iter().position(|h| *h == c.hwdec).unwrap_or(0) as i32;
                c.hwdec = HWDEC[(cur + dir).rem_euclid(HWDEC.len() as i32) as usize].to_string();
            }
            Opt::Rescue => c.rescue_mode = !c.rescue_mode,
            Opt::SeekSmall => c.seek_small_secs = step(&[3.0, 5.0, 10.0, 15.0, 30.0], c.seek_small_secs, dir),
            Opt::SeekLarge => c.seek_large_secs = step(&[30.0, 60.0, 120.0, 300.0, 600.0], c.seek_large_secs, dir),
            Opt::Readahead => c.readahead_secs = step(&[10, 30, 60, 120, 300], c.readahead_secs, dir),
            Opt::ReadaheadMem => c.readahead_mb = step(&[16, 32, 48, 64, 128, 256], c.readahead_mb, dir),
            Opt::SubScale => c.sub_scale = ((c.sub_scale + 0.1 * dir as f64) * 10.0).round().clamp(4.0, 30.0) / 10.0,
            Opt::SubBox => c.sub_background_box = !c.sub_background_box,
            Opt::VolumeMax => c.volume_max = step(&[100, 125, 150, 175, 200], c.volume_max, dir),
            Opt::ResetBoost => c.reset_boost_on_new_file = !c.reset_boost_on_new_file,
        }
        self.cfg.save();
        if o == Opt::VolumeMax && self.vol > self.cfg.volume_max as f64 {
            self.set_volume(self.cfg.volume_max as f64, false);
        }
        let cfg = self.cfg.clone();
        if let Some(p) = self.player.as_mut() {
            p.apply_config(&cfg);
        }
    }

    pub(crate) fn settings_rows(&self) -> Vec<Row> {
        match self.settings_tab {
            0 => vec![
                Row::Opt(Opt::StartFullscreen, "Start fullscreen"),
                Row::Opt(Opt::UiScale, "Interface size"),
                Row::Opt(Opt::ShowHints, "Button hints"),
                Row::Opt(Opt::Clock24, "Clock"),
                Row::Opt(Opt::OsdTimeout, "Hide player controls after"),
            ],
            1 => {
                let mut v: Vec<Row> = self.cfg.roots.iter().cloned().map(Row::Root).collect();
                v.push(Row::AddRoot);
                v.push(Row::Opt(Opt::Resume, "Resume where I stopped"));
                v
            }
            2 => vec![
                Row::Opt(Opt::Hwdec, "Hardware decoding"),
                Row::Opt(Opt::Rescue, "Rescue mode (slow CPU)"),
                Row::Opt(Opt::SeekSmall, "Small seek"),
                Row::Opt(Opt::SeekLarge, "Large seek"),
                Row::Opt(Opt::Readahead, "Read ahead (slow disks)"),
                Row::Opt(Opt::ReadaheadMem, "Read-ahead memory"),
            ],
            3 => vec![Row::Opt(Opt::SubScale, "Size"), Row::Opt(Opt::SubBox, "Background")],
            4 => {
                let bl = match self.system_backlight() {
                    Some(b) => format!("{:.0}% (system backlight)", b * 100.0),
                    None => "not available on this display".into(),
                };
                vec![
                    Row::Opt(Opt::VolumeMax, "Maximum volume (over 100% = boost)"),
                    Row::Opt(Opt::ResetBoost, "Reset boost for each new file"),
                    Row::Info("Brightness".into(), bl),
                ]
            }
            5 => (0..theme::count()).map(Row::Theme).collect(),
            6 => {
                let mut v: Vec<Row> = bindable_rows(&self.binds).into_iter().map(|(c, a)| Row::Bind(c, a)).collect();
                v.push(Row::ResetBindings);
                v
            }
            _ => {
                let mpv = self.player.as_ref().map(|_| "loaded").unwrap_or("not available");
                vec![
                    Row::Info("jimmy".into(), env!("CARGO_PKG_VERSION").into()),
                    Row::Info("libmpv".into(), mpv.into()),
                    Row::Info("Settings file".into(), crate::config::Config::path().display().to_string()),
                    Row::Info("Bindings file".into(), Bindings::path().display().to_string()),
                    Row::Info("History file".into(), crate::config::History::path().display().to_string()),
                ]
            }
        }
    }

    pub(super) fn settings_action(&mut self, ctx: &egui::Context, a: Action) {
        let rows = self.settings_rows();
        let n = rows.len();
        let row = rows.get(self.settings_focus).cloned();
        match a {
            Action::Up => self.settings_focus = self.settings_focus.saturating_sub(1),
            Action::Down => self.settings_focus = (self.settings_focus + 1).min(n.saturating_sub(1)),
            Action::PageUp => self.settings_focus = self.settings_focus.saturating_sub(10),
            Action::PageDown => self.settings_focus = (self.settings_focus + 10).min(n.saturating_sub(1)),
            Action::Home => self.settings_focus = 0,
            Action::End => self.settings_focus = n.saturating_sub(1),
            Action::NextTab | Action::PrevTab => {
                let t = TABS.len();
                self.settings_tab = if a == Action::NextTab { (self.settings_tab + 1) % t } else { (self.settings_tab + t - 1) % t };
                self.settings_focus = if self.settings_tab == 5 { self.theme_idx() } else { 0 };
            }
            Action::Back | Action::Settings => {
                self.overlay = Overlay::None;
                self.listening = None;
            }
            Action::Left | Action::Right | Action::Accept | Action::Options => {
                let dir = if a == Action::Left { -1 } else { 1 };
                match row {
                    Some(Row::Opt(o, _)) if a != Action::Options => self.opt_adjust(ctx, o, dir),
                    Some(Row::Theme(i)) if a == Action::Accept => self.set_theme(ctx, i),
                    Some(Row::Root(p)) if a == Action::Options => {
                        self.cfg.roots.retain(|r| *r != p);
                        self.cfg.save();
                        self.toast(format!("Removed {}", p.display()));
                    }
                    Some(Row::Root(p)) if a == Action::Accept => {
                        self.overlay = Overlay::None;
                        self.open_dir(p, None);
                    }
                    Some(Row::AddRoot) if a == Action::Accept => {
                        if let Source::Dir(d) = &self.source {
                            if !self.cfg.roots.contains(d) {
                                self.cfg.roots.push(d.clone());
                                self.cfg.save();
                                self.toast(format!("Added {}", d.display()));
                            }
                        } else {
                            self.toast("Open a folder in the browser first");
                        }
                    }
                    Some(Row::Bind(c, act)) => match a {
                        Action::Accept => self.listening = Some((c, act)),
                        Action::Options => {
                            if self.settings_kb {
                                self.binds.clear_keys(c, act);
                            } else {
                                self.binds.clear_pads(c, act);
                            }
                            self.binds.save();
                        }
                        _ => self.settings_kb = !self.settings_kb,
                    },
                    Some(Row::ResetBindings) if a == Action::Accept => {
                        self.binds = Bindings::defaults();
                        let _ = std::fs::remove_file(Bindings::path());
                        self.toast("Controls reset to defaults");
                    }
                    _ => {}
                }
            }
            _ => {}
        }
        // Appearance previews as you move.
        if self.settings_tab == 5 && matches!(a, Action::Up | Action::Down | Action::PageUp | Action::PageDown | Action::Home | Action::End) {
            if let Some(Row::Theme(i)) = self.settings_rows().get(self.settings_focus).cloned() {
                self.set_theme(ctx, i);
            }
        }
    }

    pub(super) fn draw_settings(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let p = ui.painter().clone();
        // Swallow clicks on the background so they don't reach the screen underneath.
        let _ = ui.interact(rect, ui.scope_id().with("settings-bg"), Sense::CLICK);
        p.rect_filled(rect, 0.0, pal.bg);

        // header
        let hy = rect.top() + 22.0;
        let x0 = rect.left() + 20.0;
        let r = text(&p, pos2(x0, hy), Align2::LEFT_CENTER, "Settings / ", 16.0, pal.dim);
        text(&p, pos2(r.right(), hy), Align2::LEFT_CENTER, TABS[self.settings_tab], 16.0, pal.text);
        text(&p, pos2(rect.right() - 20.0, hy), Align2::RIGHT_CENTER, clock_at(now_unix()), 16.0, pal.dim);

        // tab bar
        let ty = rect.top() + 44.0 + 19.0;
        let mut x = x0;
        if let Some((cap, _)) = self.glyph(Context::List, Action::PrevTab) {
            x = chip(&p, pos2(x, ty), &cap, 11.0, pal.text, pal.raise).right() + 8.0;
        }
        for (i, t) in TABS.iter().enumerate() {
            let w = measure(&p, t, 15.0).x + 28.0;
            let cell = Rect::from_min_size(pos2(x, ty - 15.0), vec2(w, 30.0));
            let on = i == self.settings_tab;
            if on {
                p.rect_filled(cell, 0.0, pal.accent);
            }
            text(&p, cell.center(), Align2::CENTER_CENTER, *t, 15.0, if on { pal.ink } else { pal.dim });
            if ui.interact(cell, ui.scope_id().with(("stab", i)), Sense::CLICK).clicked() {
                self.settings_tab = i;
                self.settings_focus = 0;
            }
            x += w + 4.0;
        }
        if let Some((cap, _)) = self.glyph(Context::List, Action::NextTab) {
            chip(&p, pos2(x + 4.0, ty), &cap, 11.0, pal.text, pal.raise);
        }

        // content
        let content = Rect::from_min_max(pos2(rect.left() + 20.0, rect.top() + 94.0), pos2(rect.right() - 20.0, rect.bottom() - 48.0));
        let mut y = content.top();
        if self.settings_tab == 6 {
            // Gamepad / Keyboard switch + listening banner
            let mut sx = content.left();
            for (i, t) in ["GAMEPAD", "KEYBOARD"].iter().enumerate() {
                let on = (i == 1) == self.settings_kb;
                let w = measure(&p, t, 12.0).x + 24.0;
                let cell = Rect::from_min_size(pos2(sx, y), vec2(w, 26.0));
                p.rect_filled(cell, 0.0, if on { pal.raise } else { pal.panel });
                if on {
                    p.rect_filled(Rect::from_min_max(pos2(cell.left(), cell.bottom() - 2.0), cell.right_bottom()), 0.0, pal.accent);
                }
                text(&p, cell.center(), Align2::CENTER_CENTER, *t, 12.0, if on { pal.text } else { pal.dim });
                if ui.interact(cell, ui.scope_id().with(("kbsw", i)), Sense::CLICK).clicked() {
                    self.settings_kb = i == 1;
                }
                sx += w + 4.0;
            }
            text(&p, pos2(sx + 12.0, y + 13.0), Align2::LEFT_CENTER, "◀ ▶ switches · Enter/A rebinds · Del/X clears", 12.0, pal.faint);
            y += 38.0;
            if let Some((_, a)) = self.listening {
                let br = Rect::from_min_size(pos2(content.left(), y), vec2(content.width(), 38.0));
                p.rect_filled(br, 0.0, pal.tint);
                let blink = (ui.input(|i| i.time) * 2.0) as i64 % 2 == 0;
                if blink {
                    p.rect_filled(Rect::from_center_size(pos2(br.left() + 16.0, br.center().y), vec2(10.0, 10.0)), 0.0, pal.accent);
                }
                let what = if self.settings_kb { "Press a key" } else { "Press a button or move a stick" };
                text(&p, pos2(br.left() + 32.0, br.center().y), Align2::LEFT_CENTER, format!("Rebinding “{}”. {what}. Esc cancels.", a.name()), 15.0, pal.text);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
                y += 46.0;
            }
            // column labels
            text(&p, pos2(content.left() + 12.0, y + 12.0), Align2::LEFT_CENTER, "ACTION", 11.0, pal.faint);
            text(&p, pos2(content.right() - 520.0, y + 12.0), Align2::LEFT_CENTER, "CONTEXT", 11.0, pal.faint);
            text(&p, pos2(content.right() - 390.0, y + 12.0), Align2::LEFT_CENTER, if self.settings_kb { "KEY" } else { "BUTTON" }, 11.0, pal.faint);
            text(&p, pos2(content.right() - 190.0, y + 12.0), Align2::LEFT_CENTER, "ALTERNATE", 11.0, pal.faint);
            y += 26.0;
        }

        let rows = self.settings_rows();
        let row_h = 34.0;
        let visible = (((content.bottom() - y) / row_h).floor() as usize).max(1);
        let first = self.settings_focus.saturating_sub(visible.saturating_sub(2)).min(rows.len().saturating_sub(visible));
        let first = if self.settings_focus < visible.saturating_sub(1) { 0 } else { first };
        for (vi, (i, row)) in rows.iter().enumerate().skip(first).take(visible).enumerate() {
            let rr = Rect::from_min_size(pos2(content.left(), y + vi as f32 * row_h), vec2(content.width(), row_h));
            let focused = i == self.settings_focus;
            let listening_here = matches!(row, Row::Bind(c, a) if self.listening == Some((*c, *a)));
            if focused && !listening_here {
                p.rect_filled(rr, 0.0, pal.accent);
            } else if listening_here {
                p.rect_filled(rr, 0.0, pal.tint);
            } else if i % 2 == 1 {
                p.rect_filled(rr, 0.0, pal.alt);
            }
            let fg = if focused && !listening_here { pal.ink } else { pal.text };
            let dimc = if focused && !listening_here { pal.ink } else { pal.dim };
            let cy = rr.center().y;
            let lx = rr.left() + 12.0;
            let rx = rr.right() - 12.0;
            let stepper = |val: &str| {
                text(&p, pos2(rx - 170.0, cy), Align2::LEFT_CENTER, "◀", 11.0, dimc);
                text(&p, pos2(rx - 85.0, cy), Align2::CENTER_CENTER, val, 15.0, fg);
                text(&p, pos2(rx, cy), Align2::RIGHT_CENTER, "▶", 11.0, dimc);
            };
            match row {
                Row::Opt(o, label) => {
                    text(&p, pos2(lx, cy), Align2::LEFT_CENTER, *label, 16.0, fg);
                    stepper(&self.opt_value(*o));
                }
                Row::Root(path) => {
                    text_clip(&p, pos2(lx, cy), rr.width() - 260.0, &path.display().to_string(), 16.0, fg);
                    let cap = self.glyph(Context::List, Action::Options).map(|g| g.0).unwrap_or_default();
                    text(&p, pos2(rx, cy), Align2::RIGHT_CENTER, format!("{cap} remove"), 13.0, dimc);
                }
                Row::AddRoot => {
                    let label = match &self.source {
                        Source::Dir(d) => format!("+ Add current folder ({})", d.display()),
                        _ => "+ Add current folder".into(),
                    };
                    text_clip(&p, pos2(lx, cy), rr.width() - 40.0, &label, 16.0, if focused { pal.ink } else { pal.accent });
                }
                Row::Theme(ti) => {
                    let current = *ti == self.theme_idx();
                    let tp = theme::palette(theme::id_at(*ti));
                    let sw = Rect::from_min_size(pos2(lx, cy - 8.0), vec2(16.0, 16.0));
                    for (k, c) in [tp.bg, tp.panel, tp.text, tp.accent, tp.boost, tp.ok].iter().enumerate() {
                        p.rect_filled(sw.translate(vec2(k as f32 * 16.0, 0.0)), 0.0, *c);
                    }
                    text(&p, pos2(lx + 112.0, cy), Align2::LEFT_CENTER, theme::name_at(*ti), 16.0, fg);
                    if current {
                        text(&p, pos2(rx, cy), Align2::RIGHT_CENTER, "current", 12.0, dimc);
                    }
                }
                Row::Bind(c, a) => {
                    text_clip(&p, pos2(lx, cy), rr.width() - 560.0, a.name(), 16.0, fg);
                    text(&p, pos2(rx - 508.0, cy), Align2::LEFT_CENTER, c.label().to_uppercase(), 11.0, dimc);
                    let caps: Vec<(String, Option<egui::Color32>)> = if self.settings_kb {
                        self.binds.keys_for(*c, *a).into_iter().map(|k| (k.label(), None)).collect()
                    } else {
                        self.binds.pads_for(*c, *a).into_iter().map(|p| (p.label(self.pad_kind).to_string(), p.color(self.pad_kind))).collect()
                    };
                    for slot in 0..2 {
                        let sx = rx - 378.0 + slot as f32 * 200.0;
                        if listening_here && slot == 0 {
                            let label = if self.settings_kb { "PRESS A KEY…" } else { "PRESS A BUTTON…" };
                            chip(&p, pos2(sx, cy), label, 11.0, pal.ink, pal.accent);
                        } else if let Some((cap, col)) = caps.get(slot) {
                            let (cf, cb) = if focused { (pal.ink, with_alpha(pal.ink, 0.15)) } else { (col.unwrap_or(pal.text), pal.raise) };
                            chip(&p, pos2(sx, cy), cap, 12.0, cf, cb);
                        } else {
                            text(&p, pos2(sx, cy), Align2::LEFT_CENTER, "—", 13.0, if focused { pal.ink } else { pal.faint });
                        }
                    }
                }
                Row::ResetBindings => {
                    text(&p, pos2(lx, cy), Align2::LEFT_CENTER, "Reset all controls to defaults", 16.0, if focused { pal.ink } else { pal.boost });
                }
                Row::Info(k, v) => {
                    text(&p, pos2(lx, cy), Align2::LEFT_CENTER, k, 16.0, fg);
                    text_clip_right(&p, pos2(rx, cy), rr.width() * 0.6, v, 14.0, dimc);
                }
            }
            let resp = ui.interact(rr, ui.scope_id().with(("srow", i)), Sense::CLICK);
            if resp.clicked() {
                let was = self.settings_focus;
                self.settings_focus = i;
                if was == i || !matches!(row, Row::Theme(_)) {
                    out.push(Action::Accept);
                } else {
                    out.push(Action::Accept);
                }
            }
        }

        // hints
        if self.cfg.show_hints {
            let hy = rect.bottom() - 20.0;
            let list: &[(Action, &str)] = match self.settings_tab {
                6 => &[(Action::Accept, "Rebind"), (Action::Options, "Clear"), (Action::Right, "Gamepad / Keyboard"), (Action::NextTab, "Section"), (Action::Back, "Close")],
                1 => &[(Action::Accept, "Open / add"), (Action::Options, "Remove"), (Action::NextTab, "Section"), (Action::Back, "Close")],
                5 => &[(Action::Down, "Move (previews)"), (Action::Accept, "Use"), (Action::NextTab, "Section"), (Action::Back, "Close")],
                _ => &[(Action::Right, "Change"), (Action::Down, "Move"), (Action::NextTab, "Section"), (Action::Back, "Close")],
            };
            out.extend(self.hints(ui, pos2(rect.left() + 20.0, hy), Context::List, list, 13.0, 22.0, pal.dim, pal.raise));
        }
        out
    }
}

/// Controls rows: everything bound by default or by the user, grouped by context.
fn bindable_rows(b: &Bindings) -> Vec<(Context, Action)> {
    let d = Bindings::defaults();
    let mut v: Vec<(Context, Action)> = Vec::new();
    for (c, _, a) in d.keys.iter().chain(b.keys.iter()) {
        v.push((*c, *a));
    }
    for (c, _, a) in d.pads.iter().chain(b.pads.iter()) {
        v.push((*c, *a));
    }
    let ctx_order = |c: Context| match c {
        Context::Player => 0,
        Context::Browser => 1,
        Context::List => 2,
        Context::Global => 3,
    };
    let act_order = |a: Action| Action::ALL.iter().position(|x| *x == a).unwrap_or(0);
    v.sort_by_key(|(c, a)| (ctx_order(*c), act_order(*a)));
    v.dedup();
    v
}
