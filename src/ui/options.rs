//! The Options menu (□ / X / Shift+Enter / O): everything you can do with the selected row.

use super::draw::*;
use super::{App, Item, Overlay, Source};
use crate::action::{Action, Context};
use egui::{Align2, Rect, Sense, pos2, vec2};
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Opt {
    Play,
    PlayFromStart,
    /// A show: its next episode.
    PlayNext(String),
    /// A show: its page.
    Episodes(String),
    /// A playlist: play it from the top.
    PlayAll(PathBuf),
    Open,
    AddToPlaylist,
    RemoveFromPlaylist(PathBuf),
    /// Mark a file (or every episode of a show) watched / unwatched.
    SetWatched(bool),
    ToggleLibrary(PathBuf),
    FixMatch,
}

impl App {
    /// What the Options menu offers for a row.
    fn options_for(&self, item: &Item) -> Vec<(String, Opt)> {
        let mut v = Vec::new();
        let in_db = self.key_of(item).is_some_and(|k| self.db.item(&k).is_some());
        if let Some(key) = &item.show {
            if let Some(show) = self.catalog.show(key) {
                let next = &show.episodes[self.next_up(show)];
                let resume = self.hist.get(&next.1.path).is_some_and(|h| h.resumable());
                v.push((format!("{} {}", if resume { "Resume" } else { "Play" }, next.0.code()), Opt::PlayNext(key.clone())));
                v.push(("Episodes".into(), Opt::Episodes(key.clone())));
                let all_watched = show.episodes.iter().all(|(_, f)| self.hist.get(&f.path).is_some_and(|h| h.watched));
                v.push((if all_watched { "Mark show unwatched" } else { "Mark show watched" }.into(), Opt::SetWatched(!all_watched)));
            }
            v.push(("Add to playlist…".into(), Opt::AddToPlaylist));
        } else if crate::playlist::is_playlist(&item.entry.path) {
            v.push(("Open".into(), Opt::Open));
            v.push(("Play all".into(), Opt::PlayAll(item.entry.path.clone())));
            v.push(("Add to another playlist…".into(), Opt::AddToPlaylist));
        } else if item.entry.is_dir {
            v.push(("Open".into(), Opt::Open));
            let in_lib = self.cfg.roots.contains(&item.entry.path);
            v.push((if in_lib { "Remove from library" } else { "Add to library…" }.into(), Opt::ToggleLibrary(item.entry.path.clone())));
        } else {
            let h = item.hist.as_ref();
            match h.filter(|h| h.resumable()) {
                Some(h) => {
                    v.push((format!("Resume from {}", fmt_time(h.pos)), Opt::Play));
                    v.push(("Play from start".into(), Opt::PlayFromStart));
                }
                None => v.push(("Play".into(), Opt::Play)),
            }
            v.push(("Add to playlist…".into(), Opt::AddToPlaylist));
            if let Source::Playlist(pl) = &self.source {
                if crate::playlist::is_own(pl) {
                    v.push(("Remove from this playlist".into(), Opt::RemoveFromPlaylist(pl.clone())));
                }
            }
            let watched = h.is_some_and(|h| h.watched);
            v.push((if watched { "Mark unwatched" } else { "Mark watched" }.into(), Opt::SetWatched(!watched)));
        }
        if in_db {
            v.push(("Fix match…".into(), Opt::FixMatch));
        }
        v
    }

    pub(crate) fn open_options(&mut self) {
        let Some(item) = self.items.get(self.focus).cloned() else { return };
        self.opt_list = self.options_for(&item);
        self.opt_focus = 0;
        if !self.opt_list.is_empty() {
            self.overlay = Overlay::Options;
        }
    }

