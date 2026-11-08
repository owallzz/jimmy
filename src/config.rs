//! User settings (`config.toml`) and playback history (`state.toml`).
//!
//! Linux: `~/.config/jimmy/` and `~/.local/share/jimmy/`.
//! macOS: `~/Library/Application Support/jimmy/` for both.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn project_dirs() -> Option<directories::ProjectDirs> {
    directories::ProjectDirs::from("", "", "jimmy")
}

pub fn config_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".jimmy"))
}

pub fn data_dir() -> PathBuf {
    project_dirs()
        .map(|d| d.data_dir().to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".jimmy"))
}

fn write_atomic(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

fn load_toml<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> T {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).unwrap_or_else(|e| {
            log::warn!("{}: {e}; using defaults", path.display());
            T::default()
        }),
        Err(_) => T::default(),
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct Config {
    pub theme: String,
    /// Folders shown in the menu. The first one opens on startup.
    pub roots: Vec<PathBuf>,
    pub start_fullscreen: bool,
    /// Multiplies every size in the UI.
    pub ui_scale: f32,
    pub show_hints: bool,
    /// How long the player controls stay up after input.
    pub osd_timeout_ms: u64,
    /// Highest volume allowed: 100 = no boost, up to 200.
    pub volume_max: u32,
    /// Drop back to 100% when a new file starts if boost was on.
    pub reset_boost_on_new_file: bool,
    pub seek_small_secs: f64,
    pub seek_large_secs: f64,
    pub resume: bool,
    /// mpv `hwdec` value.
    pub hwdec: String,
    /// Trades quality for speed on slow CPUs.
    pub rescue_mode: bool,
    pub sub_scale: f64,
    pub sub_background_box: bool,
    /// Seconds of data mpv reads ahead (bigger = fewer disk seeks).
    pub readahead_secs: u32,
    /// Extra raw mpv options, applied last.
    pub mpv_options: HashMap<String, String>,
}

impl Default for Config {
    fn default() -> Self {
        let home = directories::UserDirs::new();
        let videos = home
            .as_ref()
            .and_then(|u| u.video_dir().map(Path::to_path_buf))
            .filter(|p| p.is_dir());
        let home = home.map(|u| u.home_dir().to_path_buf());
        Self {
            theme: crate::theme::DEFAULT_THEME.into(),
            roots: videos.or(home).into_iter().collect(),
            start_fullscreen: false,
            ui_scale: 1.0,
            show_hints: true,
            osd_timeout_ms: 3000,
            volume_max: 200,
            reset_boost_on_new_file: true,
            seek_small_secs: 10.0,
            seek_large_secs: 60.0,
            resume: true,
            hwdec: "auto-safe".into(),
            rescue_mode: false,
            sub_scale: 1.0,
            sub_background_box: true,
            readahead_secs: 60,
            mpv_options: HashMap::new(),
        }
    }
}

impl Config {
    pub fn path() -> PathBuf {
        config_dir().join("config.toml")
    }

    pub fn load() -> Self {
        let mut c: Config = load_toml(&Self::path());
        c.volume_max = c.volume_max.clamp(100, 200);
        c.ui_scale = c.ui_scale.clamp(0.5, 3.0);
        c
    }

    pub fn save(&self) {
        match toml::to_string_pretty(self) {
            Ok(t) => {
                if let Err(e) = write_atomic(&Self::path(), &t) {
                    log::error!("saving config: {e}");
                }
            }
            Err(e) => log::error!("serializing config: {e}"),
        }
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct HistoryEntry {
    /// Last position in seconds.
    pub pos: f64,
    /// Known length in seconds (0 = unknown, filled after first play).
    pub duration: f64,
    pub watched: bool,
    pub last_played: u64,
    pub sub: Option<String>,
    pub aud: Option<String>,
}

impl HistoryEntry {
    pub fn resumable(&self) -> bool {
        !self.watched && self.pos > 10.0 && (self.duration <= 0.0 || self.pos < self.duration - 30.0)
    }

    pub fn progress(&self) -> f32 {
        if self.watched {
            1.0
        } else if self.duration > 0.0 {
            (self.pos / self.duration).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
#[serde(default)]
pub struct History {
    pub last_dir: Option<PathBuf>,
    pub files: HashMap<String, HistoryEntry>,
    #[serde(skip)]
    dirty: bool,
}

impl History {
    pub fn path() -> PathBuf {
        data_dir().join("state.toml")
    }

    pub fn load() -> Self {
        load_toml(&Self::path())
    }

    pub fn get(&self, path: &Path) -> Option<&HistoryEntry> {
        self.files.get(path.to_string_lossy().as_ref())
    }

    pub fn entry(&mut self, path: &Path) -> &mut HistoryEntry {
        self.dirty = true;
        self.files.entry(path.to_string_lossy().into_owned()).or_default()
    }

    pub fn set_last_dir(&mut self, dir: &Path) {
        if self.last_dir.as_deref() != Some(dir) {
            self.last_dir = Some(dir.to_path_buf());
            self.dirty = true;
        }
    }

    /// Files sorted by most recently played.
    pub fn recent(&self, only_unfinished: bool) -> Vec<(PathBuf, &HistoryEntry)> {
        let mut v: Vec<_> = self
            .files
            .iter()
            .filter(|(_, e)| e.last_played > 0 && (!only_unfinished || e.resumable()))
            .map(|(p, e)| (PathBuf::from(p), e))
            .collect();
        v.sort_by(|a, b| b.1.last_played.cmp(&a.1.last_played));
        v
    }

    pub fn save_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        match toml::to_string(self) {
            Ok(t) => {
                if let Err(e) = write_atomic(&Self::path(), &t) {
                    log::error!("saving history: {e}");
                }
            }
            Err(e) => log::error!("serializing history: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_round_trips_and_fills_defaults() {
        let c: Config = toml::from_str("theme = \"nord\"\nvolume_max = 150").unwrap();
        assert_eq!(c.theme, "nord");
        assert_eq!(c.volume_max, 150);
        assert_eq!(c.seek_small_secs, 10.0);
        let again: Config = toml::from_str(&toml::to_string(&c).unwrap()).unwrap();
        assert_eq!(again.theme, "nord");
    }

    #[test]
    fn resumable_rules() {
        let mut e = HistoryEntry { pos: 600.0, duration: 6000.0, ..Default::default() };
        assert!(e.resumable());
        e.pos = 5.0;
        assert!(!e.resumable());
        e.pos = 5990.0;
        assert!(!e.resumable());
        e.pos = 600.0;
        e.watched = true;
        assert!(!e.resumable());
    }
}
