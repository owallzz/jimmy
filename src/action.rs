//! Every command in jimmy is an [`Action`]. Keyboard, gamepad, mouse and the command palette
//! all produce actions, so remapping is just editing the input → action tables.

use serde::{Deserialize, Serialize};

/// Where a binding applies. Lookups try the current context first, then `Global`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Context {
    Global,
    Browser,
    Player,
    /// Menus, panels, palette results, settings: anything that is a list.
    List,
}

impl Context {
    pub fn label(self) -> &'static str {
        match self {
            Context::Global => "global",
            Context::Browser => "browser",
            Context::Player => "player",
            Context::List => "menus",
        }
    }
}

macro_rules! actions {
    ($( $v:ident => $cat:literal, $name:literal, $repeat:literal; )*) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
        #[serde(rename_all = "snake_case")]
        pub enum Action { $($v),* }

        impl Action {
            pub const ALL: &'static [Action] = &[$(Action::$v),*];

            pub fn name(self) -> &'static str {
                match self { $(Action::$v => $name),* }
            }

            pub fn category(self) -> &'static str {
                match self { $(Action::$v => $cat),* }
            }

            /// Repeats while the key or button is held.
            pub fn repeats(self) -> bool {
                match self { $(Action::$v => $repeat),* }
            }
        }
    };
}

actions! {
    Up => "Navigate", "Up", true;
    Down => "Navigate", "Down", true;
    Left => "Navigate", "Left", true;
    Right => "Navigate", "Right", true;
    PageUp => "Navigate", "Page up", true;
    PageDown => "Navigate", "Page down", true;
    Home => "Navigate", "First item", false;
    End => "Navigate", "Last item", false;
    Accept => "Navigate", "Open / select", false;
    Back => "Navigate", "Back", false;
    Options => "Navigate", "Options menu", false;
    PlayFromStart => "Navigate", "Play from start", false;
    ToggleLibrary => "Library", "Add / remove library folder", false;
    PlayNext => "Library", "Play a show's next episode", false;
    Filter => "Navigate", "Search (filter the list)", false;
    Keyboard => "Navigate", "On-screen keyboard", false;
    PrevTab => "Navigate", "Previous tab", true;
    NextTab => "Navigate", "Next tab", true;

    Menu => "App", "Menu", false;
    Palette => "App", "Command palette", false;
    Settings => "App", "Settings", false;
    Fullscreen => "App", "Toggle fullscreen", false;
    NextTheme => "App", "Next color scheme", false;
    PrevTheme => "App", "Previous color scheme", false;
    ToggleHints => "App", "Show / hide button hints", false;
    Quit => "App", "Quit jimmy", false;

    PlayPause => "Playback", "Play / pause", false;
    SeekBack => "Playback", "Seek back (small)", true;
    SeekForward => "Playback", "Seek forward (small)", true;
    SeekBackLarge => "Playback", "Seek back (large)", true;
    SeekForwardLarge => "Playback", "Seek forward (large)", true;
    PrevChapter => "Playback", "Previous chapter", false;
    NextChapter => "Playback", "Next chapter", false;
    SpeedDown => "Playback", "Slower", false;
    SpeedUp => "Playback", "Faster", false;
    SpeedReset => "Playback", "Normal speed", false;
    RescueMode => "Playback", "Rescue mode (slow CPU)", false;
    ShowOsd => "Playback", "Show controls", false;
    Tracks => "Playback", "Tracks and subtitles", false;
    Stop => "Playback", "Stop and go back", false;
    NextItem => "Playback", "Next episode / item", false;
    PrevItem => "Playback", "Previous episode / item", false;
    Replay => "Playback", "Replay from the start", false;
    FrameStep => "Playback", "Next frame (pauses)", true;
    DownloadSubs => "Subtitles", "Download subtitles…", false;
    FrameBackStep => "Playback", "Previous frame (pauses)", true;
    CycleRepeat => "Playback", "Repeat: off / this / all", false;
    Episodes => "Playback", "Episodes and queue", false;
    AddToPlaylist => "Library", "Add to playlist", false;
    ToggleView => "Library", "Grid / list view", false;
    FixMatch => "Library", "Fix match (pick the right show or movie)", false;

    VolumeUp => "Audio", "Volume up", true;
    VolumeDown => "Audio", "Volume down", true;
    Mute => "Audio", "Mute", false;
    NextAudio => "Audio", "Next audio track", false;

    BrightnessUp => "Display", "Brightness up", true;
    BrightnessDown => "Display", "Brightness down", true;

    NextSubtitle => "Subtitles", "Next subtitle track", false;
    SubtitlesOff => "Subtitles", "Subtitles off", false;
    SubDelayUp => "Subtitles", "Subtitle delay +0.1 s", true;
    SubDelayDown => "Subtitles", "Subtitle delay −0.1 s", true;
    SubSizeUp => "Subtitles", "Subtitle size bigger", true;
    SubSizeDown => "Subtitles", "Subtitle size smaller", true;
}

impl Action {
    /// Actions worth listing in the command palette (raw navigation is left out).
    pub fn in_palette(self) -> bool {
        !matches!(
            self,
            Action::Up
                | Action::Down
                | Action::Left
                | Action::Right
                | Action::PageUp
                | Action::PageDown
                | Action::Home
                | Action::End
                | Action::Accept
                | Action::Back
                | Action::Options
                | Action::Filter
                | Action::Keyboard
                | Action::PrevTab
                | Action::NextTab
                | Action::Palette
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serde_names_are_snake_case() {
        let s = toml::to_string(&std::collections::BTreeMap::from([("a", Action::SeekBackLarge)])).unwrap();
        assert!(s.contains("seek_back_large"), "{s}");
    }
}
