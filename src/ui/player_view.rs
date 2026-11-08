//! The playback screen: video, plus quiet controls that fade out.

use super::draw::*;
use super::{App, Screen};
use crate::action::{Action, Context};
use crate::player::TrackKind;
use egui::{Align2, Color32, Rect, Sense, pos2, vec2};
use std::time::Duration;

impl App {
    pub(super) fn draw_player(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let mut out = Vec::new();
        let p = ui.painter().clone();
        p.rect_filled(rect, 0.0, Color32::BLACK);
        if let Some(player) = &self.player {
            player.paint(&p, rect);
        }
        // Benchmark switch: video only, no UI work at all (measures the UI's own cost).
        if std::env::var_os("JIMMY_BENCH_NOUI").is_some() {
            return out;
        }

        // Mouse: moving shows the controls; click pauses; double-click toggles fullscreen.
        let resp = ui.interact(rect, ui.scope_id().with("video"), Sense::CLICK);
        if ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) && resp.hovered() {
            self.show_osd();
        }
        if resp.double_clicked() {
            out.push(Action::Fullscreen);
        } else if resp.clicked() {
            out.push(Action::PlayPause);
        }
        if resp.hovered() {
            let dy: f32 = ui.input(|i| {
                i.raw.events.iter().map(|e| if let egui::Event::MouseWheel { delta, .. } = e { delta.y } else { 0.0 }).sum()
            });
            if dy > 0.0 {
                out.push(Action::VolumeUp);
            } else if dy < 0.0 {
                out.push(Action::VolumeDown);
            }
        }

        let Some(s) = self.player.as_ref().map(|p| p.s.clone()) else { return out };
        if !s.loaded {
            text(&p, rect.center(), Align2::CENTER_CENTER, "loading…", 14.0, white(0.4));
        }

        let visible = self.osd_visible_now();
        if let Some(pl) = self.player.as_mut() {
            pl.set_sub_lift(visible && self.overlay == super::Overlay::None);
        }
        if !visible && self.overlay == super::Overlay::None {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }

        // ---- volume / brightness HUD: one slim line
        if let Some((is_vol, value)) = self.hud_state() {
            let w = 260.0;
            let r = Rect::from_center_size(pos2(rect.center().x, rect.top() + 35.0), vec2(w, 30.0));
            p.rect_filled(r, 0.0, black(0.6));
            let y = r.center().y;
            text(&p, pos2(r.left() + 10.0, y), Align2::LEFT_CENTER, if is_vol { "VOL" } else { "BRT" }, 11.0, white(0.4));
            let muted = is_vol && self.is_muted();
            let boost = is_vol && value > 100.0;
            let val = if muted { "mute".to_string() } else { format!("{value:.0}%") };
            let vc = if boost { self.pal.boost } else { white(0.7) };
            text(&p, pos2(r.right() - 10.0, y), Align2::RIGHT_CENTER, val, 13.0, vc);
            let br = Rect::from_min_max(pos2(r.left() + 44.0, y - 1.5), pos2(r.right() - 58.0, y + 1.5));
            p.rect_filled(br, 0.0, white(0.2));
            let accent = with_alpha(self.pal.accent, 0.85);
            if is_vol {
                let max = 200.0;
                let fill = (value.min(100.0) / max) * br.width();
                p.rect_filled(Rect::from_min_size(br.min, vec2(fill, br.height())), 0.0, if muted { white(0.35) } else { accent });
                if boost {
                    let b0 = br.left() + br.width() * 0.5;
                    let b1 = br.left() + br.width() * (value / max).min(1.0);
                    p.rect_filled(Rect::from_min_max(pos2(b0, br.top()), pos2(b1, br.bottom())), 0.0, self.pal.boost);
                }
                let mx = br.left() + br.width() * 0.5;
                p.rect_filled(Rect::from_min_max(pos2(mx, br.top() - 2.0), pos2(mx + 1.0, br.bottom() + 2.0)), 0.0, white(0.6));
            } else {
                p.rect_filled(Rect::from_min_size(br.min, vec2(br.width() * value / 100.0, br.height())), 0.0, white(0.8));
            }
        }

