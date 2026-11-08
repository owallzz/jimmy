//! Small painting helpers. jimmy draws its screens directly with egui's painter for exact,
//! flat, borderless layouts that match the mockups.

use egui::text::{LayoutJob, TextFormat, TextWrapping};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Vec2, pos2, vec2};

pub fn font(size: f32) -> FontId {
    FontId::monospace(size)
}

/// Plain text at an anchor point. Returns the occupied rect.
pub fn text(p: &Painter, pos: Pos2, anchor: Align2, s: impl ToString, size: f32, color: Color32) -> Rect {
    p.text(pos, anchor, s, font(size), color)
}

/// Single-line text clipped to `width` with an ellipsis. `pos` is the left-center point.
pub fn text_clip(p: &Painter, pos: Pos2, width: f32, s: &str, size: f32, color: Color32) -> Rect {
    let mut job = LayoutJob::single_section(s.to_owned(), TextFormat::simple(font(size), color));
    job.wrap = TextWrapping { max_width: width.max(1.0), max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    let g = p.layout_job(job);
    let r = Rect::from_min_size(pos2(pos.x, pos.y - g.size().y / 2.0), g.size());
    p.galley(r.min, g, color);
    r
}

/// Single-line text clipped from the right edge (`pos` is the right-center point).
pub fn text_clip_right(p: &Painter, pos: Pos2, width: f32, s: &str, size: f32, color: Color32) -> Rect {
    let w = measure(p, s, size).x.min(width);
    text_clip(p, pos2(pos.x - w, pos.y), width, s, size, color)
}

/// Text drawn at `pos` (left-center) that drops characters from the *start* to fit `width`,
/// so the end of a long path stays readable: `…/Videos/Movies/`.
pub fn text_clip_start(p: &Painter, pos: Pos2, width: f32, s: &str, size: f32, color: Color32) -> Rect {
    let mut t: String = s.to_owned();
    let mut cut = false;
    while measure(p, &t, size).x > width && t.chars().count() > 1 {
        t = t.chars().skip(if cut { 2 } else { 1 }).collect();
        t.insert(0, '…');
        cut = true;
    }
    text(p, pos, Align2::LEFT_CENTER, t, size, color)
}

/// Wrapped text inside `width`. `pos` is the top-left corner. Returns height used.
pub fn text_wrap(p: &Painter, pos: Pos2, width: f32, s: &str, size: f32, color: Color32, max_rows: usize) -> f32 {
    let mut job = LayoutJob::single_section(s.to_owned(), TextFormat::simple(font(size), color));
    job.wrap = TextWrapping { max_width: width, max_rows, break_anywhere: false, overflow_character: Some('…') };
    let g = p.layout_job(job);
    let h = g.size().y;
    p.galley(pos, g, color);
    h
}

pub fn measure(p: &Painter, s: &str, size: f32) -> Vec2 {
    p.layout_no_wrap(s.to_owned(), font(size), Color32::WHITE).size()
}

/// A key cap like `[A]`: solid block, no border. `pos` is the left-center. Returns its rect.
pub fn chip(p: &Painter, pos: Pos2, label: &str, size: f32, fg: Color32, bg: Color32) -> Rect {
    let pad = (size * 0.35).round();
    let w = (measure(p, label, size).x + pad * 2.0).max(size * 1.7);
    let h = (size * 1.7).round();
    let r = Rect::from_min_size(pos2(pos.x, (pos.y - h / 2.0).round()), vec2(w, h));
    p.rect_filled(r, 0.0, bg);
    p.text(r.center(), Align2::CENTER_CENTER, label, font(size), fg);
    r
}

/// Thin horizontal progress bar.
pub fn bar(p: &Painter, r: Rect, frac: f32, track: Color32, fill: Color32) {
    p.rect_filled(r, 0.0, track);
    let f = frac.clamp(0.0, 1.0);
    if f > 0.0 {
        p.rect_filled(Rect::from_min_size(r.min, vec2(r.width() * f, r.height())), 0.0, fill);
    }
}

/// Vertical gradient (for the player's top/bottom fades).
pub fn vgradient(p: &Painter, r: Rect, top: Color32, bottom: Color32) {
    use egui::epaint::{Mesh, Vertex, WHITE_UV};
    let mut m = Mesh::default();
    let v = |pos: Pos2, color| Vertex { pos, uv: WHITE_UV, color };
    m.vertices = vec![v(r.left_top(), top), v(r.right_top(), top), v(r.right_bottom(), bottom), v(r.left_bottom(), bottom)];
    m.indices = vec![0, 1, 2, 0, 2, 3];
    p.add(m);
}

pub fn with_alpha(c: Color32, a: f32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn white(a: f32) -> Color32 {
    Color32::from_white_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn black(a: f32) -> Color32 {
    Color32::from_black_alpha((a.clamp(0.0, 1.0) * 255.0) as u8)
}

pub fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0).round() as u64;
    format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
}

pub fn fmt_dur_short(secs: f64) -> String {
    let m = (secs.max(0.0) / 60.0).round() as u64;
    if m >= 60 { format!("{}h {}m", m / 60, m % 60) } else { format!("{m}m") }
}

pub fn fmt_size(bytes: u64) -> String {
    let b = bytes as f64;
    if b >= 1e9 {
        format!("{:.1} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.0} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} KB", b / 1e3)
    } else {
        format!("{bytes} B")
    }
}

/// Local wall-clock hour and minute for a unix timestamp.
#[cfg(unix)]
pub fn local_hm(unix: i64) -> (u32, u32) {
    let t: libc::time_t = unix as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&t, &mut tm) };
    (tm.tm_hour as u32, tm.tm_min as u32)
}

