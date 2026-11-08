//! Poster grid: the same rows as the list, drawn as tiles (V / LS toggles).

use super::draw::*;
use super::{App, Source};
use crate::action::Action;
use egui::{Align2, Rect, Sense, pos2, vec2};
use std::time::Duration;

const GAP: f32 = 18.0;
const PAD: f32 = 20.0;
/// Poster tiles are 2:3 (portrait).
const ASPECT: f32 = 1.5;
/// Lines of title under each poster.
const CAPTION_H: f32 = 44.0;
/// New poster textures loaded per frame (the rest wait a frame, so scrolling stays smooth).
const LOADS_PER_FRAME: usize = 3;

impl App {
    /// Lists where every row has (or can have) its own poster.
    pub(crate) fn grid_capable(&self) -> bool {
        matches!(self.source, Source::All | Source::Movies | Source::Shows | Source::Show(_) | Source::Added | Source::Continue | Source::Recent | Source::Playlist(_))
    }

    pub(crate) fn grid_active(&self) -> bool {
        self.cfg.grid_view && self.grid_capable() && !self.items.is_empty()
    }

    pub(crate) fn toggle_grid(&mut self) {
        self.cfg.grid_view = !self.cfg.grid_view;
        self.cfg.save();
        self.scroll = 0.0;
        self.scroll_focus = usize::MAX;
        if !self.grid_capable() {
            self.toast(if self.cfg.grid_view { "Grid view on (for Movies, TV shows and other flat lists)" } else { "List view" });
        }
    }

    /// Column count and tile width for a grid in `width`: posters ~165 px, stills ~260 px.
    fn grid_layout(width: f32, wide: bool) -> (usize, f32) {
        let inner = width - PAD * 2.0;
        let target = if wide { 260.0 } else { 165.0 };
        let cols = ((inner + GAP) / (target + GAP)).floor().max(1.0) as usize;
        let tile_w = (inner - GAP * (cols - 1) as f32) / cols as f32;
        (cols, tile_w)
    }

