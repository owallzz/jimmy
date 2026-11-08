//! Keyboard and gamepad bindings, plus the gamepad thread.

use crate::action::{Action, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- keyboard

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub key: egui::Key,
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
}

impl KeyChord {
    pub const fn new(key: egui::Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: false }
    }
    pub const fn ctrl(key: egui::Key) -> Self {
        Self { key, ctrl: true, shift: false, alt: false }
    }
    pub const fn shift(key: egui::Key) -> Self {
        Self { key, ctrl: false, shift: true, alt: false }
    }
    pub const fn alt(key: egui::Key) -> Self {
        Self { key, ctrl: false, shift: false, alt: true }
    }

    pub fn from_event(key: egui::Key, m: egui::Modifiers) -> Self {
        // On macOS treat Cmd like Ctrl so Cmd+P etc. work as expected.
        Self { key, ctrl: m.ctrl || m.mac_cmd, shift: m.shift, alt: m.alt }
    }

    /// Text-producing keys without Ctrl/Alt, which the browser filter may want instead.
    pub fn is_plain_text(&self) -> bool {
        use egui::Key::*;
        !self.ctrl
            && !self.alt
            && !matches!(
                self.key,
                ArrowUp | ArrowDown | ArrowLeft | ArrowRight | Enter | Escape | Backspace | Tab | Delete
                    | PageUp | PageDown | Home | End | Insert | F1 | F2 | F3 | F4 | F5 | F6 | F7 | F8
                    | F9 | F10 | F11 | F12
            )
    }

    pub fn label(&self) -> String {
        use egui::Key::*;
        let k = match self.key {
            ArrowUp => "↑".to_string(),
            ArrowDown => "↓".to_string(),
            ArrowLeft => "←".to_string(),
            ArrowRight => "→".to_string(),
            Escape => "Esc".to_string(),
            Backspace => "Bksp".to_string(),
            PageUp => "PgUp".to_string(),
            PageDown => "PgDn".to_string(),
            OpenBracket => "[".to_string(),
            CloseBracket => "]".to_string(),
            Comma => ",".to_string(),
            Period => ".".to_string(),
            Minus => "-".to_string(),
            Equals => "=".to_string(),
            Slash => "/".to_string(),
            k => k.name().to_string(),
        };
        let mut s = String::new();
        if self.ctrl {
            s.push_str("Ctrl+");
        }
        if self.alt {
            s.push_str("Alt+");
        }
        if self.shift {
            s.push_str("Shift+");
        }
        s + &k
    }
}

impl std::fmt::Display for KeyChord {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.ctrl {
            f.write_str("Ctrl+")?;
        }
        if self.alt {
            f.write_str("Alt+")?;
        }
        if self.shift {
            f.write_str("Shift+")?;
        }
        f.write_str(self.key.name())
    }
}

impl std::str::FromStr for KeyChord {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        let mut c = KeyChord::new(egui::Key::Space);
        let parts: Vec<&str> = s.split('+').collect();
        let (last, mods) = parts.split_last().ok_or("empty key")?;
        for m in mods {
            match m.to_ascii_lowercase().as_str() {
                "ctrl" | "control" | "cmd" => c.ctrl = true,
                "shift" => c.shift = true,
                "alt" | "option" => c.alt = true,
                o => return Err(format!("unknown modifier {o}")),
            }
        }
        // `Ctrl++` splits into an empty last part.
        let name = if last.is_empty() { "Plus" } else { last };
        c.key = egui::Key::from_name(name).ok_or_else(|| format!("unknown key {name}"))?;
        Ok(c)
    }
}