        // ---- hold-the-Subs-button picker
        if let Some(h) = self.sub_hold.filter(|h| h.open) {
            let opts = self.sub_options();
            let row_h = 34.0;
            let w = 440.0;
            let h_px = 40.0 + row_h * opts.len() as f32 + 34.0;
            let r = Rect::from_center_size(rect.center(), vec2(w, h_px));
            p.rect_filled(r, 0.0, black(0.82));
            text(&p, pos2(r.left() + 16.0, r.top() + 20.0), Align2::LEFT_CENTER, "SUBTITLES", 11.0, white(0.45));
            for (i, (_, label, meta)) in opts.iter().enumerate() {
                let rr = Rect::from_min_size(pos2(r.left(), r.top() + 40.0 + i as f32 * row_h), vec2(w, row_h));
                let on = i == h.idx;
                if on {
                    p.rect_filled(rr, 0.0, self.pal.accent);
                }
                let (fg, mc) = if on { (self.pal.ink, self.pal.ink) } else { (white(0.8), white(0.45)) };
                text_clip(&p, pos2(rr.left() + 16.0, rr.center().y), w * 0.55, label, 15.0, fg);
                text_clip_right(&p, pos2(rr.right() - 16.0, rr.center().y), w * 0.38, meta, 12.0, mc);
            }
            let cap = h.input.label(self.pad_kind);
            let hint = format!("▲▼ move · release {cap} to select · {} cancel", crate::input::PadInput::B.label(self.pad_kind));
            text(&p, pos2(r.left() + 16.0, r.bottom() - 17.0), Align2::LEFT_CENTER, hint, 12.0, white(0.45));
        }

        if !visible {
            return out;
        }
        out.extend(self.draw_end_cards(ui, rect));

        // ---- top: title and clock
        let top = Rect::from_min_size(rect.min, vec2(rect.width(), 90.0));
        vgradient(&p, top, black(0.6), black(0.0));
        let title = s.path.as_ref().map(|p| self.title_of(p)).unwrap_or_default();
        text_clip(&p, pos2(rect.left() + 24.0, rect.top() + 26.0), rect.width() * 0.6, &title, 17.0, white(0.72));
        let mut meta = Vec::new();
        if let Some(ext) = s.path.as_ref().and_then(|p| p.extension()) {
            meta.push(ext.to_string_lossy().to_uppercase());
        }
        if !s.video_codec.is_empty() {
            let codec = s.video_codec.split_whitespace().next().unwrap_or("").to_uppercase();
            meta.push(if s.height > 0 { format!("{codec} {}p", s.height) } else { codec });
        }
        meta.push(if s.hwdec.is_empty() || s.hwdec == "no" { "sw decode".into() } else { format!("hw {}", s.hwdec) });
        if self.cfg.rescue_mode {
            meta.push("rescue mode".into());
        }
        if self.queue.len() > 1 {
            let name = if self.queue_name.is_empty() { String::new() } else { format!("{} ", self.queue_name) };
            meta.push(format!("{name}{}/{}", self.queue_idx + 1, self.queue.len()));
        }
        if self.cfg.repeat != crate::config::Repeat::Off {
            meta.push(format!("repeat {}", self.cfg.repeat.label()));
        }
        text(&p, pos2(rect.left() + 24.0, rect.top() + 48.0), Align2::LEFT_CENTER, meta.join(" · "), 13.0, white(0.4));
        let now = now_unix();
        text(&p, pos2(rect.right() - 24.0, rect.top() + 26.0), Align2::RIGHT_CENTER, clock_at(now), 16.0, white(0.55));
        if s.duration > 0.0 {
            let ends = clock_at(now + s.remaining().round() as i64);
            text(&p, pos2(rect.right() - 24.0, rect.top() + 48.0), Align2::RIGHT_CENTER, format!("ends {ends}"), 13.0, white(0.4));
        }

