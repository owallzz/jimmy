//! Fix Match (pick the right show/movie) and "Movies or TV Shows?" for new libraries.

use super::draw::*;
use super::{App, FixMatch, Overlay};
use crate::action::{Action, Context};
use crate::config::LibraryKind;
use egui::{Align2, Rect, Sense, pos2, vec2};

impl App {
    // ------------------------------------------------------------------ Fix Match

    /// Fix Match (M, or from the palette) on the selected show/movie, or what's playing.
    /// Shows search TVmaze, movies search Wikipedia.
    pub(crate) fn open_fix_match(&mut self) {
        let key = match self.screen {
            super::Screen::Player => self.playing_key(),
            super::Screen::Browser => self.items.get(self.focus).and_then(|i| self.key_of(i)),
        };
        let Some(key) = key.filter(|k| self.db.item(k).is_some()) else {
            self.toast("Fix Match works on shows and movies in your libraries");
            return;
        };
        let it = self.db.item(&key).cloned().unwrap_or_default();
        let show = self.db.is_show(&key);
        let query = it.title.clone();
        self.fix = Some(FixMatch { key, show, query: query.clone(), ..Default::default() });
        self.overlay = Overlay::FixMatch;
        self.fix_search(it.year);
    }

    pub(crate) fn playing_key(&self) -> Option<String> {
        self.db.file(self.playing_path.as_ref()?).map(|f| f.item.clone())
    }

    fn fix_search(&mut self, year: Option<u32>) {
        let s = self.scrape_settings();
        let Some(f) = self.fix.as_mut() else { return };
        if f.query.trim().is_empty() {
            return;
        }
        f.busy = true;
        f.error = None;
        f.searched = f.query.clone();
        let job = crate::scrape::Job::Search { show: f.show, query: f.query.clone(), year };
        self.scraper.job(job, s);
    }

    pub(crate) fn dispatch_fix_search(&mut self) {
        self.fix_search(None);
    }

    /// Typing goes into the Fix Match search box.
    pub(crate) fn fix_type(&mut self, t: &str) {
        if let Some(f) = self.fix.as_mut() {
            f.query.push_str(t);
            f.focus = 0;
        }
    }

    pub(crate) fn fix_backspace(&mut self) {
        if let Some(f) = self.fix.as_mut() {
            f.query.pop();
            f.focus = 0;
        }
    }

    pub(super) fn fix_action(&mut self, a: Action) {
        let Some(f) = self.fix.clone() else {
            self.overlay = Overlay::None;
            return;
        };
        let n = f.results.len();
        match a {
            Action::Up => self.fix.as_mut().unwrap().focus = f.focus.saturating_sub(1),
            Action::Down => self.fix.as_mut().unwrap().focus = (f.focus + 1).min(n),
            Action::Back => {
                self.fix = None;
                self.overlay = Overlay::None;
            }
            Action::Keyboard => self.open_osk(super::osk::OskTarget::FixMatch),
            Action::Accept if f.focus == 0 => {
                if self.last_was_pad && f.query == f.searched {
                    self.open_osk(super::osk::OskTarget::FixMatch);
                } else {
                    self.fix_search(None);
                }
            }
            Action::Accept => {
                let Some(c) = f.results.get(f.focus - 1).cloned() else { return };
                let Some(req) = self.match_req(&f.key) else { return };
                let s = self.scrape_settings();
                self.scraper.job(crate::scrape::Job::Apply { req, id: c.id.clone() }, s);
                if let Some(f) = self.fix.as_mut() {
                    f.busy = true;
                }
                self.toast(format!("Matching to {}…", c.title));
            }
            _ => {}
        }
    }