// ---------------------------------------------------------------- gamepad

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PadInput {
    A,
    B,
    X,
    Y,
    Lb,
    Rb,
    Lt,
    Rt,
    Start,
    Select,
    Guide,
    Ls,
    Rs,
    DpadUp,
    DpadDown,
    DpadLeft,
    DpadRight,
    LstickUp,
    LstickDown,
    LstickLeft,
    LstickRight,
    RstickUp,
    RstickDown,
    RstickLeft,
    RstickRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PadKind {
    #[default]
    Xbox,
    PlayStation,
}

impl PadInput {
    /// Label for on-screen button hints, matching the controller's own markings.
    pub fn label(self, kind: PadKind) -> &'static str {
        use PadInput::*;
        let ps = kind == PadKind::PlayStation;
        match self {
            A => if ps { "×" } else { "A" },
            B => if ps { "○" } else { "B" },
            X => if ps { "□" } else { "X" },
            Y => if ps { "△" } else { "Y" },
            Lb => if ps { "L1" } else { "LB" },
            Rb => if ps { "R1" } else { "RB" },
            Lt => if ps { "L2" } else { "LT" },
            Rt => if ps { "R2" } else { "RT" },
            Start => "≡",
            Select => "▣",
            Guide => if ps { "PS" } else { "◉" },
            Ls => if ps { "L3" } else { "LS" },
            Rs => if ps { "R3" } else { "RS" },
            DpadUp => "▲",
            DpadDown => "▼",
            DpadLeft => "◀",
            DpadRight => "▶",
            LstickUp => "LS▲",
            LstickDown => "LS▼",
            LstickLeft => "LS◀",
            LstickRight => "LS▶",
            RstickUp => "RS▲",
            RstickDown => "RS▼",
            RstickLeft => "RS◀",
            RstickRight => "RS▶",
        }
    }

    /// Face buttons get their controller color.
    pub fn color(self, kind: PadKind) -> Option<egui::Color32> {
        use egui::Color32 as C;
        let ps = kind == PadKind::PlayStation;
        Some(match self {
            PadInput::A => if ps { C::from_rgb(0x6f, 0x9b, 0xff) } else { C::from_rgb(0x6c, 0xc2, 0x5a) },
            PadInput::B => C::from_rgb(0xff, 0x62, 0x62),
            PadInput::X => if ps { C::from_rgb(0xe0, 0x70, 0xc0) } else { C::from_rgb(0x4f, 0xa3, 0xff) },
            PadInput::Y => if ps { C::from_rgb(0x3f, 0xc7, 0x9a) } else { C::from_rgb(0xe8, 0xb9, 0x23) },
            _ => return None,
        })
    }
}

#[derive(Debug)]
pub enum PadEvent {
    Down(PadInput),
    Up(PadInput),
    Connected { name: String, kind: PadKind },
    Disconnected,
}

fn kind_from_name(name: &str, vendor: Option<u16>) -> PadKind {
    let n = name.to_ascii_lowercase();
    if vendor == Some(0x054c) || n.contains("dualsense") || n.contains("dualshock") || n.contains("playstation") || n.contains("ps5") || n.contains("ps4") || n.contains("sony") {
        PadKind::PlayStation
    } else {
        PadKind::Xbox
    }
}

fn map_button(b: gilrs::Button) -> Option<PadInput> {
    use gilrs::Button as G;
    Some(match b {
        G::South => PadInput::A,
        G::East => PadInput::B,
        G::West => PadInput::X,
        G::North => PadInput::Y,
        G::LeftTrigger => PadInput::Lb,
        G::RightTrigger => PadInput::Rb,
        G::LeftTrigger2 => PadInput::Lt,
        G::RightTrigger2 => PadInput::Rt,
        G::Start => PadInput::Start,
        G::Select => PadInput::Select,
        G::Mode => PadInput::Guide,
        G::LeftThumb => PadInput::Ls,
        G::RightThumb => PadInput::Rs,
        G::DPadUp => PadInput::DpadUp,
        G::DPadDown => PadInput::DpadDown,
        G::DPadLeft => PadInput::DpadLeft,
        G::DPadRight => PadInput::DpadRight,
        _ => return None,
    })
}

/// Turns analog axes into digital presses, with hysteresis so they don't flicker.
#[derive(Default)]
struct AxisState {
    active: HashMap<gilrs::Axis, Option<PadInput>>,
}

