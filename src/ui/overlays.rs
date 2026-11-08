//! Menu drawer, command palette and the tracks panel.

use super::draw::*;
use super::{App, Overlay, Screen, Source};
use crate::action::{Action, Context};
use crate::player::TrackKind;
use crate::theme;
use egui::{Align2, Rect, Sense, pos2, vec2};
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use std::path::PathBuf;

#[derive(Clone)]
pub(crate) enum MenuItem {
    Header(&'static str),
    Source(Source, String, Option<usize>),
    Settings,
    Quit,
}

#[derive(Clone)]
pub(crate) enum Cmd {
    Act(Action),
    Open(PathBuf),
    Theme(usize),
    Continue,
    Recent,
}

pub(crate) struct PaletteRow {
    pub cat: String,
    pub name: String,
    pub cmd: Cmd,
}

impl App {
    // ------------------------------------------------------------------ menu

    pub(crate) fn menu_items(&self) -> Vec<MenuItem> {
        let mut v = vec![
            MenuItem::Header("Library"),
            MenuItem::Source(Source::Continue, "Continue watching".into(), Some(self.hist.recent(true).len())),
            MenuItem::Source(Source::Recent, "Recently played".into(), None),
            MenuItem::Header("Folders"),
        ];
        for r in &self.cfg.roots {
            let name = r.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| r.display().to_string());
            v.push(MenuItem::Source(Source::Dir(r.clone()), name, None));
        }
        v.push(MenuItem::Header(""));
        v.push(MenuItem::Settings);
        v.push(MenuItem::Quit);
        v
    }

    fn menu_selectable(items: &[MenuItem]) -> Vec<usize> {
        items.iter().enumerate().filter(|(_, i)| !matches!(i, MenuItem::Header(_))).map(|(i, _)| i).collect()
    }

    pub(crate) fn menu_current(&self) -> usize {
        let items = self.menu_items();
        let sel = Self::menu_selectable(&items);
        sel.iter()
            .position(|&i| match &items[i] {
                MenuItem::Source(s, ..) => match (s, &self.source) {
                    (Source::Dir(r), Source::Dir(d)) => d.starts_with(r),
                    (a, b) => a == b,
                },
                _ => false,
            })
            .unwrap_or(0)
    }

    pub(super) fn menu_action(&mut self, ctx: &egui::Context, a: Action) {
        let items = self.menu_items();
        let sel = Self::menu_selectable(&items);
        match a {
            Action::Up => self.menu_focus = self.menu_focus.saturating_sub(1),
            Action::Down => self.menu_focus = (self.menu_focus + 1).min(sel.len().saturating_sub(1)),
            Action::Home => self.menu_focus = 0,
            Action::End => self.menu_focus = sel.len().saturating_sub(1),
            Action::Back | Action::Menu | Action::Left => self.overlay = Overlay::None,
            Action::Accept | Action::Right => {
                let Some(&i) = sel.get(self.menu_focus) else { return };
                self.overlay = Overlay::None;
                match items[i].clone() {
                    MenuItem::Source(s, ..) => self.open_source(s),
                    MenuItem::Settings => self.dispatch(ctx, Action::Settings),
                    MenuItem::Quit => self.dispatch(ctx, Action::Quit),
                    MenuItem::Header(_) => {}
                }
            }
            _ => {}
        }
    }