    pub(super) fn draw_fix(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let Some(f) = self.fix.clone() else { return out };
        let p = ui.painter().clone();
        let dim = ui.interact(rect, ui.scope_id().with("fix-dim"), Sense::CLICK);
        p.rect_filled(rect, 0.0, black(0.6));
        let w = (rect.width() * 0.6).clamp(420.0, 760.0);
        let osk_h = if self.osk.is_some() { 330.0 } else { 0.0 };
        let h = (rect.height() - 80.0 - osk_h).min(560.0);
        let r = Rect::from_min_size(pos2(rect.center().x - w / 2.0, rect.top() + 40.0), vec2(w, h));
        if dim.clicked() && !r.contains(dim.interact_pointer_pos().unwrap_or_default()) {
            out.push(Action::Back);
        }
        p.rect_filled(r, 0.0, pal.panel);
        let it = self.db.item(&f.key).cloned().unwrap_or_default();
        let x = r.left() + 20.0;
        text(&p, pos2(x, r.top() + 20.0), Align2::LEFT_CENTER, if f.show { "FIX MATCH · TV SHOW · TVMAZE" } else { "FIX MATCH · MOVIE · WIKIPEDIA" }, 11.0, pal.faint);
        let now = match &it.id {
            Some(id) => format!("Now: {} ({id})", it.name()),
            None if !it.info.source.is_empty() => format!("Now: {} (from {})", it.name(), it.info.source),
            None => format!("Now: {} (not matched)", it.name()),
        };
        text_clip(&p, pos2(x, r.top() + 42.0), w - 40.0, &now, 13.0, pal.dim);
        if let Some(folder) = it.folders.first() {
            text_clip(&p, pos2(x, r.top() + 62.0), w - 40.0, &folder.display().to_string(), 12.0, pal.faint);
        }

        // Search box.
        let sb = Rect::from_min_size(pos2(x, r.top() + 80.0), vec2(w - 40.0, 34.0));
        let on = f.focus == 0;
        p.rect_filled(sb, 0.0, if on { pal.raise } else { pal.bg });
        if on {
            p.rect_filled(Rect::from_min_max(pos2(sb.left(), sb.bottom() - 2.0), sb.right_bottom()), 0.0, pal.accent);
        }
        text(&p, pos2(sb.left() + 10.0, sb.center().y), Align2::LEFT_CENTER, "/", 14.0, pal.accent);
        let qr = text_clip(&p, pos2(sb.left() + 26.0, sb.center().y), sb.width() - 120.0, &f.query, 15.0, pal.text);
        if on {
            p.rect_filled(Rect::from_min_size(pos2(qr.right() + 1.0, sb.center().y - 8.0), vec2(2.0, 16.0)), 0.0, pal.accent);
        }
        let state = if f.busy { "searching…" } else { "Enter searches" };
        text(&p, pos2(sb.right() - 10.0, sb.center().y), Align2::RIGHT_CENTER, state, 12.0, pal.faint);
        if ui.interact(sb, ui.scope_id().with("fix-box"), Sense::CLICK).clicked() {
            if let Some(ff) = self.fix.as_mut() {
                ff.focus = 0;
            }
        }

        // Results.
        let mut y = sb.bottom() + 12.0;
        let row_h = 52.0;
        let list_bottom = r.bottom() - 44.0;
        if let Some(e) = &f.error {
            text_wrap(&p, pos2(x, y + 6.0), w - 40.0, e, 14.0, pal.boost, 3);
        } else if f.results.is_empty() && !f.busy {
            text(&p, pos2(x, y + 14.0), Align2::LEFT_CENTER, "No results. Edit the title and search again.", 14.0, pal.dim);
        }
        let visible = (((list_bottom - y) / row_h).floor() as usize).max(1);
        let first = f.focus.saturating_sub(1).saturating_sub(visible.saturating_sub(1));
        for (i, c) in f.results.iter().enumerate().skip(first).take(visible) {
            let rr = Rect::from_min_size(pos2(r.left(), y), vec2(w, row_h));
            let focused = f.focus == i + 1;
            let current = it.id.as_deref() == Some(c.id.as_str());
            if focused {
                p.rect_filled(rr, 0.0, pal.accent);
            }
            let (fg, dimc) = if focused { (pal.ink, pal.ink) } else { (pal.text, pal.dim) };
            let title = match c.year {
                Some(yr) => format!("{} ({yr})", c.title),
                None => c.title.clone(),
            };
            let tr = text_clip(&p, pos2(x, rr.top() + 17.0), w - 160.0, &title, 16.0, fg);
            if current {
                text(&p, pos2(tr.right() + 10.0, rr.top() + 17.0), Align2::LEFT_CENTER, "CURRENT", 10.0, if focused { pal.ink } else { pal.ok });
            }
            let src = if c.id.starts_with("tvmaze") { "TVmaze" } else { "Wikipedia" };
            text(&p, pos2(rr.right() - 20.0, rr.top() + 17.0), Align2::RIGHT_CENTER, src, 11.0, dimc);
            let ov: String = c.overview.chars().take(160).collect();
            text_clip(&p, pos2(x, rr.top() + 37.0), w - 40.0, &ov, 12.0, dimc);
            if ui.interact(rr, ui.scope_id().with(("fixr", i)), Sense::CLICK).clicked() {
                if let Some(ff) = self.fix.as_mut() {
                    ff.focus = i + 1;
                }
                out.push(Action::Accept);
            }
            y += row_h;
        }
        let list: &[(Action, &str)] = &[(Action::Accept, if f.focus == 0 { "Search" } else { "Use this match" }), (Action::Keyboard, "Keyboard"), (Action::Back, "Cancel")];
        out.extend(self.hints(ui, pos2(x, r.bottom() - 22.0), Context::List, list, 12.0, 18.0, pal.dim, pal.raise));
        out
    }