    pub(super) fn draw_grid(&mut self, ui: &mut egui::Ui, area: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let p = ui.painter().clone();
        let area = Rect::from_min_max(pos2(area.left(), area.top() + 8.0), area.max);
        // A show's page: 16:9 episode stills instead of posters.
        let wide = matches!(self.source, Source::Show(_));
        let aspect = if wide { 9.0 / 16.0 } else { ASPECT };
        let (cols, tile_w) = Self::grid_layout(area.width(), wide);
        self.grid_cols = cols;
        let tile_h = tile_w * aspect + CAPTION_H;
        let row_h = tile_h + GAP;
        let n = self.items.len();
        let rows = n.div_ceil(cols);
        let visible_rows = ((area.height() / row_h).floor() as usize).max(1);

        // Wheel scrolls by rows; moving the selection keeps it in view.
        let resp = ui.interact(area, ui.scope_id().with("grid"), Sense::hover());
        if resp.hovered() {
            let dy = ui.input(|i| i.smooth_scroll_delta.y);
            if dy != 0.0 {
                self.scroll = (self.scroll - dy / row_h).clamp(0.0, rows.saturating_sub(visible_rows) as f32);
            }
        }
        if self.focus != self.scroll_focus {
            self.scroll_focus = self.focus;
            let fr = (self.focus / cols) as f32;
            if fr < self.scroll {
                self.scroll = fr;
            } else if fr >= self.scroll + visible_rows as f32 {
                self.scroll = fr + 1.0 - visible_rows as f32;
            }
        }
        self.scroll = self.scroll.clamp(0.0, rows.saturating_sub(visible_rows) as f32);
        let first_row = self.scroll.floor() as usize;
        let y0 = area.top() - (self.scroll - first_row as f32) * row_h;

        let clip = p.with_clip_rect(area);
        let mut loads = 0;
        let mut pending = false;
        let mut hovered = None;
        for row in first_row..(first_row + visible_rows + 1).min(rows) {
            for col in 0..cols {
                let idx = row * cols + col;
                let Some(item) = self.items.get(idx).cloned() else { break };
                let x = area.left() + PAD + col as f32 * (tile_w + GAP);
                let y = y0 + (row - first_row) as f32 * row_h;
                let poster_r = Rect::from_min_size(pos2(x, y), vec2(tile_w, tile_w * aspect));
                let focused = idx == self.focus;

                // Art loads on demand: a few new textures per frame, downloads in the background.
                let kind = if wide { crate::scrape::ImgKind::Still } else { crate::scrape::ImgKind::Poster };
                let tex = match self.tile_art(&item, wide) {
                    Some(src) if self.art_ready(&src) || loads < LOADS_PER_FRAME => {
                        if !self.art_ready(&src) {
                            loads += 1;
                        }
                        self.art(ui.ctx(), &src, kind)
                    }
                    Some(_) => {
                        pending = true;
                        None
                    }
                    None => None,
                };
                match tex {
                    Some(tex) => {
                        // Fill the tile, cropping the poster's overflow instead of letterboxing.
                        let [tw, th] = tex.size();
                        let (tw, th) = (tw as f32, th as f32);
                        let want = poster_r.width() / poster_r.height();
                        let have = tw / th.max(1.0);
                        let uv = if have > want {
                            let u = (1.0 - want / have) / 2.0;
                            Rect::from_min_max(pos2(u, 0.0), pos2(1.0 - u, 1.0))
                        } else {
                            let v = (1.0 - have / want) / 2.0;
                            Rect::from_min_max(pos2(0.0, v), pos2(1.0, 1.0 - v))
                        };
                        clip.image(tex.id(), poster_r, uv, egui::Color32::WHITE);
                    }
                    None => {
                        // Placeholder: the title on a plain card.
                        clip.rect_filled(poster_r, 0.0, pal.panel);
                        let icon = if item.show.is_some() { "▥" } else { "▣" };
                        text(&clip, pos2(poster_r.center().x, poster_r.top() + poster_r.height() * 0.3), Align2::CENTER_CENTER, icon, 28.0, pal.faint);
                        let name = self.grid_caption(&item).0;
                        text_wrap(&clip, pos2(poster_r.left() + 12.0, poster_r.center().y), poster_r.width() - 24.0, &name, 14.0, pal.dim, 4);
                    }
                }

                // Progress along the poster's bottom edge; watched gets a corner tag.
                let h = item.hist.as_ref();
                if item.show.is_none() {
                    if h.is_some_and(|h| h.watched) {
                        let w = measure(&clip, "WATCHED", 10.0).x + 12.0;
                        let tag = Rect::from_min_size(pos2(poster_r.right() - 6.0 - w, poster_r.top() + 6.0), vec2(w, 18.0));
                        clip.rect_filled(tag, 0.0, black(0.7));
                        text(&clip, tag.center(), Align2::CENTER_CENTER, "WATCHED", 10.0, pal.ok);
                    } else if let Some(prog) = h.map(|h| h.progress()).filter(|p| *p > 0.0) {
                        let br = Rect::from_min_max(pos2(poster_r.left(), poster_r.bottom() - 4.0), poster_r.right_bottom());
                        clip.rect_filled(br, 0.0, black(0.6));
                        clip.rect_filled(Rect::from_min_size(br.min, vec2(br.width() * prog, br.height())), 0.0, pal.accent);
                    }
                }
                if focused {
                    let ring = poster_r.expand(3.0);
                    clip.rect_stroke(ring, 0.0, egui::Stroke::new(3.0, pal.accent), egui::StrokeKind::Outside);
                }

                // Caption: title, then a quiet second line (year / episode / show size).
                let cy = poster_r.bottom() + 8.0;
                let (title, sub) = self.grid_caption(&item);
                let fg = if focused { pal.accent } else { pal.text };
                text_clip(&clip, pos2(x, cy + 8.0), tile_w, &title, 14.0, fg);
                if !sub.is_empty() {
                    text_clip(&clip, pos2(x, cy + 27.0), tile_w, &sub, 12.0, pal.faint);
                }

                let tile = Rect::from_min_max(poster_r.min, pos2(poster_r.right(), poster_r.bottom() + CAPTION_H)).intersect(area);
                let r = ui.interact(tile, ui.scope_id().with(("tile", idx)), Sense::CLICK);
                if r.hovered() {
                    hovered = Some(idx);
                }
                if r.clicked() {
                    self.focus = idx;
                }
                if r.double_clicked() {
                    self.focus = idx;
                    out.push(Action::Accept);
                }
            }
        }
        self.hover_item = hovered;
        if pending {
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
        out
    }

    /// The image a tile shows: an episode's still on a show's page, else the poster
    /// (an episode elsewhere shows its season's poster, or the show's).
    pub(crate) fn tile_art(&self, item: &super::Item, wide: bool) -> Option<String> {
        let info = self.info_for(item)?;
        if let Some(e) = &item.ep {
            let ep = info.episodes.get(&crate::scrape::ep_key(e.season, e.episode));
            if wide {
                return ep.and_then(|x| x.still.clone()).or_else(|| info.backdrop.clone());
            }
            return info.seasons.get(&e.season).and_then(|s| s.poster.clone()).or_else(|| info.poster.clone());
        }
        info.poster.clone()
    }

    /// Title and second line under a tile.
    fn grid_caption(&self, item: &super::Item) -> (String, String) {
        if let (Source::Show(_), Some(e)) = (&self.source, &item.ep) {
            let t = self.episode_title(&item.entry.path, e).unwrap_or_else(|| format!("Episode {}", e.episode));
            let aired = self.info_for(item).and_then(|i| i.episodes.get(&crate::scrape::ep_key(e.season, e.episode))).map(|x| x.aired.clone()).unwrap_or_default();
            return (format!("{}  {t}", e.code()), aired);
        }
        if let Some(show) = item.show.as_ref().and_then(|k| self.catalog.show(k)) {
            let title = self.info_for(item).map(|i| i.title.clone()).filter(|t| !t.is_empty()).unwrap_or_else(|| show.name.clone());
            let ns = show.seasons().len();
            let next = show.episodes[self.next_up(show)].0.code();
            return (title, format!("{ns} season{} · next {next}", if ns == 1 { "" } else { "s" }));
        }
        if let Some(e) = &item.ep {
            let t = self.episode_title(&item.entry.path, e).unwrap_or_else(|| format!("Episode {}", e.episode));
            return (e.show.clone(), format!("{} · {t}", e.code()));
        }
        if let Some(i) = self.info_for(item).filter(|i| !i.title.is_empty()) {
            return (i.title.clone(), i.year.map(|y| y.to_string()).unwrap_or_default());
        }
        let (t, y) = crate::media::movie_title(&item.entry.name);
        (t, y.map(|y| y.to_string()).unwrap_or_default())
    }

    /// Grid navigation: up/down move a row, left/right a tile. Returns false when the move
    /// should fall through to the list behaviour (left at a row's start goes back).
    pub(crate) fn grid_move(&mut self, a: Action) -> bool {
        let cols = self.grid_cols.max(1);
        let n = self.items.len();
        let last = n.saturating_sub(1);
        match a {
            Action::Up => self.focus = self.focus.saturating_sub(cols),
            Action::Down => {
                if self.focus + cols <= last {
                    self.focus += cols;
                } else if self.focus / cols < last / cols {
                    // Partial last row: land on its last tile.
                    self.focus = last;
                }
            }
            Action::PageUp => self.focus = self.focus.saturating_sub(cols * 3),
            Action::PageDown => self.focus = (self.focus + cols * 3).min(last),
            Action::Left => {
                if self.focus % cols == 0 {
                    return false;
                }
                self.focus -= 1;
            }
            Action::Right => {
                if self.focus % cols == cols - 1 || self.focus >= last {
                    return true;
                }
                self.focus += 1;
            }
            _ => return false,
        }
        true
    }
}