    pub(super) fn draw_menu(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let p = ui.painter().clone();
        let mut out = Vec::new();
        let dim = ui.interact(rect, ui.scope_id().with("menu-dim"), Sense::click());
        p.rect_filled(rect, 0.0, black(0.55));
        let w = 320.0;
        let r = Rect::from_min_size(rect.min, vec2(w, rect.height()));
        p.rect_filled(r, 0.0, pal.panel);
        if dim.clicked() && !r.contains(dim.interact_pointer_pos().unwrap_or_default()) {
            out.push(Action::Back);
        }
        let items = self.menu_items();
        let sel = Self::menu_selectable(&items);
        let mut y = r.top() + 14.0;
        let mut quit_y = None;
        for (i, item) in items.iter().enumerate() {
            match item {
                MenuItem::Header(h) => {
                    if h.is_empty() {
                        // push Settings/Quit to the bottom
                        let rest = (items.len() - i - 1) as f32 * 38.0;
                        y = (r.bottom() - 52.0 - rest).max(y + 12.0);
                        quit_y = Some(y);
                    } else {
                        text(&p, pos2(r.left() + 20.0, y + 14.0), Align2::LEFT_CENTER, format!("# {}", h.to_uppercase()), 11.0, pal.faint);
                        y += 26.0;
                    }
                }
                _ => {
                    let row = Rect::from_min_size(pos2(r.left(), y), vec2(w, 38.0));
                    let si = sel.iter().position(|&s| s == i).unwrap_or(0);
                    let focused = si == self.menu_focus;
                    let current = matches!(item, MenuItem::Source(s, ..) if *s == self.source || matches!((s, &self.source), (Source::Dir(a), Source::Dir(b)) if b.starts_with(a)));
                    if focused {
                        p.rect_filled(row, 0.0, pal.accent);
                    }
                    let fg = if focused { pal.ink } else if current { pal.accent } else { pal.text };
                    let (icon, label, count) = match item {
                        MenuItem::Source(Source::Continue, l, c) => ("▶", l.clone(), *c),
                        MenuItem::Source(Source::Recent, l, c) => ("◷", l.clone(), *c),
                        MenuItem::Source(Source::Dir(_), l, c) => ("▤", l.clone(), *c),
                        MenuItem::Settings => ("⚙", "Settings".into(), None),
                        MenuItem::Quit => ("×", "Quit jimmy".into(), None),
                        MenuItem::Header(_) => unreachable!(),
                    };
                    let ic = if focused { pal.ink } else { pal.faint };
                    text(&p, pos2(row.left() + 20.0, row.center().y), Align2::LEFT_CENTER, icon, 13.0, ic);
                    text_clip(&p, pos2(row.left() + 46.0, row.center().y), w - 90.0, &label, 16.0, fg);
                    if let Some(c) = count.filter(|c| *c > 0) {
                        text(&p, pos2(row.right() - 20.0, row.center().y), Align2::RIGHT_CENTER, c.to_string(), 12.0, ic);
                    }
                    let resp = ui.interact(row, ui.scope_id().with(("menu", i)), Sense::click());
                    if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                        self.menu_focus = si;
                    }
                    if resp.clicked() {
                        self.menu_focus = si;
                        out.push(Action::Accept);
                    }
                    y += 38.0;
                }
            }
        }
        let _ = quit_y;
        if self.cfg.show_hints {
            let list: &[(Action, &str)] = &[(Action::Down, "Move"), (Action::Accept, "Open"), (Action::Back, "Close")];
            out.extend(self.hints(ui, pos2(r.left() + 20.0, r.bottom() - 20.0), Context::List, list, 12.0, 18.0, pal.dim, pal.raise));
        }
        out
    }

    // ------------------------------------------------------------------ palette

    pub(crate) fn palette_rows(&self) -> Vec<PaletteRow> {
        let mut rows: Vec<PaletteRow> = Action::ALL
            .iter()
            .filter(|a| a.in_palette())
            .map(|a| PaletteRow { cat: a.category().into(), name: a.name().into(), cmd: Cmd::Act(*a) })
            .collect();
        rows.push(PaletteRow { cat: "Go to".into(), name: "Continue watching".into(), cmd: Cmd::Continue });
        rows.push(PaletteRow { cat: "Go to".into(), name: "Recently played".into(), cmd: Cmd::Recent });
        for r in &self.cfg.roots {
            rows.push(PaletteRow { cat: "Go to".into(), name: r.display().to_string(), cmd: Cmd::Open(r.clone()) });
        }
        for i in 0..theme::count() {
            rows.push(PaletteRow { cat: "Color scheme".into(), name: theme::name_at(i).into(), cmd: Cmd::Theme(i) });
        }
        rows
    }

    pub(crate) fn palette_matches(&self) -> Vec<PaletteRow> {
        let rows = self.palette_rows();
        let q = self.palette_query.trim();
        if q.is_empty() {
            // Most useful first when nothing is typed.
            let ctx_first = |r: &PaletteRow| match (&r.cmd, self.palette_from) {
                (Cmd::Act(a), Screen::Player) => !matches!(a.category(), "Playback" | "Audio" | "Subtitles" | "Display"),
                (Cmd::Act(_), Screen::Browser) => false,
                _ => true,
            };
            let mut rows = rows;
            rows.sort_by_key(|r| ctx_first(r));
            return rows;
        }
        let mut m = nucleo_matcher::Matcher::new(nucleo_matcher::Config::DEFAULT);
        let pat = Pattern::parse(q, CaseMatching::Ignore, Normalization::Smart);
        let hay: Vec<String> = rows.iter().map(|r| format!("{} {}", r.cat, r.name)).collect();
        let mut scored: Vec<(usize, u32)> = pat
            .match_list(hay.iter().enumerate().map(|(i, s)| Hay(i, s.as_str())), &mut m)
            .into_iter()
            .map(|(h, s)| (h.0, s))
            .collect();
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        let mut rows: Vec<Option<PaletteRow>> = rows.into_iter().map(Some).collect();
        scored.into_iter().filter_map(|(i, _)| rows[i].take()).collect()
    }

    pub(super) fn palette_action(&mut self, ctx: &egui::Context, a: Action) {
        let n = self.palette_matches().len();
        match a {
            Action::Up => self.palette_focus = self.palette_focus.saturating_sub(1),
            Action::Down => self.palette_focus = (self.palette_focus + 1).min(n.saturating_sub(1)),
            Action::PageUp => self.palette_focus = self.palette_focus.saturating_sub(8),
            Action::PageDown => self.palette_focus = (self.palette_focus + 8).min(n.saturating_sub(1)),
            Action::Back => self.overlay = Overlay::None,
            Action::Accept => {
                let Some(row) = self.palette_matches().into_iter().nth(self.palette_focus) else { return };
                self.overlay = Overlay::None;
                match row.cmd {
                    Cmd::Act(a) => self.dispatch(ctx, a),
                    Cmd::Open(p) => {
                        if self.screen == Screen::Player {
                            self.stop();
                        }
                        self.open_dir(p, None);
                    }
                    Cmd::Theme(i) => self.set_theme(ctx, i),
                    Cmd::Continue | Cmd::Recent => {
                        if self.screen == Screen::Player {
                            self.stop();
                        }
                        self.open_source(if matches!(row.cmd, Cmd::Continue) { Source::Continue } else { Source::Recent });
                    }
                }
            }
            _ => {}
        }
    }

    pub(super) fn draw_palette(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let p = ui.painter().clone();
        let dim = ui.interact(rect, ui.scope_id().with("pal-dim"), Sense::click());
        p.rect_filled(rect, 0.0, black(0.55));
        let w = (rect.width() * 0.6).clamp(420.0, 820.0);
        let row_h = 38.0;
        let max_rows = 10usize;
        let matches = self.palette_matches();
        let shown = matches.len().min(max_rows);
        let h = 50.0 + row_h * shown.max(1) as f32 + 34.0;
        let r = Rect::from_min_size(pos2(rect.center().x - w / 2.0, rect.top() + 90.0), vec2(w, h));
        if dim.clicked() && !r.contains(dim.interact_pointer_pos().unwrap_or_default()) {
            out.push(Action::Back);
        }
        p.rect_filled(r, 0.0, pal.panel);

        // input row
        let ir = Rect::from_min_size(r.min, vec2(w, 50.0));
        p.rect_filled(ir, 0.0, pal.raise);
        text(&p, pos2(ir.left() + 16.0, ir.center().y), Align2::LEFT_CENTER, ">", 20.0, pal.accent);
        let edit_rect = Rect::from_min_max(pos2(ir.left() + 40.0, ir.top() + 10.0), pos2(ir.right() - 80.0, ir.bottom() - 10.0));
        let before = self.palette_query.clone();
        let edit = egui::TextEdit::singleline(&mut self.palette_query)
            .frame(egui::Frame::NONE)
            .font(font(20.0))
            .text_color(pal.text)
            .hint_text(egui::RichText::new("type a command, folder or color scheme").color(pal.faint).font(font(20.0)))
            .desired_width(edit_rect.width());
        let resp = ui.put(edit_rect, edit);
        resp.request_focus();
        if self.palette_query != before {
            self.palette_focus = 0;
        }
        text(&p, pos2(ir.right() - 16.0, ir.center().y), Align2::RIGHT_CENTER, format!("{} / {}", matches.len(), self.palette_rows().len()), 12.0, pal.faint);

        // results (scroll so the focus is visible)
        let first = self.palette_focus.saturating_sub(max_rows - 1);
        for (vi, (i, row)) in matches.iter().enumerate().skip(first).take(max_rows).enumerate() {
            let rr = Rect::from_min_size(pos2(r.left(), ir.bottom() + vi as f32 * row_h), vec2(w, row_h));
            let focused = i == self.palette_focus;
            if focused {
                p.rect_filled(rr, 0.0, pal.accent);
            }
            let y = rr.center().y;
            let (fg, catc) = if focused { (pal.ink, pal.ink) } else { (pal.text, pal.faint) };
            text_clip(&p, pos2(rr.left() + 16.0, y), 110.0, &row.cat.to_uppercase(), 10.0, catc);
            text_clip(&p, pos2(rr.left() + 136.0, y), w - 300.0, &row.name, 16.0, fg);
            // bindings for the context the palette was opened from
            if let Cmd::Act(a) = row.cmd {
                let c = match self.palette_from {
                    Screen::Browser => Context::Browser,
                    Screen::Player => Context::Player,
                };
                let mut caps: Vec<(String, Option<egui::Color32>)> = Vec::new();
                if let Some(k) = self.binds.key_for(c, a) {
                    caps.push((k.label(), None));
                }
                if self.pad_name.is_some() || self.last_was_pad {
                    if let Some(pd) = self.binds.pad_for(c, a) {
                        caps.push((pd.label(self.pad_kind).to_string(), pd.color(self.pad_kind)));
                    }
                }
                let mut x = rr.right() - 16.0;
                for (cap, col) in caps.iter().rev() {
                    let cw = measure(&p, cap, 11.0).x.max(11.0 * 1.7 - 8.0) + 8.0;
                    x -= cw;
                    let (cfg, cbg) = if focused { (pal.ink, with_alpha(pal.ink, 0.18)) } else { (col.unwrap_or(pal.text), pal.raise) };
                    chip(&p, pos2(x, y), cap, 11.0, cfg, cbg);
                    x -= 6.0;
                }
            } else if let Cmd::Theme(i) = row.cmd {
                if i == self.theme_idx() {
                    text(&p, pos2(rr.right() - 16.0, y), Align2::RIGHT_CENTER, "current", 11.0, catc);
                }
            }
            let resp = ui.interact(rr, ui.scope_id().with(("pal", i)), Sense::click());
            if resp.clicked() {
                self.palette_focus = i;
                out.push(Action::Accept);
            }
        }
        if matches.is_empty() {
            text(&p, pos2(r.left() + 136.0, ir.bottom() + row_h / 2.0), Align2::LEFT_CENTER, "no matching command", 15.0, pal.dim);
        }
        let fr = Rect::from_min_max(pos2(r.left(), r.bottom() - 34.0), r.right_bottom());
        p.rect_filled(fr, 0.0, pal.bg);
        if self.cfg.show_hints {
            let list: &[(Action, &str)] = &[(Action::Down, "Move"), (Action::Accept, "Run"), (Action::Back, "Close")];
            out.extend(self.hints(ui, pos2(fr.left() + 16.0, fr.center().y), Context::List, list, 12.0, 18.0, pal.dim, pal.raise));
        }
        out
    }

    // ------------------------------------------------------------------ tracks

    /// Rows of the tracks panel for the current tab: (label, meta, selected, kind of row).
    fn track_rows(&self) -> Vec<TrackRow> {
        let Some(pl) = &self.player else { return vec![] };
        let s = &pl.s;
        let mut v = Vec::new();
        match self.tracks_tab {
            0 => {
                v.push(TrackRow::Select(TrackKind::Sub, None, "Off".into(), String::new(), s.selected(TrackKind::Sub).is_none()));
                for t in s.tracks_of(TrackKind::Sub) {
                    v.push(TrackRow::Select(TrackKind::Sub, Some(t.id), t.label(), t.meta(), t.selected));
                }
                v.push(TrackRow::Header("Adjust"));
                v.push(TrackRow::Step(Adj::SubDelay, "Delay".into(), format!("{:+.2} s", s.sub_delay)));
                v.push(TrackRow::Step(Adj::SubSize, "Size".into(), format!("{:.0}%", s.sub_scale * 100.0)));
                v.push(TrackRow::Step(Adj::SubBox, "Background".into(), if self.cfg.sub_background_box { "box".into() } else { "outline".into() }));
            }
            1 => {
                for t in s.tracks_of(TrackKind::Audio) {
                    v.push(TrackRow::Select(TrackKind::Audio, Some(t.id), t.label(), t.meta(), t.selected));
                }
                if s.tracks_of(TrackKind::Audio).next().is_none() {
                    v.push(TrackRow::Info("No audio tracks".into()));
                }
                v.push(TrackRow::Header("Adjust"));
                v.push(TrackRow::Step(Adj::Volume, "Volume".into(), format!("{:.0}%", self.vol)));
            }
            _ => {
                for t in s.tracks_of(TrackKind::Video) {
                    v.push(TrackRow::Select(TrackKind::Video, Some(t.id), t.label(), t.meta(), t.selected));
                }
                v.push(TrackRow::Header("Adjust"));
                v.push(TrackRow::Step(Adj::Speed, "Speed".into(), format!("{:.2}×", s.speed)));
                v.push(TrackRow::Step(Adj::Rescue, "Rescue mode".into(), if self.cfg.rescue_mode { "on".into() } else { "off".into() }));
                let dec = if s.hwdec.is_empty() || s.hwdec == "no" { "software".to_string() } else { s.hwdec.clone() };
                v.push(TrackRow::Info(format!("decoder: {dec} · {} {}x{}", s.video_codec.split_whitespace().next().unwrap_or(""), s.width, s.height)));
            }
        }
        v
    }

    fn track_selectable(rows: &[TrackRow]) -> Vec<usize> {
        rows.iter().enumerate().filter(|(_, r)| matches!(r, TrackRow::Select(..) | TrackRow::Step(..))).map(|(i, _)| i).collect()
    }

    pub(super) fn tracks_action(&mut self, a: Action) {
        let rows = self.track_rows();
        let sel = Self::track_selectable(&rows);
        let focused = sel.get(self.tracks_focus).and_then(|&i| rows.get(i)).cloned();
        match a {
            Action::Up => self.tracks_focus = self.tracks_focus.saturating_sub(1),
            Action::Down => self.tracks_focus = (self.tracks_focus + 1).min(sel.len().saturating_sub(1)),
            Action::NextTab | Action::PrevTab => {
                self.tracks_tab = if a == Action::NextTab { (self.tracks_tab + 1) % 3 } else { (self.tracks_tab + 2) % 3 };
                self.tracks_focus = 0;
            }
            Action::Back | Action::Tracks | Action::Menu => self.overlay = Overlay::None,
            Action::Accept | Action::Left | Action::Right => match focused {
                Some(TrackRow::Select(kind, id, ..)) if a == Action::Accept => {
                    if let Some(p) = &self.player {
                        p.select_track(kind, id);
                    }
                }
                Some(TrackRow::Step(adj, ..)) => {
                    let d = if a == Action::Left { -1.0 } else { 1.0 };
                    self.adjust(adj, d);
                }
                _ => {}
            },
            _ => {}
        }
    }

    fn adjust(&mut self, adj: Adj, d: f64) {
        match adj {
            Adj::SubDelay => {
                if let Some(p) = &self.player {
                    p.add("sub-delay", 0.1 * d);
                }
            }
            Adj::SubSize => {
                if let Some(p) = &self.player {
                    p.add("sub-scale", 0.1 * d);
                }
            }
            Adj::SubBox => {
                self.cfg.sub_background_box = !self.cfg.sub_background_box;
                self.cfg.save();
                let cfg = self.cfg.clone();
                if let Some(p) = self.player.as_mut() {
                    p.apply_config(&cfg);
                }
            }
            Adj::Volume => self.set_volume(self.vol + 5.0 * d, true),
            Adj::Speed => {
                if let Some(p) = &self.player {
                    p.set_speed(p.s.speed + 0.25 * d);
                }
            }
            Adj::Rescue => {
                self.cfg.rescue_mode = !self.cfg.rescue_mode;
                self.cfg.save();
                if let Some(p) = &self.player {
                    p.set_rescue(self.cfg.rescue_mode);
                }
            }
        }
    }

    pub(super) fn draw_tracks(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let p = ui.painter().clone();
        let dim = ui.interact(rect, ui.scope_id().with("tr-dim"), Sense::click());
        p.rect_filled(rect, 0.0, black(0.5));
        let w = (rect.width() * 0.32).clamp(340.0, 440.0);
        let r = Rect::from_min_max(pos2(rect.right() - w, rect.top()), rect.right_bottom());
        if dim.clicked() && !r.contains(dim.interact_pointer_pos().unwrap_or_default()) {
            out.push(Action::Back);
        }
        p.rect_filled(r, 0.0, pal.panel);

        // tabs
        let tr = Rect::from_min_size(r.min, vec2(w, 42.0));
        p.rect_filled(tr, 0.0, pal.bg);
        let tabs = ["SUBTITLES", "AUDIO", "VIDEO"];
        let lb = self.glyph(Context::List, Action::PrevTab).map(|g| g.0).unwrap_or_default();
        let rb = self.glyph(Context::List, Action::NextTab).map(|g| g.0).unwrap_or_default();
        let lw = if lb.is_empty() { 8.0 } else { chip(&p, pos2(tr.left() + 8.0, tr.center().y), &lb, 11.0, pal.text, pal.raise).right() + 4.0 - tr.left() };
        let rw = if rb.is_empty() { 8.0 } else { measure(&p, &rb, 11.0).x.max(11.0 * 1.7 - 8.0) + 8.0 + 12.0 };
        if !rb.is_empty() {
            chip(&p, pos2(tr.right() - rw + 4.0, tr.center().y), &rb, 11.0, pal.text, pal.raise);
        }
        let tw = (w - lw - rw) / 3.0;
        for (i, t) in tabs.iter().enumerate() {
            let cell = Rect::from_min_size(pos2(tr.left() + lw + i as f32 * tw, tr.top() + 6.0), vec2(tw - 4.0, 30.0));
            let on = i == self.tracks_tab;
            if on {
                p.rect_filled(cell, 0.0, pal.accent);
            }
            text(&p, cell.center(), Align2::CENTER_CENTER, *t, 12.0, if on { pal.ink } else { pal.dim });
            if ui.interact(cell, ui.scope_id().with(("tab", i)), Sense::click()).clicked() {
                self.tracks_tab = i;
                self.tracks_focus = 0;
            }
        }

        let rows = self.track_rows();
        let sel = Self::track_selectable(&rows);
        let mut y = tr.bottom() + 8.0;
        if !rows.is_empty() && !matches!(rows[0], TrackRow::Header(_)) {
            text(&p, pos2(r.left() + 16.0, y + 10.0), Align2::LEFT_CENTER, "# TRACK", 11.0, pal.faint);
            y += 24.0;
        }
        for (i, row) in rows.iter().enumerate() {
            let si = sel.iter().position(|&s| s == i);
            let focused = si == Some(self.tracks_focus);
            match row {
                TrackRow::Header(h) => {
                    y += 8.0;
                    text(&p, pos2(r.left() + 16.0, y + 10.0), Align2::LEFT_CENTER, format!("# {}", h.to_uppercase()), 11.0, pal.faint);
                    y += 24.0;
                }
                TrackRow::Info(t) => {
                    text_clip(&p, pos2(r.left() + 16.0, y + 14.0), w - 32.0, t, 12.0, pal.dim);
                    y += 28.0;
                }
                TrackRow::Select(_, _, label, meta, selected) => {
                    let rr = Rect::from_min_size(pos2(r.left(), y), vec2(w, 36.0));
                    if focused {
                        p.rect_filled(rr, 0.0, pal.accent);
                    }
                    let cy = rr.center().y;
                    let dot = Rect::from_center_size(pos2(rr.left() + 21.0, cy), vec2(10.0, 10.0));
                    let dot_c = match (*selected, focused) {
                        (true, true) => pal.ink,
                        (true, false) => pal.accent,
                        (false, true) => with_alpha(pal.ink, 0.3),
                        (false, false) => pal.raise,
                    };
                    p.rect_filled(dot, 0.0, dot_c);
                    let fg = if focused { pal.ink } else { pal.text };
                    text_clip(&p, pos2(rr.left() + 38.0, cy), w * 0.55, label, 15.0, fg);
                    text_clip_right(&p, pos2(rr.right() - 16.0, cy), w * 0.4, meta, 12.0, if focused { pal.ink } else { pal.dim });
                    if ui.interact(rr, ui.scope_id().with(("trk", i)), Sense::click()).clicked() {
                        self.tracks_focus = si.unwrap_or(0);
                        out.push(Action::Accept);
                    }
                    y += 36.0;
                }
                TrackRow::Step(_, label, value) => {
                    let rr = Rect::from_min_size(pos2(r.left(), y), vec2(w, 36.0));
                    if focused {
                        p.rect_filled(rr, 0.0, pal.accent);
                    }
                    let cy = rr.center().y;
                    let fg = if focused { pal.ink } else { pal.text };
                    let ac = if focused { pal.ink } else { pal.faint };
                    text(&p, pos2(rr.left() + 16.0, cy), Align2::LEFT_CENTER, label, 15.0, fg);
                    let right = rr.right() - 16.0;
                    let lr = text(&p, pos2(right - 120.0, cy), Align2::LEFT_CENTER, "◀", 11.0, ac);
                    text(&p, pos2(right - 60.0, cy), Align2::CENTER_CENTER, value, 14.0, fg);
                    let rr2 = text(&p, pos2(right, cy), Align2::RIGHT_CENTER, "▶", 11.0, ac);
                    if ui.interact(lr.expand(6.0), ui.scope_id().with(("l", i)), Sense::click()).clicked() {
                        self.tracks_focus = si.unwrap_or(0);
                        out.push(Action::Left);
                    }
                    if ui.interact(rr2.expand(6.0), ui.scope_id().with(("r", i)), Sense::click()).clicked() {
                        self.tracks_focus = si.unwrap_or(0);
                        out.push(Action::Right);
                    }
                    y += 36.0;
                }
            }
        }
        if self.cfg.show_hints {
            let hr = Rect::from_min_max(pos2(r.left(), r.bottom() - 40.0), r.right_bottom());
            p.rect_filled(hr, 0.0, pal.bg);
            let list: &[(Action, &str)] = &[(Action::Accept, "Select"), (Action::Right, "Adjust"), (Action::NextTab, "Tab"), (Action::Back, "Close")];
            out.extend(self.hints(ui, pos2(hr.left() + 16.0, hr.center().y), Context::List, list, 12.0, 16.0, pal.dim, pal.raise));
        }
        out
    }
}

#[derive(Clone, Copy)]
pub(crate) enum Adj {
    SubDelay,
    SubSize,
    SubBox,
    Volume,
    Speed,
    Rescue,
}

#[derive(Clone)]
pub(crate) enum TrackRow {
    Header(&'static str),
    Info(String),
    Select(TrackKind, Option<i64>, String, String, bool),
    Step(Adj, String, String),
}

struct Hay<'a>(usize, &'a str);

impl AsRef<str> for Hay<'_> {
    fn as_ref(&self) -> &str {
        self.1
    }
}