impl AxisState {
    fn update(&mut self, axis: gilrs::Axis, value: f32, deadzone: f32, out: &mut Vec<PadEvent>) {
        use gilrs::Axis as A;
        let (neg, pos) = match axis {
            // gilrs: +Y is up.
            A::LeftStickX => (PadInput::LstickLeft, PadInput::LstickRight),
            A::LeftStickY => (PadInput::LstickDown, PadInput::LstickUp),
            A::RightStickX => (PadInput::RstickLeft, PadInput::RstickRight),
            A::RightStickY => (PadInput::RstickDown, PadInput::RstickUp),
            A::DPadX => (PadInput::DpadLeft, PadInput::DpadRight),
            A::DPadY => (PadInput::DpadDown, PadInput::DpadUp),
            _ => return,
        };
        let press = deadzone.max(0.3) + 0.25;
        let release = deadzone.max(0.2);
        let cur = self.active.get(&axis).copied().flatten();
        let want = if value >= press {
            Some(pos)
        } else if value <= -press {
            Some(neg)
        } else if value.abs() < release {
            None
        } else {
            cur
        };
        if want != cur {
            if let Some(c) = cur {
                out.push(PadEvent::Up(c));
            }
            if let Some(w) = want {
                out.push(PadEvent::Down(w));
            }
            self.active.insert(axis, want);
        }
    }
}

/// Spawns the gamepad thread. Returns `None` if gilrs can't start (no gamepad support).
pub fn spawn_gamepad_thread(ctx: egui::Context, deadzone: f32) -> Receiver<PadEvent> {
    let (tx, rx) = channel();
    let _ = std::thread::Builder::new().name("gamepad".into()).spawn(move || {
        let mut g = match gilrs::Gilrs::new() {
            Ok(g) => g,
            Err(e) => {
                log::warn!("gamepad support unavailable: {e}");
                return;
            }
        };
        for (_, pad) in g.gamepads() {
            let _ = tx.send(PadEvent::Connected {
                name: pad.name().to_string(),
                kind: kind_from_name(pad.name(), pad.vendor_id()),
            });
        }
        ctx.request_repaint();
        let mut axes = AxisState::default();
        let mut out = Vec::new();
        loop {
            let Some(ev) = g.next_event_blocking(Some(Duration::from_millis(1000))) else { continue };
            use gilrs::EventType as E;
            match ev.event {
                E::ButtonPressed(b, _) => {
                    if let Some(p) = map_button(b) {
                        out.push(PadEvent::Down(p));
                    }
                }
                E::ButtonReleased(b, _) => {
                    if let Some(p) = map_button(b) {
                        out.push(PadEvent::Up(p));
                    }
                }
                E::AxisChanged(a, v, _) => axes.update(a, v, deadzone, &mut out),
                E::Connected => {
                    let pad = g.gamepad(ev.id);
                    out.push(PadEvent::Connected {
                        name: pad.name().to_string(),
                        kind: kind_from_name(pad.name(), pad.vendor_id()),
                    });
                }
                E::Disconnected => out.push(PadEvent::Disconnected),
                _ => {}
            }
            if !out.is_empty() {
                for e in out.drain(..) {
                    if tx.send(e).is_err() {
                        return;
                    }
                }
                ctx.request_repaint();
            }
        }
    });
    rx
}

