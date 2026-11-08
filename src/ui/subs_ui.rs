//! "Download subtitles…": OpenSubtitles results for the playing video; picking one adds it.

use super::draw::*;
use super::{App, Overlay};
use crate::action::{Action, Context};
use egui::{Align2, Rect, Sense, pos2, vec2};
use std::path::PathBuf;

/// The subtitle search panel.
#[derive(Clone, Debug, Default)]
pub(crate) struct SubDl {
    pub video: PathBuf,
    pub busy: bool,
    pub results: Vec<crate::subs::Found>,
    pub error: Option<String>,
    pub focus: usize,
}

/// Track id used for the "Download subtitles…" entry in subtitle lists.
pub(crate) const DOWNLOAD_ID: i64 = -1;

impl App {
    /// Start a search for the playing video (from the Subs hold menu, Tracks, D, or the palette).
    pub(crate) fn open_sub_download(&mut self) {
        let Some(video) = self.playing_path.clone() else {
            self.toast("Play something first");
            return;
        };
        let ep = self.episode_of(&video);
        let item = self.db.file(&video).and_then(|f| self.db.item(&f.item)).cloned();
        let title = item.as_ref().map(|i| i.name().to_string()).unwrap_or_else(|| self.title_of(&video));
        let query = crate::subs::Query {
            path: video.clone(),
            languages: self.cfg.sub_languages.clone(),
            title: ep.as_ref().map(|e| e.show.clone()).unwrap_or(title),
            year: item.as_ref().and_then(|i| i.info.year.or(i.year)),
            season: ep.as_ref().map(|e| e.season).filter(|s| *s < 1900),
            episode: ep.as_ref().map(|e| e.episode).filter(|_| ep.as_ref().is_some_and(|e| e.season < 1900)),
            tmdb: item.as_ref().and_then(|i| i.tmdb),
            is_episode: ep.is_some(),
        };
        self.sub_dl = Some(SubDl { video, busy: true, ..Default::default() });
        self.overlay = Overlay::SubDownload;
        let key = self.cfg.opensubtitles_api_key.clone();
        let s = self.scrape_settings();
        self.scraper.job(crate::scrape::Job::SubSearch { query, key }, s);
    }

    pub(crate) fn on_sub_results(&mut self, video: PathBuf, result: Result<Vec<crate::subs::Found>, String>) {
        let Some(d) = self.sub_dl.as_mut().filter(|d| d.video == video) else { return };
        d.busy = false;
        match result {
            Ok(list) => d.results = list,
            Err(e) => d.error = Some(e),
        }
        d.focus = 0;
    }

    pub(crate) fn on_sub_saved(&mut self, video: PathBuf, result: Result<PathBuf, String>) {
        match result {
            Ok(path) => {
                if self.playing_path.as_ref() == Some(&video) {
                    if let Some(p) = &self.player {
                        p.add_sub(&path, true);
                    }
                }
                self.toast("Subtitles downloaded");
                if self.overlay == Overlay::SubDownload {
                    self.overlay = Overlay::None;
                }
                self.sub_dl = None;
            }
            Err(e) => {
                if let Some(d) = self.sub_dl.as_mut() {
                    d.busy = false;
                    d.error = Some(e.clone());
                }
                self.toast(e);
            }
        }
    }

    pub(super) fn sub_dl_action(&mut self, a: Action) {
        let Some(d) = self.sub_dl.clone() else {
            self.overlay = Overlay::None;
            return;
        };
        let n = d.results.len();
        match a {
            Action::Up => self.sub_dl.as_mut().unwrap().focus = d.focus.saturating_sub(1),
            Action::Down => self.sub_dl.as_mut().unwrap().focus = (d.focus + 1).min(n.saturating_sub(1)),
            Action::PageUp => self.sub_dl.as_mut().unwrap().focus = d.focus.saturating_sub(8),
            Action::PageDown => self.sub_dl.as_mut().unwrap().focus = (d.focus + 8).min(n.saturating_sub(1)),
            Action::Back | Action::DownloadSubs => {
                self.overlay = Overlay::None;
                self.sub_dl = None;
            }
            Action::Accept if !d.busy => {
                let Some(found) = d.results.get(d.focus).cloned() else { return };
                if let Some(dd) = self.sub_dl.as_mut() {
                    dd.busy = true;
                    dd.error = None;
                }
                let key = self.cfg.opensubtitles_api_key.clone();
                let s = self.scrape_settings();
                self.scraper.job(crate::scrape::Job::SubDownload { video: d.video, found, key }, s);
            }
            _ => {}
        }
    }

