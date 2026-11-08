//! On-screen keyboard for controllers: type a browser search filter or a palette query.

use super::draw::*;
use super::{App, Overlay};
use crate::input::PadInput;
use egui::{Align2, Rect, Sense, pos2, vec2};

const ROWS: [&str; 4] = ["1234567890", "qwertyuiop", "asdfghjkl'", "zxcvbnm,.-"];
const COLS: usize = 10;

/// Bottom row: (label, start column, width in columns).
const BOTTOM: [(Key, usize, usize); 4] = [(Key::Space, 0, 4), (Key::Delete, 4, 2), (Key::Clear, 6, 2), (Key::Done, 8, 2)];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Key {
    Char(char),
    Space,
    Delete,
    Clear,
    Done,
}

impl Key {
    fn label(self) -> String {
        match self {
            Key::Char(c) => c.to_string(),
            Key::Space => "space".into(),
            Key::Delete => "delete".into(),
            Key::Clear => "clear".into(),
            Key::Done => "done".into(),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum OskTarget {
    Filter,
    Palette,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Osk {
    pub target: OskTarget,
    row: usize,
    col: usize,
}

impl Osk {
    pub fn new(target: OskTarget) -> Self {
        // Start on "q": the most likely first letter row.
        Self { target, row: 1, col: 0 }
    }

    pub fn key(&self) -> Key {
        if self.row < ROWS.len() {
            Key::Char(ROWS[self.row].chars().nth(self.col).unwrap_or(' '))
        } else {
            BOTTOM.iter().find(|(_, s, w)| self.col >= *s && self.col < s + w).map(|b| b.0).unwrap_or(Key::Done)
        }
    }

    /// Move the selection; wraps around like most console keyboards.
    pub fn step(&mut self, dx: i32, dy: i32) {
        let rows = ROWS.len() + 1;
        if dy != 0 {
            self.row = (self.row as i32 + dy).rem_euclid(rows as i32) as usize;
        }
        if dx != 0 {
            if self.row == ROWS.len() {
                // Bottom row: jump whole keys.
                let i = BOTTOM.iter().position(|(_, s, w)| self.col >= *s && self.col < s + w).unwrap_or(0) as i32;
                let n = (i + dx).rem_euclid(BOTTOM.len() as i32) as usize;
                self.col = BOTTOM[n].1;
            } else {
                self.col = (self.col as i32 + dx).rem_euclid(COLS as i32) as usize;
            }
        }
    }
}

impl App {
    pub(crate) fn open_osk(&mut self, target: OskTarget) {
        self.osk = Some(Osk::new(target));
        if target == OskTarget::Filter {
            self.filtering = true;
        }
    }

    pub(crate) fn close_osk(&mut self) {
        if let Some(o) = self.osk.take() {
            if o.target == OskTarget::Filter {
                self.filtering = false;
            }
        }
    }

    fn osk_text(&mut self) -> &mut String {
        match self.osk.map(|o| o.target) {
            Some(OskTarget::Palette) => &mut self.palette_query,
            _ => &mut self.filter,
        }
    }

    fn osk_changed(&mut self) {
        match self.osk.map(|o| o.target) {
            Some(OskTarget::Palette) => self.palette_focus = 0,
            _ => {
                self.apply_filter();
                self.focus = 0;
            }
        }
    }

    pub(crate) fn osk_press(&mut self, key: Key) {
        match key {
            Key::Char(c) => self.osk_text().push(c),
            Key::Space => self.osk_text().push(' '),
            Key::Delete => {
                self.osk_text().pop();
            }
            Key::Clear => self.osk_text().clear(),
            Key::Done => return self.close_osk(),
        }
        self.osk_changed();
    }

    /// Controller input while the keyboard is open. Fixed controls, shown on the keyboard itself.
    pub(crate) fn osk_pad(&mut self, p: PadInput) {
        use PadInput::*;
        let Some(osk) = self.osk.as_mut() else { return };
        match p {
            DpadUp | LstickUp => osk.step(0, -1),
            DpadDown | LstickDown => osk.step(0, 1),
            DpadLeft | LstickLeft => osk.step(-1, 0),
            DpadRight | LstickRight => osk.step(1, 0),
            A => {
                let k = osk.key();
                self.osk_press(k);
            }
            X => self.osk_press(Key::Delete),
            Y => self.osk_press(Key::Space),
            B | Start => self.close_osk(),
            _ => {}
        }
    }

    /// Keep the keyboard in sync with what it types into (e.g. the palette was closed).
    pub(crate) fn osk_sync(&mut self) {
        match self.osk.map(|o| o.target) {
            Some(OskTarget::Filter) if !self.filtering || self.overlay != Overlay::None => self.close_osk(),
            Some(OskTarget::Palette) if self.overlay != Overlay::Palette => self.osk = None,
            _ => {}
        }
    }

    pub(super) fn draw_osk(&mut self, ui: &mut egui::Ui, area: Rect) {
        let Some(osk) = self.osk else { return };
        let pal = self.pal;
        let p = ui.painter().clone();
        let (kw, kh, gap, pad) = (50.0, 42.0, 4.0, 14.0);
        let grid_w = kw * COLS as f32 + gap * (COLS as f32 - 1.0);
        let w = grid_w + pad * 2.0;
        let h = pad + 34.0 + (kh + gap) * 5.0 - gap + 10.0 + 26.0 + pad;
        let r = Rect::from_min_size(pos2(area.center().x - w / 2.0, area.bottom() - h - 12.0), vec2(w, h));
        // Swallow clicks on the panel background.
        let _ = ui.interact(r, ui.scope_id().with("osk-bg"), Sense::CLICK);
        p.rect_filled(r, 0.0, pal.panel);

        // What's been typed so far.
        let (label, typed) = match osk.target {
            OskTarget::Filter => ("SEARCH", self.filter.clone()),
            OskTarget::Palette => ("COMMAND", self.palette_query.clone()),
        };
        let ty = r.top() + pad + 15.0;
        let lr = text(&p, pos2(r.left() + pad, ty), Align2::LEFT_CENTER, label, 11.0, pal.faint);
        let field = Rect::from_min_max(pos2(lr.right() + 12.0, ty - 14.0), pos2(r.right() - pad, ty + 14.0));
        p.rect_filled(field, 0.0, pal.bg);
        let tr = text_clip_start(&p, pos2(field.left() + 10.0, ty), field.width() - 24.0, &typed, 16.0, pal.text);
        let cx = if typed.is_empty() { field.left() + 10.0 } else { tr.right() + 1.0 };
        p.rect_filled(Rect::from_min_size(pos2(cx, ty - 9.0), vec2(2.0, 18.0)), 0.0, pal.accent);

        // Keys.
        let gx = r.left() + pad;
        let gy = r.top() + pad + 34.0;
        let mut clicked = None;
        let mut key_rect = |row: usize, col: usize, span: usize, key: Key| {
            let kr = Rect::from_min_size(
                pos2(gx + col as f32 * (kw + gap), gy + row as f32 * (kh + gap)),
                vec2(kw * span as f32 + gap * (span as f32 - 1.0), kh),
            );
            let focused = osk.key() == key && osk.row == row;
            p.rect_filled(kr, 0.0, if focused { pal.accent } else { pal.raise });
            let size = if matches!(key, Key::Char(_)) { 18.0 } else { 13.0 };
            text(&p, kr.center(), Align2::CENTER_CENTER, key.label(), size, if focused { pal.ink } else { pal.text });
            if ui.interact(kr, ui.scope_id().with(("osk", row, col)), Sense::CLICK).clicked() {
                clicked = Some((row, col, key));
            }
        };
        for (ri, row) in ROWS.iter().enumerate() {
            for (ci, ch) in row.chars().enumerate() {
                key_rect(ri, ci, 1, Key::Char(ch));
            }
        }
        for (key, start, span) in BOTTOM {
            key_rect(ROWS.len(), start, span, key);
        }
        if let Some((row, col, key)) = clicked {
            if let Some(o) = self.osk.as_mut() {
                o.row = row;
                o.col = col;
            }
            self.osk_press(key);
        }

        // Fixed controller controls for the keyboard.
        let hy = r.bottom() - pad - 13.0;
        let mut x = gx;
        for (b, what) in [(PadInput::A, "type"), (PadInput::X, "delete"), (PadInput::Y, "space"), (PadInput::B, "done")] {
            let cap = b.label(self.pad_kind);
            let cr = chip(&p, pos2(x, hy), cap, 11.0, b.color(self.pad_kind).unwrap_or(pal.text), pal.raise);
            x = text(&p, pos2(cr.right() + 6.0, hy), Align2::LEFT_CENTER, what, 12.0, pal.dim).right() + 16.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_wraps_and_bottom_row_jumps_keys() {
        let mut o = Osk::new(OskTarget::Filter);
        assert_eq!(o.key(), Key::Char('q'));
        o.step(-1, 0);
        assert_eq!(o.key(), Key::Char('p'));
        o.step(0, 3); // rows 1 → 4 (bottom row), column 9 → "done"
        assert_eq!(o.key(), Key::Done);
        o.step(1, 0);
        assert_eq!(o.key(), Key::Space);
        o.step(1, 0);
        assert_eq!(o.key(), Key::Delete);
        o.step(0, 1); // wraps to the number row
        assert_eq!(o.key(), Key::Char('5'));
    }
}
