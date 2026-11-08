//! "Add to playlist" picker: a new playlist, or one of jimmy's existing ones.

use super::draw::*;
use super::{App, Overlay};
use crate::action::{Action, Context};
use egui::{Align2, Rect, Sense, pos2, vec2};
use std::path::PathBuf;

impl App {
    /// None = "+ New playlist", then jimmy's playlists newest first.
    fn pl_choices(&self) -> Vec<Option<PathBuf>> {
        std::iter::once(None).chain(crate::playlist::list_own().into_iter().map(Some)).collect()
    }

    pub(super) fn add_playlist_action(&mut self, a: Action) {
        let n = self.pl_choices().len();
        match a {
            Action::Up => self.pl_focus = self.pl_focus.saturating_sub(1),
            Action::Down => self.pl_focus = (self.pl_focus + 1).min(n.saturating_sub(1)),
            Action::Home => self.pl_focus = 0,
            Action::End => self.pl_focus = n.saturating_sub(1),
            Action::Back | Action::AddToPlaylist | Action::Menu => {
                self.overlay = Overlay::None;
                self.pl_files.clear();
            }
            Action::Accept | Action::Right => {
                let target = self.pl_choices().get(self.pl_focus).cloned().flatten();
                self.add_to_playlist(target);
            }
            _ => {}
        }
    }

    pub(super) fn draw_add_playlist(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let p = ui.painter().clone();
        let dim = ui.interact(rect, ui.scope_id().with("pl-dim"), Sense::CLICK);
        p.rect_filled(rect, 0.0, black(0.55));
        let choices = self.pl_choices();
        let row_h = 38.0;
        let shown = choices.len().min(10);
        let w = 460.0;
        let h = 56.0 + row_h * shown as f32 + 44.0;
        let r = Rect::from_center_size(rect.center(), vec2(w, h));
        if dim.clicked() && !r.contains(dim.interact_pointer_pos().unwrap_or_default()) {
            out.push(Action::Back);
        }
        p.rect_filled(r, 0.0, pal.panel);
        let what = match self.pl_files.len() {
            1 => self.pl_files.first().map(|f| self.title_of(f)).unwrap_or_default(),
            n => format!("{n} videos"),
        };
        text(&p, pos2(r.left() + 20.0, r.top() + 20.0), Align2::LEFT_CENTER, "ADD TO PLAYLIST", 11.0, pal.faint);
        text_clip(&p, pos2(r.left() + 20.0, r.top() + 40.0), w - 40.0, &what, 14.0, pal.dim);
        let first = self.pl_focus.saturating_sub(shown.saturating_sub(1));
        for (vi, (i, c)) in choices.iter().enumerate().skip(first).take(shown).enumerate() {
            let rr = Rect::from_min_size(pos2(r.left(), r.top() + 56.0 + vi as f32 * row_h), vec2(w, row_h));
            let focused = i == self.pl_focus;
            if focused {
                p.rect_filled(rr, 0.0, pal.accent);
            }
            let (label, count) = match c {
                None => ("+ New playlist".to_string(), String::new()),
                Some(path) => (crate::playlist::name(path), format!("{}", crate::playlist::read(path).len())),
            };
            let fg = if focused { pal.ink } else if c.is_none() { pal.accent } else { pal.text };
            text_clip(&p, pos2(rr.left() + 20.0, rr.center().y), w - 90.0, &label, 16.0, fg);
            text(&p, pos2(rr.right() - 20.0, rr.center().y), Align2::RIGHT_CENTER, count, 12.0, if focused { pal.ink } else { pal.faint });
            if ui.interact(rr, ui.scope_id().with(("pl", i)), Sense::CLICK).clicked() {
                self.pl_focus = i;
                out.push(Action::Accept);
            }
        }
        let list: &[(Action, &str)] = &[(Action::Accept, "Add"), (Action::Back, "Cancel")];
        out.extend(self.hints(ui, pos2(r.left() + 20.0, r.bottom() - 22.0), Context::List, list, 12.0, 18.0, pal.dim, pal.raise));
        out
    }
}