    pub(super) fn draw_sub_dl(&mut self, ui: &mut egui::Ui, rect: Rect) -> Vec<Action> {
        let pal = self.pal;
        let mut out = Vec::new();
        let Some(d) = self.sub_dl.clone() else { return out };
        let p = ui.painter().clone();
        let dim = ui.interact(rect, ui.scope_id().with("sdl-dim"), Sense::CLICK);
        p.rect_filled(rect, 0.0, black(0.55));
        let w = (rect.width() * 0.55).clamp(420.0, 720.0);
        let h = (rect.height() - 120.0).min(520.0);
        let r = Rect::from_center_size(rect.center(), vec2(w, h));
        if dim.clicked() && !r.contains(dim.interact_pointer_pos().unwrap_or_default()) {
            out.push(Action::Back);
        }
        p.rect_filled(r, 0.0, black(0.88));
        let x = r.left() + 18.0;
        text(&p, pos2(x, r.top() + 20.0), Align2::LEFT_CENTER, format!("DOWNLOAD SUBTITLES · {}", self.cfg.sub_languages.to_uppercase()), 11.0, white(0.45));
        text_clip(&p, pos2(x, r.top() + 42.0), w - 36.0, &self.title_of(&d.video), 14.0, white(0.7));
        let mut y = r.top() + 62.0;
        let row_h = 40.0;
        let bottom = r.bottom() - 40.0;
        if d.busy && d.results.is_empty() {
            text(&p, pos2(x, y + 16.0), Align2::LEFT_CENTER, "Searching OpenSubtitles…", 14.0, white(0.6));
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        } else if let Some(e) = &d.error {
            text_wrap(&p, pos2(x, y + 6.0), w - 36.0, e, 14.0, pal.boost, 4);
        } else if d.results.is_empty() {
            text(&p, pos2(x, y + 16.0), Align2::LEFT_CENTER, "Nothing found in these languages (sub_languages in config.toml)", 14.0, white(0.6));
        }
        let visible = (((bottom - y) / row_h).floor() as usize).max(1);
        let first = d.focus.saturating_sub(visible.saturating_sub(1));
        for (i, f) in d.results.iter().enumerate().skip(first).take(visible) {
            let rr = Rect::from_min_size(pos2(r.left(), y), vec2(w, row_h));
            let focused = i == d.focus;
            if focused {
                p.rect_filled(rr, 0.0, pal.accent);
            }
            let (fg, mc) = if focused { (pal.ink, pal.ink) } else { (white(0.85), white(0.45)) };
            let cy = rr.center().y;
            text(&p, pos2(x, cy), Align2::LEFT_CENTER, f.language.to_uppercase(), 12.0, if focused { pal.ink } else { pal.accent });
            let mut tags = Vec::new();
            if f.exact {
                tags.push("exact file".to_string());
            }
            if f.hearing_impaired {
                tags.push("SDH".into());
            }
            tags.push(format!("{} dl", f.downloads));
            let meta = tags.join(" · ");
            let mw = measure(&p, &meta, 12.0).x;
            text_clip(&p, pos2(x + 40.0, cy), w - 100.0 - mw, &f.release, 14.0, fg);
            text(&p, pos2(rr.right() - 18.0, cy), Align2::RIGHT_CENTER, meta, 12.0, mc);
            if ui.interact(rr, ui.scope_id().with(("sdl", i)), Sense::CLICK).clicked() {
                if let Some(dd) = self.sub_dl.as_mut() {
                    dd.focus = i;
                }
                out.push(Action::Accept);
            }
            y += row_h;
        }
        let label = if d.busy && !d.results.is_empty() { "Downloading…" } else { "Download" };
        let list: &[(Action, &str)] = if d.results.is_empty() { &[(Action::Back, "Close")] } else { &[(Action::Accept, label), (Action::Back, "Close")] };
        out.extend(self.hints(ui, pos2(x, r.bottom() - 20.0), Context::List, list, 12.0, 18.0, white(0.5), white(0.1)));
        out
    }
}
