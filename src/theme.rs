//! Color schemes. Every color in the UI comes from a [`Palette`].

use egui::{Color32, CornerRadius, Shadow, Stroke, Visuals};

#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// Main background.
    pub bg: Color32,
    /// Side panels (info panel, drawers, menus).
    pub panel: Color32,
    /// Raised elements: key caps, buttons, input fields.
    pub raise: Color32,
    pub text: Color32,
    pub dim: Color32,
    pub faint: Color32,
    /// Focus, progress, primary actions.
    pub accent: Color32,
    /// Text drawn on top of `accent`.
    pub ink: Color32,
    /// Volume boost (over 100%), warnings.
    pub boost: Color32,
    /// "Watched", success.
    pub ok: Color32,
    /// Alternating row stripe (derived).
    pub alt: Color32,
    /// Soft accent tint for backgrounds (derived).
    pub tint: Color32,
}

#[cfg(test)]
pub struct Theme {
    pub id: &'static str,
    pub name: &'static str,
    pub palette: Palette,
}

pub const DEFAULT_THEME: &str = "everforest";

/// id, display name, then bg, panel, raise, text, dim, faint, accent, ink, boost, ok.
const RAW: &[(&str, &str, [u32; 10])] = &[
    ("amber-crt", "Amber CRT", [0x0c0c0c, 0x131313, 0x1f1f1f, 0xe8e6e1, 0x8f8c86, 0x55534f, 0xffb000, 0x0c0c0c, 0xff5a36, 0x7bc96f]),
    ("green-phosphor", "Green phosphor", [0x060a06, 0x0c130c, 0x152215, 0xb9f3b1, 0x62a05c, 0x37593a, 0x39ff6a, 0x031003, 0xff6040, 0x39ff6a]),
    ("darkroom-red", "Darkroom red", [0x0a0505, 0x130a0a, 0x201010, 0xecd0ca, 0xa0726b, 0x5c3c38, 0xff3b30, 0x0a0505, 0xffb000, 0xd0a050]),
    ("mono", "Mono", [0x0b0b0b, 0x151515, 0x242424, 0xeeeeee, 0x8c8c8c, 0x525252, 0xffffff, 0x000000, 0xff5a5a, 0xbdbdbd]),
    ("ibm-blue", "IBM blue", [0x0a0f1e, 0x0f1730, 0x18234a, 0xdfe6ff, 0x8692c0, 0x4a557e, 0x5c8dff, 0x050a18, 0xff6b5a, 0x7ad1a0]),
    ("gruvbox", "Gruvbox", [0x1d2021, 0x282828, 0x3c3836, 0xebdbb2, 0xa89984, 0x665c54, 0xfabd2f, 0x1d2021, 0xfb4934, 0xb8bb26]),
    ("nord", "Nord", [0x2e3440, 0x3b4252, 0x434c5e, 0xeceff4, 0xa3abba, 0x69728a, 0x88c0d0, 0x2e3440, 0xbf616a, 0xa3be8c]),
    ("solarized-dark", "Solarized dark", [0x002b36, 0x073642, 0x0e4655, 0xeee8d5, 0x93a1a1, 0x587179, 0xb58900, 0x002b36, 0xdc322f, 0x859900]),
    ("tokyo-night", "Tokyo Night", [0x1a1b26, 0x20212e, 0x2b2d40, 0xc0caf5, 0x8089b3, 0x4b5070, 0x7aa2f7, 0x1a1b26, 0xf7768e, 0x9ece6a]),
    ("catppuccin-mocha", "Catppuccin Mocha", [0x181825, 0x1e1e2e, 0x313244, 0xcdd6f4, 0xa6adc8, 0x6c7086, 0xcba6f7, 0x1e1e2e, 0xf38ba8, 0xa6e3a1]),
    ("rose-pine", "Rosé Pine", [0x191724, 0x1f1d2e, 0x26233a, 0xe0def4, 0x908caa, 0x6e6a86, 0xebbcba, 0x191724, 0xeb6f92, 0x9ccfd8]),
    ("everforest", "Everforest", [0x272e33, 0x2e383c, 0x374145, 0xd3c6aa, 0x9da9a0, 0x62706b, 0xa7c080, 0x272e33, 0xe67e80, 0x83c092]),
    ("kanagawa", "Kanagawa", [0x16161d, 0x1f1f28, 0x2a2a37, 0xdcd7ba, 0x9a9888, 0x54546d, 0xe6c384, 0x16161d, 0xff5d62, 0x98bb6c]),
    ("monokai", "Monokai", [0x1e1f1c, 0x272822, 0x3e3d32, 0xf8f8f2, 0xa59f85, 0x75715e, 0xa6e22e, 0x1e1f1c, 0xf92672, 0xe6db74]),
    ("one-dark", "One Dark", [0x21252b, 0x282c34, 0x323842, 0xdcdfe4, 0x8a92a0, 0x5c6370, 0x61afef, 0x21252b, 0xe06c75, 0x98c379]),
    ("dracula", "Dracula", [0x21222c, 0x282a36, 0x343746, 0xf8f8f2, 0xa0a4bf, 0x6272a4, 0xbd93f9, 0x21222c, 0xff5555, 0x50fa7b]),
    ("synthwave", "Synthwave", [0x140b24, 0x1c1033, 0x2a1a4a, 0xf4e9ff, 0xa68cc7, 0x5e4a80, 0xff2bd6, 0x140b24, 0xffb000, 0x36f9f6]),
    ("deep-ocean", "Deep ocean", [0x0b1a1f, 0x10242b, 0x17323b, 0xd8f1f0, 0x7fa9ad, 0x46666b, 0x2ee6c8, 0x04201c, 0xff6b5a, 0x2ee6c8]),
    ("paper", "Paper (light)", [0xf4f1ea, 0xebe7dd, 0xddd8cb, 0x1f1d1a, 0x6b665c, 0xa39d90, 0xd9480f, 0xffffff, 0xc92a2a, 0x2b8a3e]),
    ("solarized-light", "Solarized light", [0xfdf6e3, 0xeee8d5, 0xe2dac2, 0x073642, 0x586e75, 0x93a1a1, 0x268bd2, 0xfdf6e3, 0xdc322f, 0x859900]),
];