// ---------------------------------------------------------------- bindings

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct KeyBinding {
    pub context: Context,
    pub key: String,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PadBinding {
    pub context: Context,
    pub button: PadInput,
    pub action: Action,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(default)]
struct BindingsFile {
    keyboard: Vec<KeyBinding>,
    gamepad: Vec<PadBinding>,
}

#[derive(Clone, Debug)]
pub struct Bindings {
    pub keys: Vec<(Context, KeyChord, Action)>,
    pub pads: Vec<(Context, PadInput, Action)>,
}

impl Bindings {
    pub fn path() -> PathBuf {
        crate::config::config_dir().join("bindings.toml")
    }

    pub fn defaults() -> Self {
        use Action as A;
        use Context::*;
        use egui::Key as K;
        let k = KeyChord::new;
        let c = KeyChord::ctrl;
        let s = KeyChord::shift;
        let keys = vec![
            (Global, KeyChord::new(K::F11), A::Fullscreen),
            (Global, KeyChord::alt(K::Enter), A::Fullscreen),
            (Global, c(K::P), A::Palette),
            (Global, k(K::Colon), A::Palette),
            (Global, s(K::Semicolon), A::Palette),
            (Global, c(K::Q), A::Quit),
            (Global, c(K::Comma), A::Settings),
            (Global, c(K::T), A::NextTheme),
            (Global, KeyChord { key: K::T, ctrl: true, shift: true, alt: false }, A::PrevTheme),
            (Global, c(K::H), A::ToggleHints),
            (Browser, k(K::ArrowUp), A::Up),
            (Browser, k(K::ArrowDown), A::Down),
            (Browser, k(K::ArrowLeft), A::Left),
            (Browser, k(K::ArrowRight), A::Right),
            (Browser, k(K::PageUp), A::PageUp),
            (Browser, k(K::PageDown), A::PageDown),
            (Browser, k(K::Home), A::Home),
            (Browser, k(K::End), A::End),
            (Browser, k(K::Enter), A::Accept),
            (Browser, s(K::Enter), A::Options),
            (Browser, k(K::Backspace), A::Back),
            (Browser, k(K::Escape), A::Back),
            (Browser, k(K::Tab), A::Menu),
            (Browser, k(K::Slash), A::Filter),
            // vim motions (gg and counts like 5j are handled in the app)
            (Browser, k(K::J), A::Down),
            (Browser, k(K::K), A::Up),
            (Browser, k(K::H), A::Left),
            (Browser, k(K::L), A::Right),
            (Browser, s(K::G), A::End),
            (Browser, c(K::D), A::PageDown),
            (Browser, c(K::U), A::PageUp),
            (Browser, k(K::O), A::Options),
            (Browser, k(K::OpenBracket), A::PrevTab),
            (Browser, k(K::CloseBracket), A::NextTab),
            (Browser, k(K::P), A::AddToPlaylist),
            (Browser, k(K::V), A::ToggleView),
            (Browser, k(K::M), A::FixMatch),
            (List, k(K::ArrowUp), A::Up),
            (List, k(K::ArrowDown), A::Down),
            (List, k(K::ArrowLeft), A::Left),
            (List, k(K::ArrowRight), A::Right),
            (List, k(K::PageUp), A::PageUp),
            (List, k(K::PageDown), A::PageDown),
            (List, k(K::Home), A::Home),
            (List, k(K::End), A::End),
            (List, k(K::Enter), A::Accept),
            (List, k(K::Escape), A::Back),
            (List, k(K::Backspace), A::Back),
            (List, k(K::Delete), A::Options),
            (List, k(K::Tab), A::NextTab),
            (List, s(K::Tab), A::PrevTab),
            (List, k(K::J), A::Down),
            (List, k(K::K), A::Up),
            (List, k(K::H), A::Left),
            (List, k(K::L), A::Right),
            (List, s(K::G), A::End),
            (List, c(K::D), A::PageDown),
            (List, c(K::U), A::PageUp),
            (List, k(K::Q), A::Back),
            (List, k(K::X), A::Options),
            (Player, k(K::Space), A::PlayPause),
            (Player, k(K::ArrowLeft), A::SeekBack),
            (Player, k(K::ArrowRight), A::SeekForward),
            (Player, s(K::ArrowLeft), A::SeekBackLarge),
            (Player, s(K::ArrowRight), A::SeekForwardLarge),
            (Player, k(K::ArrowUp), A::VolumeUp),
            (Player, k(K::ArrowDown), A::VolumeDown),
            (Player, k(K::M), A::Mute),
            (Player, k(K::PageUp), A::BrightnessUp),
            (Player, k(K::PageDown), A::BrightnessDown),
            (Player, k(K::C), A::NextSubtitle),
            (Player, s(K::C), A::SubtitlesOff),
            (Player, k(K::X), A::SubDelayUp),
            (Player, k(K::Z), A::SubDelayDown),
            (Player, k(K::G), A::SubSizeUp),
            (Player, s(K::G), A::SubSizeDown),
            (Player, k(K::A), A::NextAudio),
            (Player, k(K::N), A::NextChapter),
            (Player, s(K::N), A::PrevChapter),
            (Player, k(K::CloseBracket), A::SpeedUp),
            (Player, k(K::OpenBracket), A::SpeedDown),
            (Player, k(K::Equals), A::SpeedReset),
            (Player, c(K::R), A::RescueMode),
            (Player, k(K::Enter), A::ShowOsd),
            (Player, k(K::T), A::Tracks),
            (Player, k(K::Tab), A::Tracks),
            (Player, k(K::F), A::Fullscreen),
            (Player, k(K::Escape), A::Back),
            // vim motions
            (Player, k(K::H), A::SeekBack),
            (Player, k(K::L), A::SeekForward),
            (Player, s(K::H), A::SeekBackLarge),
            (Player, s(K::L), A::SeekForwardLarge),
            (Player, k(K::K), A::VolumeUp),
            (Player, k(K::J), A::VolumeDown),
            (Player, k(K::Q), A::Stop),
            (Player, k(K::Period), A::FrameStep),
            (Player, k(K::Comma), A::FrameBackStep),
            (Player, s(K::Period), A::NextItem),
            (Player, s(K::Comma), A::PrevItem),
            (Player, k(K::E), A::Episodes),
            (Player, k(K::R), A::CycleRepeat),
            (Player, k(K::Backspace), A::Stop),
        ];
        use PadInput as P;
        let pads = vec![
            (Global, P::Select, A::Fullscreen),
            (Global, P::Guide, A::Palette),
            (Browser, P::DpadUp, A::Up),
            (Browser, P::DpadDown, A::Down),
            (Browser, P::DpadLeft, A::Left),
            (Browser, P::DpadRight, A::Right),
            (Browser, P::LstickUp, A::Up),
            (Browser, P::LstickDown, A::Down),
            (Browser, P::Lb, A::PageUp),
            (Browser, P::Rb, A::PageDown),
            (Browser, P::A, A::Accept),
            (Browser, P::B, A::Back),
            (Browser, P::X, A::Options),
            (Browser, P::Y, A::Filter),
            (Browser, P::Start, A::Menu),
            (Browser, P::Lt, A::PrevTab),
            (Browser, P::Rt, A::NextTab),
            (Browser, P::Rs, A::AddToPlaylist),
            (Browser, P::Ls, A::ToggleView),
            (List, P::DpadUp, A::Up),
            (List, P::DpadDown, A::Down),
            (List, P::DpadLeft, A::Left),
            (List, P::DpadRight, A::Right),
            (List, P::LstickUp, A::Up),
            (List, P::LstickDown, A::Down),
            (List, P::LstickLeft, A::Left),
            (List, P::LstickRight, A::Right),
            (List, P::A, A::Accept),
            (List, P::B, A::Back),
            (List, P::Start, A::Back),
            (List, P::X, A::Options),
            (List, P::Y, A::Keyboard),
            (List, P::Lb, A::PrevTab),
            (List, P::Rb, A::NextTab),
            (Player, P::A, A::PlayPause),
            (Player, P::B, A::Back),
            (Player, P::X, A::NextSubtitle),
            (Player, P::Y, A::NextAudio),
            (Player, P::DpadLeft, A::SeekBack),
            (Player, P::DpadRight, A::SeekForward),
            (Player, P::LstickLeft, A::SeekBack),
            (Player, P::LstickRight, A::SeekForward),
            (Player, P::DpadUp, A::VolumeUp),
            (Player, P::DpadDown, A::VolumeDown),
            (Player, P::Lb, A::SeekBackLarge),
            (Player, P::Rb, A::SeekForwardLarge),
            (Player, P::Lt, A::PrevChapter),
            (Player, P::Rt, A::NextChapter),
            (Player, P::RstickUp, A::BrightnessUp),
            (Player, P::RstickDown, A::BrightnessDown),
            (Player, P::Rs, A::Mute),
            (Player, P::Start, A::Tracks),
            (Player, P::RstickRight, A::NextItem),
            (Player, P::RstickLeft, A::PrevItem),
            (Player, P::Ls, A::Episodes),
        ];
        Self { keys, pads }
    }

    /// Loads `bindings.toml` if present (it holds the complete set once the user changes anything).
    pub fn load() -> Self {
        let Ok(text) = std::fs::read_to_string(Self::path()) else { return Self::defaults() };
        let file: BindingsFile = match toml::from_str(&text) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("bindings.toml: {e}; using defaults");
                return Self::defaults();
            }
        };
        let mut keys = Vec::new();
        for b in file.keyboard {
            match b.key.parse::<KeyChord>() {
                Ok(k) => keys.push((b.context, k, b.action)),
                Err(e) => log::warn!("bindings.toml: {e}"),
            }
        }
        let pads = file.gamepad.into_iter().map(|b| (b.context, b.button, b.action)).collect();
        Self { keys, pads }
    }

    pub fn save(&self) {
        let file = BindingsFile {
            keyboard: self
                .keys
                .iter()
                .map(|(c, k, a)| KeyBinding { context: *c, key: k.to_string(), action: *a })
                .collect(),
            gamepad: self
                .pads
                .iter()
                .map(|(c, b, a)| PadBinding { context: *c, button: *b, action: *a })
                .collect(),
        };
        let text = match toml::to_string_pretty(&file) {
            Ok(t) => t,
            Err(e) => return log::error!("bindings: {e}"),
        };
        let path = Self::path();
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Err(e) = std::fs::write(&path, text) {
            log::error!("saving bindings: {e}");
        }
    }

    pub fn key_action(&self, ctx: Context, chord: KeyChord) -> Option<Action> {
        let find = |c: Context| self.keys.iter().find(|(bc, k, _)| *bc == c && *k == chord).map(|b| b.2);
        find(ctx).or_else(|| find(Context::Global))
    }

    pub fn pad_action(&self, ctx: Context, input: PadInput) -> Option<Action> {
        let find = |c: Context| self.pads.iter().find(|(bc, p, _)| *bc == c && *p == input).map(|b| b.2);
        find(ctx).or_else(|| find(Context::Global))
    }

    /// First key bound to `action` in `ctx` (or global), for hints.
    pub fn key_for(&self, ctx: Context, action: Action) -> Option<KeyChord> {
        let find = |c: Context| self.keys.iter().find(|(bc, _, a)| *bc == c && *a == action).map(|b| b.1);
        find(ctx).or_else(|| find(Context::Global))
    }

    pub fn pad_for(&self, ctx: Context, action: Action) -> Option<PadInput> {
        let find = |c: Context| self.pads.iter().find(|(bc, _, a)| *bc == c && *a == action).map(|b| b.1);
        find(ctx).or_else(|| find(Context::Global))
    }

    pub fn keys_for(&self, ctx: Context, action: Action) -> Vec<KeyChord> {
        self.keys.iter().filter(|(c, _, a)| *c == ctx && *a == action).map(|b| b.1).collect()
    }

    pub fn pads_for(&self, ctx: Context, action: Action) -> Vec<PadInput> {
        self.pads.iter().filter(|(c, _, a)| *c == ctx && *a == action).map(|b| b.1).collect()
    }

    /// Bind `chord` to `action` in `ctx`, replacing whatever that chord did in that context.
    /// Returns the action that lost the chord, if any.
    pub fn set_key(&mut self, ctx: Context, chord: KeyChord, action: Action) -> Option<Action> {
        let old = self.keys.iter().position(|(c, k, _)| *c == ctx && *k == chord).map(|i| self.keys.remove(i).2);
        self.keys.push((ctx, chord, action));
        old.filter(|a| *a != action)
    }

    pub fn set_pad(&mut self, ctx: Context, input: PadInput, action: Action) -> Option<Action> {
        let old = self.pads.iter().position(|(c, p, _)| *c == ctx && *p == input).map(|i| self.pads.remove(i).2);
        self.pads.push((ctx, input, action));
        old.filter(|a| *a != action)
    }

    pub fn clear_keys(&mut self, ctx: Context, action: Action) {
        self.keys.retain(|(c, _, a)| !(*c == ctx && *a == action));
    }

    pub fn clear_pads(&mut self, ctx: Context, action: Action) {
        self.pads.retain(|(c, _, a)| !(*c == ctx && *a == action));
    }
}

