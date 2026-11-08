//! The media browser: header row, file list, info panel, hint bar.

use super::draw::{self, *};
use super::{App, Source};
use crate::action::{Action, Context};
use egui::{Align2, Rect, Sense, pos2, vec2};

const HEAD_H: f32 = 44.0;
const COLS_H: f32 = 28.0;
const ROW_H: f32 = 38.0;
const HINT_H: f32 = 40.0;
const PAD: f32 = 20.0;

impl App {
    pub(super) fn draw_browser(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, pal.bg);

        let detail_w = (rect.width() * 0.3).clamp(300.0, 460.0);
        let list_rect = Rect::from_min_max(pos2(rect.left(), rect.top() + HEAD_H), pos2(rect.right() - detail_w, rect.bottom() - HINT_H));
        let detail_rect = Rect::from_min_max(pos2(rect.right() - detail_w, rect.top() + HEAD_H), rect.right_bottom());

        // ---- header: path · filter · count · clock
        let hy = rect.top() + HEAD_H / 2.0;
        let mut x = rect.left() + PAD;
        let (parent, current) = match &self.source {
            Source::Dir(d) => {
                let cur = d.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| d.display().to_string());
                let par = d.parent().map(|p| {
                    let s = p.display().to_string();
                    if s.ends_with('/') { s } else { s + "/" }
                });
                (par.unwrap_or_default(), cur)
            }
            Source::All => (String::new(), "My media".into()),
            Source::Continue => (String::new(), "Continue watching".into()),
            Source::Recent => (String::new(), "Recently played".into()),
            Source::Movies => (String::new(), "Movies".into()),
            Source::Shows => (String::new(), "TV shows".into()),
            Source::Show(key) => ("TV shows / ".into(), self.catalog.show(key).map(|s| s.name.clone()).unwrap_or_default()),
            Source::Playlists => (String::new(), "Playlists".into()),
            Source::Playlist(pl) => ("Playlists / ".into(), crate::playlist::name(pl)),
            Source::Added => (String::new(), "Recently added".into()),
        };
        let max_path = (rect.width() * 0.35).max(120.0);
        if !parent.is_empty() {
            let cur_w = measure(&p, &current, 16.0).x;
            let avail = (max_path - cur_w).max(40.0);
            let par_w = measure(&p, &parent, 16.0).x.min(avail);
            x = text_clip_start(&p, pos2(x, hy), avail, &parent, 16.0, pal.dim).right();
            let _ = par_w;
        }
        x = text(&p, pos2(x, hy), Align2::LEFT_CENTER, &current, 16.0, pal.text).right() + 18.0;

        let filter_w = (rect.width() * 0.24).clamp(160.0, 460.0);
        let fr = Rect::from_min_size(pos2(x, hy - 14.0), vec2(filter_w, 28.0));
        p.rect_filled(fr, 0.0, if self.filtering { pal.raise } else { pal.panel });
        text(&p, pos2(fr.left() + 10.0, hy), Align2::LEFT_CENTER, "/", 14.0, pal.accent);
        if self.filter.is_empty() && !self.filtering {
            text(&p, pos2(fr.left() + 26.0, hy), Align2::LEFT_CENTER, "filter", 14.0, pal.faint);
        } else {
            let r = text_clip(&p, pos2(fr.left() + 26.0, hy), filter_w - 40.0, &self.filter, 14.0, pal.text);
            if self.filtering {
                let cx = if self.filter.is_empty() { fr.left() + 26.0 } else { r.right() + 1.0 };
                p.rect_filled(Rect::from_min_size(pos2(cx, hy - 8.0), vec2(2.0, 16.0)), 0.0, pal.accent);
                if self.filter.is_empty() {
                    text(&p, pos2(cx + 8.0, hy), Align2::LEFT_CENTER, "type, Enter keeps, Esc clears", 13.0, pal.faint);
                }
            }
        }
        if ui.interact(fr, ui.scope_id().with("filterbox"), Sense::CLICK).clicked() {
            self.filtering = true;
        }
        let count = if self.loading {
            "reading…".to_string()
        } else if self.filter.is_empty() {
            format!("{} items", self.items.len())
        } else {
            format!("{} of {}", self.items.len(), self.all_items.len())
        };
        text(&p, pos2(fr.right() + 18.0, hy), Align2::LEFT_CENTER, count, 14.0, pal.dim);
        text(&p, pos2(rect.right() - PAD, hy), Align2::RIGHT_CENTER, clock_at(now_unix()), 16.0, pal.dim);

        // ---- background art of the selected show/movie, faint behind the list
        let backdrop = self.items.get(self.focus).cloned().and_then(|it| self.info_for(&it).and_then(|i| i.backdrop.clone()));
        if let Some(tex) = backdrop.and_then(|src| self.art(ui.ctx(), &src, crate::scrape::ImgKind::Backdrop)) {
            let [tw, th] = tex.size();
            let (want, have) = (list_rect.width() / list_rect.height(), tw as f32 / th.max(1) as f32);
            let uv = if have > want {
                let u = (1.0 - want / have) / 2.0;
                Rect::from_min_max(pos2(u, 0.0), pos2(1.0 - u, 1.0))
            } else {
                let v = (1.0 - have / want) / 2.0;
                Rect::from_min_max(pos2(0.0, v), pos2(1.0, 1.0 - v))
            };
            p.image(tex.id(), list_rect, uv, egui::Color32::WHITE);
            p.rect_filled(list_rect, 0.0, draw::with_alpha(pal.bg, 0.86));
        }

        // ---- list
        let seasons = match &self.source {
            Source::Show(key) => self.catalog.show(key).map(|s| s.seasons()).unwrap_or_default(),
            _ => vec![],
        };
        if self.grid_active() {
            out.extend(self.draw_grid(ui, list_rect));
        } else {
            let w = list_rect.width();
            let show_prog = w > 640.0;
            let show_len = w > 480.0;
            let c_size = list_rect.right() - PAD;
            let c_len = c_size - 100.0;
            let c_prog = if show_len { c_len - 100.0 - 150.0 } else { c_size - 100.0 - 150.0 };
            let name_left = list_rect.left() + PAD + 26.0;
            let name_right = if show_prog { c_prog - 12.0 } else if show_len { c_len - 100.0 } else { c_size - 100.0 };

            let cy = list_rect.top() + COLS_H / 2.0;
            let label = |s: &str, pos, align| {
                text(&p, pos, align, s, 11.0, pal.faint);
            };
            if seasons.is_empty() {
                label("NAME", pos2(name_left, cy), Align2::LEFT_CENTER);
            } else {
                // Season tabs (◀ ▶ / LT RT switch).
                let mut sx = list_rect.left() + PAD;
                if seasons.len() > 1 {
                    if let Some((cap, _)) = self.glyph(Context::Browser, Action::PrevTab) {
                        sx = chip(&p, pos2(sx, cy), &cap, 10.0, pal.dim, pal.raise).right() + 6.0;
                    }
                }
                for sn in &seasons {
                    let t = format!("SEASON {sn}");
                    let w = measure(&p, &t, 11.0).x + 18.0;
                    let cell = Rect::from_min_size(pos2(sx, cy - 11.0), vec2(w, 22.0));
                    let on = *sn == self.season;
                    if on {
                        p.rect_filled(cell, 0.0, pal.raise);
                        p.rect_filled(Rect::from_min_max(pos2(cell.left(), cell.bottom() - 2.0), cell.right_bottom()), 0.0, pal.accent);
                    }
                    text(&p, cell.center(), Align2::CENTER_CENTER, &t, 11.0, if on { pal.text } else { pal.faint });
                    if ui.interact(cell, ui.scope_id().with(("season", *sn)), Sense::CLICK).clicked() {
                        self.season = *sn;
                        self.focus = 0;
                        self.rebuild_catalog_view();
                    }
                    sx += w + 4.0;
                }
                if seasons.len() > 1 {
                    if let Some((cap, _)) = self.glyph(Context::Browser, Action::NextTab) {
                        chip(&p, pos2(sx + 2.0, cy), &cap, 10.0, pal.dim, pal.raise);
                    }
                }
            }
            if show_prog {
                label("PROGRESS", pos2(c_prog, cy), Align2::LEFT_CENTER);
            }
            if show_len {
                label("LENGTH", pos2(c_len, cy), Align2::RIGHT_CENTER);
            }
            label("SIZE", pos2(c_size, cy), Align2::RIGHT_CENTER);

            let rows_rect = Rect::from_min_max(pos2(list_rect.left(), list_rect.top() + COLS_H), list_rect.right_bottom());
            let visible = ((rows_rect.height() / ROW_H).floor() as usize).max(1);
            let n = self.items.len();

            // Mouse wheel scrolls; keyboard/gamepad focus keeps the selection in view.
            let resp = ui.interact(rows_rect, ui.scope_id().with("rows"), Sense::hover());
            if resp.hovered() {
                let dy = ui.input(|i| i.smooth_scroll_delta.y);
                if dy != 0.0 {
                    self.scroll = (self.scroll - dy / ROW_H).clamp(0.0, n.saturating_sub(visible) as f32);
                }
            }
            // Only follow the selection when it moved, so wheel scrolling isn't undone.
            if self.focus != self.scroll_focus {
                self.scroll_focus = self.focus;
                let focus = self.focus as f32;
                if focus < self.scroll {
                    self.scroll = focus;
                } else if focus >= self.scroll + visible as f32 {
                    self.scroll = focus + 1.0 - visible as f32;
                }
            }
            self.scroll = self.scroll.clamp(0.0, n.saturating_sub(visible) as f32);
            let first = self.scroll.floor() as usize;

            let p_rows = p.with_clip_rect(rows_rect);
            let mut hovered = None;
            for (vis_i, idx) in (first..n.min(first + visible + 1)).enumerate() {
                let item = &self.items[idx].clone();
                let top = rows_rect.top() + vis_i as f32 * ROW_H;
                let r = Rect::from_min_size(pos2(rows_rect.left(), top), vec2(rows_rect.width(), ROW_H));
                let focused = idx == self.focus;
                let h = item.hist.as_ref();
                let watched = h.is_some_and(|h| h.watched);
                if focused {
                    p_rows.rect_filled(r, 0.0, pal.accent);
                } else if idx % 2 == 1 {
                    p_rows.rect_filled(r, 0.0, pal.alt);
                }
                let fg = if focused { pal.ink } else if watched { pal.dim } else { pal.text };
                let dimc = if focused { pal.ink } else { pal.dim };
                let y = r.center().y;
                let icon = if item.entry.is_dir { "▸" } else { "·" };
                let icon_c = if focused { pal.ink } else if item.entry.is_dir { pal.accent } else { pal.faint };
                text(&p_rows, pos2(r.left() + PAD, y), Align2::LEFT_CENTER, icon, 14.0, icon_c);
                let name = self.row_name(item);
                let nr = text_clip(&p_rows, pos2(name_left, y), name_right - name_left, &name, 17.0, fg);
                // Second, quieter text after the name: a show's size, or which folder a file lives in.
                let note = if let Some(key) = &item.show {
                    self.catalog.show(key).map(|s| {
                        let ns = s.seasons().len();
                        let ne = s.episodes.len();
                        format!("{ns} season{} · {ne} episode{}", if ns == 1 { "" } else { "s" }, if ne == 1 { "" } else { "s" })
                    })
                } else if matches!(self.source, Source::All | Source::Continue | Source::Recent) && item.ep.is_none() && self.db.file(&item.entry.path).is_none() {
                    item.entry.path.parent().and_then(|d| d.file_name()).map(|f| f.to_string_lossy().into_owned())
                } else {
                    None
                };
                if let Some(note) = note {
                    let fx = nr.right() + 12.0;
                    if fx + 40.0 < name_right {
                        text_clip(&p_rows, pos2(fx, y), name_right - fx, &note, 13.0, if focused { pal.ink } else { pal.faint });
                    }
                }

                if let Some(key) = &item.show {
                    if show_prog {
                        if let Some(show) = self.catalog.show(key) {
                            let all_watched = show.episodes.iter().all(|(_, f)| self.hist.get(&f.path).is_some_and(|h| h.watched));
                            let (t, c) = if all_watched {
                                ("WATCHED".to_string(), pal.ok)
                            } else {
                                (format!("NEXT {}", show.episodes[self.next_up(show)].0.code()), pal.accent)
                            };
                            text(&p_rows, pos2(c_prog, y), Align2::LEFT_CENTER, t, 11.0, if focused { pal.ink } else { c });
                        }
                    }
                    text(&p_rows, pos2(c_size, y), Align2::RIGHT_CENTER, fmt_size(item.entry.size), 15.0, dimc);
                } else if item.entry.is_dir {
                    if show_prog && self.cfg.roots.contains(&item.entry.path) {
                        text(&p_rows, pos2(c_prog, y), Align2::LEFT_CENTER, "LIBRARY", 11.0, if focused { pal.ink } else { pal.ok });
                    }
                } else {
                    if show_prog {
                        if watched {
                            text(&p_rows, pos2(c_prog, y), Align2::LEFT_CENTER, "WATCHED", 11.0, if focused { pal.ink } else { pal.ok });
                        } else if let Some(h) = h.filter(|h| h.progress() > 0.0) {
                            let br = Rect::from_min_size(pos2(c_prog, y - 2.0), vec2(110.0, 4.0));
                            let (track, fill) = if focused { (draw::with_alpha(pal.ink, 0.25), pal.ink) } else { (pal.raise, pal.accent) };
                            bar(&p_rows, br, h.progress(), track, fill);
                        }
                    }
                    if show_len {
                        let d = item.duration();
                        let len = if d > 0.0 { fmt_time(d) } else { "—".into() };
                        text(&p_rows, pos2(c_len, y), Align2::RIGHT_CENTER, len, 15.0, dimc);
                    }
                    text(&p_rows, pos2(c_size, y), Align2::RIGHT_CENTER, fmt_size(item.entry.size), 15.0, dimc);
                }

                let resp = ui.interact(r.intersect(rows_rect), ui.scope_id().with(("row", idx)), Sense::CLICK);
                if resp.hovered() {
                    hovered = Some(idx);
                }
                if resp.clicked() {
                    self.focus = idx;
                }
                if resp.double_clicked() {
                    self.focus = idx;
                    out.push(Action::Accept);
                }
            }
            self.hover_item = hovered;
            let empty_msg = if self.loading && n == 0 {
                Some("Reading folder…".to_string())
            } else if let Some(e) = self.list_error.as_ref().filter(|_| n == 0) {
                Some(format!("Can't open this folder: {e}"))
            } else if n == 0 && !self.filter.is_empty() {
                Some("Nothing matches the filter".into())
            } else if n == 0 {
                Some(match self.source {
                    Source::Dir(_) => "No videos or folders here".into(),
                    Source::All if self.loading => "Looking through your folders…".into(),
                    Source::All => "No media found. Add folders in Settings → Library.".into(),
                    Source::Continue => "Nothing to continue yet".into(),
                    Source::Recent => "Nothing played yet".into(),
                    Source::Movies | Source::Shows | Source::Show(_) if self.loading => "Looking through your folders…".into(),
                    Source::Movies => "No movies found. Add folders in Settings → Library.".into(),
                    Source::Shows => "No TV shows found. Episodes need names like Show S01E02 or a Season 1 folder.".into(),
                    Source::Show(_) => "No episodes".into(),
                    Source::Playlists => "No playlists yet. Select a video and press Add to playlist.".into(),
                    Source::Playlist(_) => "This playlist is empty".into(),
                Source::Added if self.loading => "Looking through your folders…".into(),
                Source::Added => "Nothing in your libraries yet. Add folders in Settings → Library.".into(),
                })
            } else {
                None
            };
            if let Some(m) = empty_msg {
                text(&p, pos2(name_left, rows_rect.top() + ROW_H / 2.0), Align2::LEFT_CENTER, m, 15.0, pal.dim);
            }

        }

        // ---- hint bar (under the list only)
        if self.cfg.show_hints {
            let hint_y = rect.bottom() - HINT_H / 2.0;
            let focused = self.items.get(self.focus);
            let on_show = focused.is_some_and(|i| i.show.is_some());
            let on_dir = focused.is_some_and(|i| i.entry.is_dir);
            let on_playlist = focused.is_some_and(|i| crate::playlist::is_playlist(&i.entry.path));
            let open = if on_show { "Episodes" } else if on_dir || on_playlist { "Open" } else { "Play" };
            // Everything else (from start, playlists, watched, fix match…) is in Options.
            let mut list: Vec<(Action, &str)> = vec![(Action::Accept, open), (Action::Back, "Back"), (Action::Options, "Options")];
            if seasons.len() > 1 {
                list.push((Action::NextTab, "Season"));
            }
            if self.grid_capable() {
                list.push((Action::ToggleView, if self.cfg.grid_view { "List" } else { "Grid" }));
            }
            list.push((Action::Filter, "Search"));
            if list.len() < 6 {
                list.push((Action::Palette, "Commands"));
            }
            list.push((Action::Menu, "Menu"));
            out.extend(self.hints(ui, pos2(rect.left() + PAD, hint_y), Context::Browser, &list, 13.0, 22.0, pal.dim, pal.raise));
        }

        // ---- info panel
        out.extend(self.draw_detail(ui, detail_rect));
        out
    }

    fn draw_detail(&mut self, ui: &mut egui::Ui, r: Rect) -> Vec<Action> {
        let pal = self.pal;
        let p = ui.painter().clone();
        let mut out = Vec::new();
        p.rect_filled(r, 0.0, pal.panel);
        let Some(item) = self.items.get(self.focus).cloned() else { return out };
        let in_library = self.in_library(&item.entry.path);
        let x = r.left() + PAD;
        let inner = r.width() - PAD * 2.0;
        let mut y = r.top() + 18.0;

        let info = self.info_for(&item).cloned();
        let ep_info = item.ep.as_ref().and_then(|e| info.as_ref()?.episodes.get(&crate::scrape::ep_key(e.season, e.episode)).cloned());
        let title = if item.show.is_some() {
            info.as_ref().map(|i| i.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| item.entry.name.clone())
        } else if item.entry.is_dir {
            format!("{}/", item.entry.name)
        } else if let Some(e) = &item.ep {
            let t = self.episode_title(&item.entry.path, e).map(|t| format!(" · {t}")).unwrap_or_default();
            format!("{}{t}", e.code())
        } else {
            // Metadata title, else the cleaned-up file name ("Alien (1979)").
            self.title_of(&item.entry.path)
        };

        // ---- poster beside title / year / rating / genres
        // Episodes: the still across the top, and the season's poster beside the title.
        if let Some(still) = ep_info.as_ref().and_then(|e| e.still.clone()) {
            if let Some(tex) = self.art(ui.ctx(), &still, crate::scrape::ImgKind::Still) {
                let [tw, th] = tex.size();
                let sh = inner * th as f32 / tw.max(1) as f32;
                let sr = Rect::from_min_size(pos2(x, y), vec2(inner, sh.min(r.height() * 0.3)));
                p.image(tex.id(), sr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
                y = sr.bottom() + 14.0;
            }
        }
        let poster_src = if item.ep.is_some() { self.tile_art(&item, false) } else { info.as_ref().and_then(|i| i.poster.clone()) };
        let poster = poster_src.and_then(|src| self.art(ui.ctx(), &src, crate::scrape::ImgKind::Poster));
        if let Some(tex) = poster {
            let [tw, th] = tex.size();
            let has_still = ep_info.as_ref().is_some_and(|e| e.still.is_some());
            let ph = if has_still { (r.height() * 0.2).min(170.0) } else { (r.height() * 0.32).min(260.0) };
            let pw = (ph * tw as f32 / th.max(1) as f32).min(inner * 0.45);
            let ph = pw * th as f32 / tw.max(1) as f32;
            let pr = Rect::from_min_size(pos2(x, y), vec2(pw, ph));
            p.image(tex.id(), pr, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), egui::Color32::WHITE);
            let tx = pr.right() + 16.0;
            let tw_ = r.right() - PAD - tx;
            let mut ty = y;
            if let Some(e) = &item.ep {
                ty += text_wrap(&p, pos2(tx, ty), tw_, &e.show, 13.0, pal.dim, 2) + 4.0;
            }
            ty += text_wrap(&p, pos2(tx, ty), tw_, &title, 19.0, pal.text, 4) + 8.0;
            if let Some(i) = &info {
                let mut bits = Vec::new();
                if let Some(yr) = i.year {
                    bits.push(yr.to_string());
                }
                if let Some(rt) = i.rating {
                    bits.push(format!("{rt:.1}/10"));
                }
                if let Some(a) = ep_info.as_ref().map(|e| e.aired.clone()).filter(|a| !a.is_empty()) {
                    bits.push(format!("aired {a}"));
                }
                if !bits.is_empty() {
                    ty += text_wrap(&p, pos2(tx, ty), tw_, &bits.join(" · "), 13.0, pal.accent, 2) + 4.0;
                }
                if !i.genres.is_empty() {
                    ty += text_wrap(&p, pos2(tx, ty), tw_, &i.genres.join(", "), 13.0, pal.dim, 3) + 4.0;
                }
            }
            let _ = ty;
            y = pr.bottom() + 14.0;
        } else {
            if let Some(e) = &item.ep {
                y += text_wrap(&p, pos2(x, y), inner, &e.show, 14.0, pal.dim, 2) + 4.0;
            }
            y += text_wrap(&p, pos2(x, y), inner, &title, 21.0, pal.text, 3) + 14.0;
        }
        // Description: the episode's own if known, else the show's or movie's.
        let overview = ep_info.as_ref().map(|e| e.overview.clone()).filter(|o| !o.is_empty()).or_else(|| info.as_ref().map(|i| i.overview.clone()).filter(|o| !o.is_empty()));
        if let Some(o) = overview {
            let lines = if item.show.is_some() { 8 } else { 5 };
            y += text_wrap(&p, pos2(x, y), inner, &o, 14.0, pal.dim, lines) + 14.0;
        }

        let key_w = if inner < 380.0 { 96.0 } else { 112.0 };
        // Value segments flow onto a second line if they don't fit (keeps "ends 21:45" visible).
        let mut kv = |k: &str, v: &[(String, egui::Color32)]| {
            text(&p, pos2(x, y + 9.0), Align2::LEFT_CENTER, k.to_uppercase(), 11.0, pal.faint);
            let left = x + key_w;
            let right = r.right() - PAD;
            let mut vx = left;
            for (s, c) in v {
                let w = measure(&p, s, 15.0).x;
                if vx > left && vx + w > right {
                    y += 20.0;
                    vx = left;
                }
                let s = if vx == left { s.trim_start() } else { s.as_str() };
                if vx == left && w > right - left {
                    // Too long for one line (file names, paths): wrap instead of cutting it off.
                    let line_h = measure(&p, "X", 15.0).y;
                    let h = text_wrap_any(&p, pos2(vx, y + 9.0 - line_h / 2.0), right - left, s, 15.0, *c);
                    y += (h - line_h).max(0.0);
                    vx = right;
                    continue;
                }
                let rr = text(&p, pos2(vx, y + 9.0), Align2::LEFT_CENTER, s, 15.0, *c);
                vx = rr.right();
            }
            y += 24.0;
        };

        if let Some(show) = item.show.as_ref().and_then(|k| self.catalog.show(k)).cloned() {
            let ns = show.seasons().len();
            kv("Seasons", &[(ns.to_string(), pal.text)]);
            let watched = show.episodes.iter().filter(|(_, f)| self.hist.get(&f.path).is_some_and(|h| h.watched)).count();
            kv("Episodes", &[(format!("{} · {watched} watched", show.episodes.len()), pal.text)]);
            let next = &show.episodes[self.next_up(&show)];
            let nt = self.episode_title(&next.1.path, &next.0).map(|t| format!(" · {t}")).unwrap_or_default();
            kv("Next", &[(format!("{}{nt}", next.0.code()), pal.accent)]);
            if let Some(src) = info.as_ref().map(|i| i.source.clone()).filter(|s| !s.is_empty()) {
                kv("Info from", &[(src, pal.faint)]);
            }
        } else if item.entry.is_dir {
            kv("Path", &[(item.entry.path.display().to_string(), pal.dim)]);
            if self.cfg.roots.contains(&item.entry.path) {
                kv("Library", &[("yes, part of My media".into(), pal.ok)]);
            }
        } else {
            let h = item.hist.clone().unwrap_or_default();
            let duration = item.duration();
            // Metadata is read in the background after a short pause on a library file.
            let reading = item.meta.is_none()
                && duration <= 0.0
                && in_library
                && !self.probe_failed.contains(&item.entry.path);
            kv("File", &[(item.entry.name.clone(), pal.dim)]);
            // The folder is noise once a poster and description are showing.
            if !matches!(self.source, Source::Dir(_)) && info.is_none() {
                if let Some(parent) = item.entry.path.parent() {
                    kv("Folder", &[(parent.display().to_string(), pal.dim)]);
                }
            }
            if duration > 0.0 {
                let from = if h.resumable() { h.pos } else { 0.0 };
                let left = duration - from;
                let ends = clock_at(now_unix() + left.round() as i64);
                let mut v = vec![(fmt_time(duration), pal.text), (" · ".into(), pal.dim)];
                if h.resumable() {
                    v.push((format!("{} left · ", fmt_dur_short(left)), pal.dim));
                }
                v.push((format!("ends {ends}"), pal.accent));
                kv("Length", &v);
            } else if reading {
                kv("Length", &[("reading…".into(), pal.faint)]);
            } else {
                kv("Length", &[("—".into(), pal.faint)]);
            }
            if h.resumable() {
                kv("Stopped at", &[(fmt_time(h.pos), pal.text)]);
            }
            if h.watched {
                kv("Status", &[("watched".into(), pal.ok)]);
            }
            if let Some(m) = &item.meta {
                if let Some(v) = &m.video {
                    kv("Video", &[(v.clone(), pal.text)]);
                }
                if m.audio_tracks > 0 {
                    let n = if m.audio_tracks == 1 { "1 track".to_string() } else { format!("{} tracks", m.audio_tracks) };
                    let langs = if m.audio_langs.is_empty() { String::new() } else { format!(" · {}", m.audio_langs.join(", ")) };
                    kv("Audio", &[(format!("{n}{langs}"), pal.text)]);
                }
            }
            kv("Size", &[(fmt_size(item.entry.size), pal.text)]);
            kv("Container", &[(item.entry.ext(), pal.text)]);
            let mut subs: Vec<String> = item.entry.subs.clone();
            if let Some(n) = item.meta.as_ref().map(|m| m.sub_tracks).filter(|n| *n > 0) {
                subs.push(format!("{n} embedded"));
            }
            let subs = if subs.is_empty() { "—".to_string() } else { subs.join(", ") };
            kv("Subtitles", &[(subs, pal.text)]);
            if let Some(m) = item.entry.modified {
                kv("Modified", &[(local_date(m), pal.text)]);
            }
        }

        // ---- action buttons at the bottom of the panel
        let resumable = item.hist.as_ref().is_some_and(|h| h.resumable());
        let buttons: Vec<(String, Action, bool)> = if let Some(show) = item.show.as_ref().and_then(|k| self.catalog.show(k)) {
            let next = &show.episodes[self.next_up(show)];
            let resume = self.hist.get(&next.1.path).is_some_and(|h| h.resumable());
            vec![
                (format!("{} {}", if resume { "▶ Resume" } else { "▶ Play" }, next.0.code()), Action::PlayNext, true),
                ("Episodes".into(), Action::Accept, false),
            ]
        } else if item.entry.is_dir {
            let in_lib = self.cfg.roots.contains(&item.entry.path);
            vec![
                ("Open folder".into(), Action::Accept, true),
                (if in_lib { "− Remove from library" } else { "+ Add to library" }.into(), Action::ToggleLibrary, false),
            ]
        } else if resumable {
            let pos = fmt_time(item.hist.as_ref().map(|h| h.pos).unwrap_or(0.0));
            vec![(format!("▶ Resume from {pos}"), Action::Accept, true), ("↺ Play from start".into(), Action::PlayFromStart, false)]
        } else {
            vec![("▶ Play".into(), Action::Accept, true)]
        };
        let bh = 40.0;
        let mut by = r.bottom() - PAD - bh * buttons.len() as f32 - 6.0 * (buttons.len() as f32 - 1.0);
        // Long details stop above the buttons instead of running underneath them.
        p.rect_filled(Rect::from_min_max(pos2(r.left(), by - 14.0), r.right_bottom()), 0.0, pal.panel);
        for (label, action, primary) in buttons {
            let br = Rect::from_min_size(pos2(x, by), vec2(inner, bh));
            let resp = ui.interact(br, ui.scope_id().with(("btn", label.as_str())), Sense::CLICK);
            let (bg, fg) = if primary { (pal.accent, pal.ink) } else { (pal.raise, pal.text) };
            let bg = if resp.hovered() && !primary { theme_mix(pal.raise, pal.accent, 0.15) } else { bg };
            p.rect_filled(br, 0.0, bg);
            text(&p, pos2(br.left() + 12.0, br.center().y), Align2::LEFT_CENTER, &label, 15.0, fg);
            if let Some((cap, color)) = self.glyph(Context::Browser, action) {
                let cw = draw::measure(&p, &cap, 12.0).x.max(12.0 * 1.7 - 8.0) + 8.0;
                let cap_bg = if primary { draw::with_alpha(pal.ink, 0.18) } else { pal.bg };
                let cap_fg = if primary { pal.ink } else { color.unwrap_or(pal.text) };
                draw::chip(&p, pos2(br.right() - 10.0 - cw, br.center().y), &cap, 12.0, cap_fg, cap_bg);
            }
            if resp.clicked() {
                out.push(action);
            }
            by += bh + 6.0;
        }
        out
    }
}

fn theme_mix(a: egui::Color32, b: egui::Color32, t: f32) -> egui::Color32 {
    crate::theme::mix(a, b, t)
}