#[cfg(not(unix))]
pub fn local_hm(unix: i64) -> (u32, u32) {
    (((unix / 3600) % 24) as u32, ((unix / 60) % 60) as u32)
}

static CLOCK_24H: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Switch every clock in the UI between 12-hour (default) and 24-hour.
pub fn set_clock_24h(on: bool) {
    CLOCK_24H.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub fn format_clock(h: u32, m: u32, h24: bool) -> String {
    if h24 {
        format!("{h:02}:{m:02}")
    } else {
        let (h12, ampm) = match h {
            0 => (12, "AM"),
            1..=11 => (h, "AM"),
            12 => (12, "PM"),
            _ => (h - 12, "PM"),
        };
        format!("{h12}:{m:02} {ampm}")
    }
}

pub fn clock_at(unix: i64) -> String {
    let (h, m) = local_hm(unix);
    format_clock(h, m, CLOCK_24H.load(std::sync::atomic::Ordering::Relaxed))
}

pub fn now_unix() -> i64 {
    crate::config::now_unix() as i64
}

/// Local date for a file's modification time, `YYYY-MM-DD`.
#[cfg(unix)]
pub fn local_date(t: std::time::SystemTime) -> String {
    let secs = t.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let tt: libc::time_t = secs as libc::time_t;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&tt, &mut tm) };
    format!("{:04}-{:02}-{:02}", tm.tm_year + 1900, tm.tm_mon + 1, tm.tm_mday)
}

#[cfg(not(unix))]
pub fn local_date(_: std::time::SystemTime) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        assert_eq!(fmt_time(4353.0), "1:12:33");
        assert_eq!(fmt_dur_short(2480.0), "41m");
        assert_eq!(fmt_dur_short(6833.0), "1h 54m");
        assert_eq!(fmt_size(7_300_000_000), "7.3 GB");
        assert_eq!(format_clock(0, 5, false), "12:05 AM");
        assert_eq!(format_clock(12, 0, false), "12:00 PM");
        assert_eq!(format_clock(17, 36, false), "5:36 PM");
        assert_eq!(format_clock(17, 36, true), "17:36");
        assert_eq!(format_clock(7, 3, true), "07:03");
    }
}