// ---------------------------------------------------------------- held-button repeat

/// Fires repeats for held gamepad inputs bound to repeating actions.
#[derive(Default)]
pub struct Repeater {
    held: HashMap<PadInput, Instant>,
}

pub const REPEAT_DELAY: Duration = Duration::from_millis(350);
pub const REPEAT_RATE: Duration = Duration::from_millis(70);

impl Repeater {
    pub fn press(&mut self, p: PadInput) {
        self.held.insert(p, Instant::now() + REPEAT_DELAY);
    }

    pub fn release(&mut self, p: PadInput) {
        self.held.remove(&p);
    }

    pub fn clear(&mut self) {
        self.held.clear();
    }

    /// Inputs due for a repeat now, and when to wake up next.
    pub fn due(&mut self) -> (Vec<PadInput>, Option<Duration>) {
        let now = Instant::now();
        let mut out = Vec::new();
        let mut next: Option<Instant> = None;
        for (p, t) in self.held.iter_mut() {
            if *t <= now {
                out.push(*p);
                *t = now + REPEAT_RATE;
            }
            next = Some(next.map_or(*t, |n| n.min(*t)));
        }
        (out, next.map(|n| n.saturating_duration_since(now)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_chord_round_trip() {
        for s in ["Ctrl+P", "Shift+ArrowLeft", "F11", "Ctrl+Shift+T", "Space"] {
            let c: KeyChord = s.parse().unwrap();
            let again: KeyChord = c.to_string().parse().unwrap();
            assert_eq!(c, again, "{s}");
        }
    }

    #[test]
    fn context_falls_back_to_global() {
        let b = Bindings::defaults();
        assert_eq!(b.key_action(Context::Player, KeyChord::new(egui::Key::F11)), Some(Action::Fullscreen));
        assert_eq!(b.key_action(Context::Player, KeyChord::new(egui::Key::Space)), Some(Action::PlayPause));
        assert_eq!(b.pad_action(Context::Player, PadInput::DpadUp), Some(Action::VolumeUp));
        assert_eq!(b.pad_action(Context::Browser, PadInput::DpadUp), Some(Action::Up));
        assert_eq!(b.pad_action(Context::Browser, PadInput::Y), Some(Action::Filter));
        assert_eq!(b.pad_action(Context::Browser, PadInput::Guide), Some(Action::Palette));
        // vim motions
        assert_eq!(b.key_action(Context::Browser, KeyChord::new(egui::Key::J)), Some(Action::Down));
        assert_eq!(b.key_action(Context::Player, KeyChord::new(egui::Key::L)), Some(Action::SeekForward));
        assert_eq!(b.key_action(Context::List, KeyChord::shift(egui::Key::G)), Some(Action::End));
    }

    #[test]
    fn rebinding_replaces_chord_in_context() {
        let mut b = Bindings::defaults();
        let lost = b.set_pad(Context::Player, PadInput::X, Action::Mute);
        assert_eq!(lost, Some(Action::NextSubtitle));
        assert_eq!(b.pad_action(Context::Player, PadInput::X), Some(Action::Mute));
        // Browser binding of X is untouched.
        assert_eq!(b.pad_action(Context::Browser, PadInput::X), Some(Action::Options));
    }

    #[test]
    fn bindings_file_round_trip() {
        let b = Bindings::defaults();
        let file = BindingsFile {
            keyboard: b.keys.iter().map(|(c, k, a)| KeyBinding { context: *c, key: k.to_string(), action: *a }).collect(),
            gamepad: b.pads.iter().map(|(c, p, a)| PadBinding { context: *c, button: *p, action: *a }).collect(),
        };
        let text = toml::to_string_pretty(&file).unwrap();
        let back: BindingsFile = toml::from_str(&text).unwrap();
        assert_eq!(back.keyboard.len(), b.keys.len());
        assert_eq!(back.gamepad, file.gamepad);
    }

    #[test]
    fn stick_hysteresis() {
        let mut s = AxisState::default();
        let mut out = Vec::new();
        s.update(gilrs::Axis::LeftStickY, 0.9, 0.15, &mut out);
        s.update(gilrs::Axis::LeftStickY, 0.5, 0.15, &mut out); // still held
        s.update(gilrs::Axis::LeftStickY, 0.05, 0.15, &mut out);
        assert!(matches!(out.as_slice(), [PadEvent::Down(PadInput::LstickUp), PadEvent::Up(PadInput::LstickUp)]));
    }
}
