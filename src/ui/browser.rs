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
            Source::Continue => (String::new(), "Continue watching".into()),
            Source::Recent => (String::new(), "Recently played".into()),
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
        p.rect_filled(fr, 0.0, pal.panel);
        text(&p, pos2(fr.left() + 10.0, hy), Align2::LEFT_CENTER, "/", 14.0, pal.accent);
        if self.filter.is_empty() {
            text(&p, pos2(fr.left() + 26.0, hy), Align2::LEFT_CENTER, "type to filter", 14.0, pal.faint);
        } else {
            let r = text_clip(&p, pos2(fr.left() + 26.0, hy), filter_w - 40.0, &self.filter, 14.0, pal.text);
            p.rect_filled(Rect::from_min_size(pos2(r.right() + 1.0, hy - 8.0), vec2(2.0, 16.0)), 0.0, pal.accent);
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

        // ---- list
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
        label("NAME", pos2(name_left, cy), Align2::LEFT_CENTER);
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
        for (vis_i, idx) in (first..n.min(first + visible + 1)).enumerate() {
            let item = &self.items[idx];
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
            let name = if item.entry.is_dir { format!("{}/", item.entry.name) } else { item.entry.name.clone() };
            text_clip(&p_rows, pos2(name_left, y), name_right - name_left, &name, 17.0, fg);

            if item.entry.is_dir {
                // folders: nothing else (no extra disk reads)
            } else {
                if show_prog {
                    if watched {
                        text(&p_rows, pos2(c_prog, y), Align2::LEFT_CENTER, "✓ WATCHED", 11.0, if focused { pal.ink } else { pal.ok });
                    } else if let Some(h) = h.filter(|h| h.progress() > 0.0) {
                        let br = Rect::from_min_size(pos2(c_prog, y - 2.0), vec2(110.0, 4.0));
                        let (track, fill) = if focused { (draw::with_alpha(pal.ink, 0.25), pal.ink) } else { (pal.raise, pal.accent) };
                        bar(&p_rows, br, h.progress(), track, fill);
                    }
                }
                if show_len {
                    let len = h.filter(|h| h.duration > 0.0).map(|h| fmt_time(h.duration)).unwrap_or_else(|| "—".into());
                    text(&p_rows, pos2(c_len, y), Align2::RIGHT_CENTER, len, 15.0, dimc);
                }
                text(&p_rows, pos2(c_size, y), Align2::RIGHT_CENTER, fmt_size(item.entry.size), 15.0, dimc);
            }

            let resp = ui.interact(r.intersect(rows_rect), ui.scope_id().with(("row", idx)), Sense::click());
            if resp.clicked() {
                self.focus = idx;
            }
            if resp.double_clicked() {
                self.focus = idx;
                out.push(Action::Accept);
            }
        }
        let empty_msg = if self.loading && n == 0 {
            Some("Reading folder…".to_string())
        } else if let Some(e) = &self.list_error {
            Some(format!("Can't open this folder: {e}"))
        } else if n == 0 && !self.filter.is_empty() {
            Some("Nothing matches the filter".into())
        } else if n == 0 {
            Some(match self.source {
                Source::Dir(_) => "No videos or folders here".into(),
                Source::Continue => "Nothing to continue yet".into(),
                Source::Recent => "Nothing played yet".into(),
            })
        } else {
            None
        };
        if let Some(m) = empty_msg {
            text(&p, pos2(name_left, rows_rect.top() + ROW_H / 2.0), Align2::LEFT_CENTER, m, 15.0, pal.dim);
        }

        // ---- hint bar (under the list only)
        if self.cfg.show_hints {
            let hint_y = rect.bottom() - HINT_H / 2.0;
            let list: &[(Action, &str)] = &[
                (Action::Accept, "Play"),
                (Action::Back, "Back"),
                (Action::Options, "From start"),
                (Action::Palette, "Commands"),
                (Action::Menu, "Menu"),
                (Action::Fullscreen, "Fullscreen"),
            ];
            out.extend(self.hints(ui, pos2(rect.left() + PAD, hint_y), Context::Browser, list, 13.0, 22.0, pal.dim, pal.raise));
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
        let x = r.left() + PAD;
        let inner = r.width() - PAD * 2.0;
        let mut y = r.top() + 18.0;

        let title = if item.entry.is_dir {
            format!("{}/", item.entry.name)
        } else {
            std::path::Path::new(&item.entry.name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        };
        y += text_wrap(&p, pos2(x, y), inner, &title, 21.0, pal.text, 3) + 14.0;

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
                let rr = text_clip(&p, pos2(vx, y + 9.0), (right - vx).max(10.0), s.trim_start(), 15.0, *c);
                vx = rr.right();
            }
            y += 24.0;
        };

        if item.entry.is_dir {
            kv("Path", &[(item.entry.path.display().to_string(), pal.dim)]);
        } else {
            let h = item.hist.clone().unwrap_or_default();
            kv("File", &[(item.entry.name.clone(), pal.dim)]);
            if h.duration > 0.0 {
                let from = if h.resumable() { h.pos } else { 0.0 };
                let left = h.duration - from;
                let ends = clock_at(now_unix() + left.round() as i64);
                let mut v = vec![(fmt_time(h.duration), pal.text), (" · ".into(), pal.dim)];
                if h.resumable() {
                    v.push((format!("{} left · ", fmt_dur_short(left)), pal.dim));
                }
                v.push((format!("ends {ends}"), pal.accent));
                kv("Length", &v);
            } else {
                kv("Length", &[("after first play".into(), pal.faint)]);
            }
            if h.resumable() {
                kv("Stopped at", &[(fmt_time(h.pos), pal.text)]);
            }
            if h.watched {
                kv("Status", &[("watched".into(), pal.ok)]);
            }
            kv("Size", &[(fmt_size(item.entry.size), pal.text)]);
            kv("Container", &[(item.entry.ext(), pal.text)]);
            let subs = if item.entry.subs.is_empty() { "—".to_string() } else { item.entry.subs.join(", ") };
            kv("Subtitles", &[(subs, pal.text)]);
            if let Some(m) = item.entry.modified {
                kv("Modified", &[(local_date(m), pal.text)]);
            }
            if h.duration <= 0.0 {
                y += 8.0;
                let note = "// Not played yet. jimmy doesn't open files just to list them, so slow disks stay fast. Length and end time fill in after the first play.";
                let nr = Rect::from_min_size(pos2(x, y), vec2(inner, 10.0));
                let galley_h = draw::text_wrap(&p, pos2(x + 12.0, y + 10.0), inner - 24.0, note, 13.0, pal.faint, 6);
                p.rect_filled(Rect::from_min_size(nr.min, vec2(inner, galley_h + 20.0)), 0.0, pal.bg);
                draw::text_wrap(&p, pos2(x + 12.0, y + 10.0), inner - 24.0, note, 13.0, pal.faint, 6);
            }
        }

        // ---- action buttons at the bottom of the panel
        let resumable = item.hist.as_ref().is_some_and(|h| h.resumable());
        let buttons: Vec<(String, Action, bool)> = if item.entry.is_dir {
            vec![("Open folder".into(), Action::Accept, true)]
        } else if resumable {
            let pos = fmt_time(item.hist.as_ref().map(|h| h.pos).unwrap_or(0.0));
            vec![(format!("▶ Resume from {pos}"), Action::Accept, true), ("↺ Play from start".into(), Action::Options, false)]
        } else {
            vec![("▶ Play".into(), Action::Accept, true)]
        };
        let bh = 40.0;
        let mut by = r.bottom() - PAD - bh * buttons.len() as f32 - 6.0 * (buttons.len() as f32 - 1.0);
        for (label, action, primary) in buttons {
            let br = Rect::from_min_size(pos2(x, by), vec2(inner, bh));
            let resp = ui.interact(br, ui.scope_id().with(("btn", label.as_str())), Sense::click());
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