        // ---- bottom: seek bar and one quiet info line
        let bottom = Rect::from_min_max(pos2(rect.left(), rect.bottom() - 110.0), rect.right_bottom());
        vgradient(&p, bottom, black(0.0), black(0.72));
        let sy = rect.bottom() - 46.0;
        let cur = fmt_time(s.time_pos);
        let rem = format!("−{}", fmt_time((s.duration - s.time_pos).max(0.0)));
        // The icon shows what A / Space will do: ▶ while paused, || while playing.
        let icon = if s.paused { "▶" } else { "||" };
        // Fixed-width slot (the wider of the two icons) so the track doesn't shift on play/pause.
        let slot = measure(&p, "||", 13.0).x.max(measure(&p, "▶", 13.0).x);
        // Right end, after the remaining time: [time] ━━━━━ [−left] [▶]
        let ix = rect.right() - 24.0 - slot;
        let icon_r = text(&p, pos2(ix + slot / 2.0, sy), Align2::CENTER_CENTER, icon, 13.0, with_alpha(self.pal.accent, 0.8));
        if ui.interact(icon_r.expand(8.0), ui.scope_id().with("playpause"), Sense::CLICK).clicked() {
            out.push(Action::PlayPause);
        }
        let l = text(&p, pos2(rect.left() + 24.0, sy), Align2::LEFT_CENTER, &cur, 15.0, white(0.72));
        let r = text(&p, pos2(ix - 12.0, sy), Align2::RIGHT_CENTER, &rem, 15.0, white(0.45));
        let track = Rect::from_min_max(pos2(l.right() + 14.0, sy - 2.0), pos2(r.left() - 14.0, sy + 2.0));
        p.rect_filled(track, 0.0, white(0.2));
        if s.duration > 0.0 {
            let frac = (s.time_pos / s.duration).clamp(0.0, 1.0) as f32;
            let cache = ((s.time_pos + s.cache_secs) / s.duration).clamp(0.0, 1.0) as f32;
            p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * cache, 4.0)), 0.0, white(0.4));
            p.rect_filled(Rect::from_min_size(track.min, vec2(track.width() * frac, 4.0)), 0.0, with_alpha(self.pal.accent, 0.85));
            let tx = track.left() + track.width() * frac;
            p.rect_filled(Rect::from_min_max(pos2(tx - 1.5, sy - 7.0), pos2(tx + 1.5, sy + 7.0)), 0.0, white(0.75));

            // Click / drag on the bar to seek.
            let hit = track.expand2(vec2(0.0, 10.0));
            let sresp = ui.interact(hit, ui.scope_id().with("seek"), Sense::CLICK | Sense::DRAG);
            if let Some(pos) = sresp.interact_pointer_pos().filter(|_| sresp.clicked() || sresp.dragged()) {
                let f = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
                if let Some(pl) = &self.player {
                    pl.seek_absolute(f * s.duration);
                }
                self.show_osd();
            }
            if let Some(hp) = sresp.hover_pos() {
                let f = ((hp.x - track.left()) / track.width()).clamp(0.0, 1.0) as f64;
                let label = fmt_time(f * s.duration);
                let lr = Rect::from_center_size(pos2(hp.x, sy - 20.0), vec2(measure(&p, &label, 12.0).x + 12.0, 20.0));
                p.rect_filled(lr, 0.0, black(0.7));
                text(&p, lr.center(), Align2::CENTER_CENTER, label, 12.0, white(0.8));
            }
        }

        let iy = rect.bottom() - 16.0;
        let mut x = rect.left() + 24.0;
        let mut item = |k: &str, v: String| {
            let kr = text(&p, pos2(x, iy), Align2::LEFT_CENTER, k, 13.0, white(0.4));
            let vr = text(&p, pos2(kr.right() + 7.0, iy), Align2::LEFT_CENTER, v, 13.0, white(0.65));
            x = vr.right() + 18.0;
        };
        if s.chapters > 0 {
            item("ch", format!("{}/{}", s.chapter + 1, s.chapters));
        }
        item("vol", if self.is_muted() { "mute".into() } else { format!("{:.0}%", self.vol) });
        item("sub", s.selected(TrackKind::Sub).map(|t| t.label()).unwrap_or_else(|| "off".into()));
        if let Some(a) = s.selected(TrackKind::Audio) {
            let ch = match a.channels {
                Some(6) => " 5.1",
                Some(8) => " 7.1",
                _ => "",
            };
            item("aud", format!("{}{ch}", a.label()));
        }
        if (s.speed - 1.0).abs() > 0.01 {
            item("speed", format!("{:.2}×", s.speed));
        }
        if s.sub_delay.abs() > 0.001 {
            item("sub delay", format!("{:+.1}s", s.sub_delay));
        }

        if self.cfg.show_hints && self.overlay == super::Overlay::None {
            let list: &[(Action, &str)] = &[
                (Action::PlayPause, if s.paused { "Play" } else { "Pause" }),
                (Action::SeekForward, "Seek"),
                (Action::VolumeUp, "Vol"),
                (Action::NextSubtitle, "Subs"),
                (Action::Tracks, "Tracks"),
                (Action::Back, "Back"),
            ];
            // Right-aligned with the same 24 px margin as the times above.
            let w = self.hints_width(&p, Context::Player, list, 11.0, 14.0);
            let hx = (rect.right() - 24.0 - w).max(x + 20.0);
            out.extend(self.hints(ui, pos2(hx, iy), Context::Player, list, 11.0, 14.0, white(0.4), white(0.1)));
        }

        // Keep the clock and progress ticking while the controls are up.
        if self.screen == Screen::Player {
            ui.ctx().request_repaint_after(Duration::from_millis(if s.paused { 1000 } else { 250 }));
        }
        out
    }
}