    // ------------------------------------------------------------------ Movies or TV Shows?

    pub(super) fn lib_kind_action(&mut self, a: Action) {
        match a {
            Action::Up | Action::Left => self.lib_focus = 0,
            Action::Down | Action::Right => self.lib_focus = 1,
            Action::Back => {
                self.lib_pending = None;
                self.overlay = Overlay::None;
            }
            Action::Accept => {
                self.overlay = Overlay::None;
                if let Some(dir) = self.lib_pending.take() {
                    let kind = if self.lib_focus == 1 { LibraryKind::Shows } else { LibraryKind::Movies };
                    self.add_library(&dir, kind);
                }
            }
            _ => {}
        }
    }

    pub(super) fn draw_lib_kind(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let p = ui.painter().clone();
        let _ = ui.interact(rect, ui.scope_id().with("lk-dim"), Sense::CLICK);
        p.rect_filled(rect, 0.0, black(0.6));
        let w = 520.0;
        let r = Rect::from_center_size(rect.center(), vec2(w, 250.0));
        p.rect_filled(r, 0.0, pal.panel);
        let x = r.left() + 20.0;
        text(&p, pos2(x, r.top() + 22.0), Align2::LEFT_CENTER, "ADD LIBRARY FOLDER", 11.0, pal.faint);
        let dir = self.lib_pending.as_ref().map(|d| d.display().to_string()).unwrap_or_default();
        text_clip(&p, pos2(x, r.top() + 46.0), w - 40.0, &dir, 14.0, pal.dim);
        let opts = [
            ("Movies", "One movie per file, or per “Title (Year)” folder"),
            ("TV Shows", "Each folder inside is a show: Show (Year)/Season 1/S01E01.mkv"),
        ];
        for (i, (label, desc)) in opts.iter().enumerate() {
            let rr = Rect::from_min_size(pos2(r.left(), r.top() + 70.0 + i as f32 * 60.0), vec2(w, 56.0));
            let focused = i == self.lib_focus;
            if focused {
                p.rect_filled(rr, 0.0, pal.accent);
            }
            let (fg, dimc) = if focused { (pal.ink, pal.ink) } else { (pal.text, pal.dim) };
            text(&p, pos2(x, rr.top() + 18.0), Align2::LEFT_CENTER, *label, 17.0, fg);
            text_clip(&p, pos2(x, rr.top() + 39.0), w - 40.0, desc, 12.0, dimc);
            if ui.interact(rr, ui.scope_id().with(("lk", i)), Sense::CLICK).clicked() {
                self.lib_focus = i;
                out.push(Action::Accept);
            }
        }
        let list: &[(Action, &str)] = &[(Action::Accept, "Add"), (Action::Back, "Cancel")];
        out.extend(self.hints(ui, pos2(x, r.bottom() - 22.0), Context::List, list, 12.0, 18.0, pal.dim, pal.raise));
        out
    }

    // ------------------------------------------------------------------ Recently added

    pub(crate) fn rebuild_added_view(&mut self) {
        let paths = self.db.recently_added(120);
        let items: Vec<super::Item> = paths
            .into_iter()
            .filter_map(|p| self.entry_map.get(&p).cloned())
            .map(|e| self.item(e))
            .collect();
        if self.all_cache.is_some() {
            self.loading = false;
        }
        self.set_items(items);
    }
}