const fn hex(v: u32) -> Color32 {
    Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
}

/// Linear mix of two colors: `t = 0` gives `a`, `t = 1` gives `b`.
pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn build(c: [u32; 10]) -> Palette {
    let [bg, panel, raise, text, dim, faint, accent, ink, boost, ok] = c.map(hex);
    Palette {
        bg,
        panel,
        raise,
        text,
        dim,
        faint,
        accent,
        ink,
        boost,
        ok,
        alt: mix(bg, panel, 0.55),
        tint: mix(bg, accent, 0.13),
    }
}

#[cfg(test)]
pub fn all() -> Vec<Theme> {
    RAW.iter()
        .map(|(id, name, c)| Theme { id, name, palette: build(*c) })
        .collect()
}

pub fn index_of(id: &str) -> usize {
    RAW.iter()
        .position(|(i, ..)| *i == id)
        .or_else(|| RAW.iter().position(|(i, ..)| *i == DEFAULT_THEME))
        .unwrap_or(0)
}

pub fn count() -> usize {
    RAW.len()
}

pub fn id_at(i: usize) -> &'static str {
    RAW[i % RAW.len()].0
}

pub fn name_at(i: usize) -> &'static str {
    RAW[i % RAW.len()].1
}

pub fn palette(id: &str) -> Palette {
    build(RAW[index_of(id)].2)
}

fn is_light(c: Color32) -> bool {
    (c.r() as u32 * 299 + c.g() as u32 * 587 + c.b() as u32 * 114) / 1000 > 140
}

/// Push the palette into egui's own visuals so built-in widgets (text edit, scroll bars) match.
pub fn apply(ctx: &egui::Context, p: &Palette) {
    let mut v = if is_light(p.bg) { Visuals::light() } else { Visuals::dark() };
    let none = CornerRadius::ZERO;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.bg;
    v.window_fill = p.panel;
    v.extreme_bg_color = p.raise;
    v.faint_bg_color = p.alt;
    v.code_bg_color = p.raise;
    v.hyperlink_color = p.accent;
    v.window_corner_radius = none;
    v.menu_corner_radius = none;
    v.window_stroke = Stroke::NONE;
    v.window_shadow = Shadow::NONE;
    v.popup_shadow = Shadow::NONE;
    v.selection.bg_fill = mix(p.bg, p.accent, 0.35);
    v.selection.stroke = Stroke::new(1.0, p.accent);
    v.text_cursor.stroke = Stroke::new(2.0, p.accent);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
        &mut v.widgets.open,
    ] {
        w.corner_radius = none;
        w.bg_stroke = Stroke::NONE;
        w.bg_fill = p.raise;
        w.weak_bg_fill = p.raise;
        w.fg_stroke = Stroke::new(1.0, p.text);
        w.expansion = 0.0;
    }
    v.widgets.hovered.bg_fill = mix(p.raise, p.accent, 0.2);
    v.widgets.active.bg_fill = p.accent;
    v.widgets.active.fg_stroke = Stroke::new(1.0, p.ink);
    ctx.set_visuals(v);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twenty_themes_with_unique_ids() {
        let t = all();
        assert_eq!(t.len(), 20);
        let mut ids: Vec<_> = t.iter().map(|t| t.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 20);
        assert!(t.iter().all(|t| !t.name.is_empty() && t.palette.text != t.palette.bg));
    }

    #[test]
    fn unknown_theme_falls_back_to_everforest() {
        assert_eq!(id_at(index_of("nope")), "everforest");
        assert_eq!(id_at(index_of(DEFAULT_THEME)), "everforest");
    }
}