impl App {
    /// "Up next" countdown card, or the end card when the player stays open after a video.
    fn draw_end_cards(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let mut out = Vec::new();
        let p = ui.painter().clone();
        let accent = self.pal.accent;
        let ink = self.pal.ink;
        if let Some(at) = self.up_next {
            let Some(next) = self.queue_next().and_then(|i| self.queue.get(i).cloned()) else { return out };
            let secs = at.saturating_duration_since(std::time::Instant::now()).as_secs_f32().ceil() as u32;
            let w = 420.0;
            let r = Rect::from_min_size(pos2(rect.right() - 24.0 - w, rect.bottom() - 110.0 - 120.0), vec2(w, 108.0));
            p.rect_filled(r, 0.0, black(0.82));
            text(&p, pos2(r.left() + 16.0, r.top() + 20.0), Align2::LEFT_CENTER, format!("UP NEXT IN {secs}"), 11.0, white(0.5));
            text_clip(&p, pos2(r.left() + 16.0, r.top() + 46.0), w - 32.0, &self.title_of(&next), 16.0, white(0.85));
            // Countdown bar.
            let total = self.cfg.autoplay_countdown.max(1) as f32;
            let left = at.saturating_duration_since(std::time::Instant::now()).as_secs_f32();
            let bar = Rect::from_min_size(pos2(r.left(), r.top()), vec2(w * (1.0 - left / total).clamp(0.0, 1.0), 2.0));
            p.rect_filled(bar, 0.0, accent);
            let list: &[(Action, &str)] = &[(Action::PlayPause, "Play now"), (Action::Back, "Cancel")];
            out.extend(self.hints(ui, pos2(r.left() + 16.0, r.bottom() - 22.0), Context::Player, list, 12.0, 18.0, white(0.6), white(0.12)));
            let resp = ui.interact(r, ui.scope_id().with("upnext"), Sense::CLICK);
            if resp.clicked() {
                out.push(Action::NextItem);
            }
        } else if self.at_end {
            let w = 360.0;
            let r = Rect::from_center_size(rect.center(), vec2(w, 120.0));
            p.rect_filled(r, 0.0, black(0.82));
            text(&p, pos2(r.center().x, r.top() + 30.0), Align2::CENTER_CENTER, "Finished", 18.0, white(0.85));
            let bw = (w - 48.0) / 2.0;
            for (i, (label, a)) in [("↺ Replay", Action::Replay), ("Back", Action::Stop)].into_iter().enumerate() {
                let br = Rect::from_min_size(pos2(r.left() + 16.0 + i as f32 * (bw + 16.0), r.bottom() - 56.0), vec2(bw, 40.0));
                let primary = i == 0;
                p.rect_filled(br, 0.0, if primary { accent } else { white(0.12) });
                text(&p, pos2(br.left() + 12.0, br.center().y), Align2::LEFT_CENTER, label, 15.0, if primary { ink } else { white(0.8) });
                let hint = if primary { Action::PlayPause } else { Action::Back };
                if let Some((cap, _)) = self.glyph(Context::Player, hint) {
                    chip(&p, pos2(br.right() - 10.0 - measure(&p, &cap, 12.0).x.max(12.0 * 1.7 - 8.0) - 8.0, br.center().y), &cap, 12.0, if primary { ink } else { white(0.8) }, if primary { with_alpha(ink, 0.18) } else { black(0.4) });
                }
                if ui.interact(br, ui.scope_id().with(("endbtn", i)), Sense::CLICK).clicked() {
                    out.push(a);
                }
            }
        }
        if self.up_next.is_some() {
            ui.ctx().request_repaint_after(Duration::from_millis(200));
        }
        out
    }
}