    fn run_option(&mut self, o: Opt) {
        self.overlay = Overlay::None;
        let item = self.items.get(self.focus).cloned();
        match o {
            Opt::Play | Opt::Open => self.activate(false),
            Opt::PlayFromStart => self.activate(true),
            Opt::PlayNext(key) => self.play_next_up(&key),
            Opt::Episodes(_) => self.activate(false),
            Opt::PlayAll(pl) => {
                let list = crate::playlist::read(&pl);
                self.play_queue(list, 0, crate::playlist::name(&pl), true);
            }
            Opt::AddToPlaylist => {
                let files = self.files_for_playlist();
                self.open_add_to_playlist(files);
            }
            Opt::RemoveFromPlaylist(pl) => {
                if let Some(i) = item {
                    match crate::playlist::remove(&pl, &i.entry.path) {
                        Ok(()) => self.toast(format!("Removed from {}", crate::playlist::name(&pl))),
                        Err(e) => self.toast(format!("Couldn't save playlist: {e}")),
                    }
                    self.rebuild_playlist_view();
                }
            }
            Opt::SetWatched(w) => {
                let Some(i) = item else { return };
                let files: Vec<PathBuf> = match i.show.as_ref().and_then(|k| self.catalog.show(k)) {
                    Some(show) => show.episodes.iter().map(|(_, f)| f.path.clone()).collect(),
                    None => vec![i.entry.path.clone()],
                };
                for f in &files {
                    let e = self.hist.entry(f);
                    e.watched = w;
                    e.pos = 0.0;
                    if w && e.last_played == 0 {
                        e.last_played = crate::config::now_unix();
                    }
                }
                self.hist.save_if_dirty();
                self.refresh_hist_in_items();
                self.toast(if w { "Marked watched" } else { "Marked unwatched" });
            }
            Opt::ToggleLibrary(p) => self.toggle_library(&p),
            Opt::FixMatch => self.open_fix_match(),
        }
    }

    pub(super) fn options_action(&mut self, a: Action) {
        let n = self.opt_list.len();
        match a {
            Action::Up => self.opt_focus = self.opt_focus.saturating_sub(1),
            Action::Down => self.opt_focus = (self.opt_focus + 1).min(n.saturating_sub(1)),
            Action::Home => self.opt_focus = 0,
            Action::End => self.opt_focus = n.saturating_sub(1),
            Action::Back | Action::Options | Action::Menu | Action::Left => self.overlay = Overlay::None,
            Action::Accept | Action::Right => {
                if let Some((_, o)) = self.opt_list.get(self.opt_focus).cloned() {
                    self.run_option(o);
                }
            }
            _ => {}
        }
    }

    pub(super) fn draw_options(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let p = ui.painter().clone();
        let dim = ui.interact(rect, ui.scope_id().with("opt-dim"), Sense::CLICK);
        p.rect_filled(rect, 0.0, black(0.5));
        let row_h = 40.0;
        let w = 400.0;
        let h = 56.0 + row_h * self.opt_list.len() as f32 + 12.0;
        let r = Rect::from_center_size(rect.center(), vec2(w, h));
        if dim.clicked() && !r.contains(dim.interact_pointer_pos().unwrap_or_default()) {
            out.push(Action::Back);
        }
        p.rect_filled(r, 0.0, pal.panel);
        let title = self.items.get(self.focus).map(|i| self.row_name(i)).unwrap_or_default();
        text(&p, pos2(r.left() + 20.0, r.top() + 18.0), Align2::LEFT_CENTER, "OPTIONS", 11.0, pal.faint);
        text_clip(&p, pos2(r.left() + 20.0, r.top() + 38.0), w - 40.0, &title, 14.0, pal.dim);
        for (i, (label, _)) in self.opt_list.clone().iter().enumerate() {
            let rr = Rect::from_min_size(pos2(r.left(), r.top() + 56.0 + i as f32 * row_h), vec2(w, row_h));
            let focused = i == self.opt_focus;
            if focused {
                p.rect_filled(rr, 0.0, pal.accent);
            }
            text(&p, pos2(rr.left() + 20.0, rr.center().y), Align2::LEFT_CENTER, label, 16.0, if focused { pal.ink } else { pal.text });
            let resp = ui.interact(rr, ui.scope_id().with(("opt", i)), Sense::CLICK);
            if resp.hovered() && ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO) {
                self.opt_focus = i;
            }
            if resp.clicked() {
                self.opt_focus = i;
                out.push(Action::Accept);
            }
        }
        if self.cfg.show_hints {
            let list: &[(Action, &str)] = &[(Action::Accept, "Select"), (Action::Back, "Close")];
            out.extend(self.hints(ui, pos2(r.left(), r.bottom() + 22.0), Context::List, list, 12.0, 18.0, pal.dim, pal.raise));
        }
        out
    }
}
