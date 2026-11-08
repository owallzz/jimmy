//! Folder listing on a background thread.
//!
//! Built for slow disks: one `read_dir` per folder, no file probing (durations only come from
//! playback history), and the thread runs at idle I/O priority on Linux so it never competes
//! with playback.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::SystemTime;

pub const VIDEO_EXTS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "webm", "wmv", "flv", "mpg", "mpeg", "m2ts", "mts", "ts",
    "vob", "ogv", "3gp", "divx", "xvid", "rm", "rmvb", "asf", "f4v", "mxf", "dv", "y4m",
];
pub const AUDIO_EXTS: &[&str] = &["mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "wma", "mka"];
pub const SUB_EXTS: &[&str] = &["srt", "ass", "ssa", "vtt", "sub", "idx", "sup", "smi"];

fn ext_of(p: &Path) -> String {
    p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

pub fn is_media(p: &Path) -> bool {
    let e = ext_of(p);
    VIDEO_EXTS.contains(&e.as_str()) || AUDIO_EXTS.contains(&e.as_str())
}

#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub size: u64,
    pub modified: Option<SystemTime>,
    /// Sidecar subtitle files next to a video, e.g. `["en.srt", "es.srt"]`.
    pub subs: Vec<String>,
}

impl Entry {
    pub fn ext(&self) -> String {
        ext_of(&self.path).to_ascii_uppercase()
    }
}

#[derive(Clone, Debug)]
pub struct Listing {
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub error: Option<String>,
}

pub fn list_dir(dir: &Path) -> Listing {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => return Listing { dir: dir.into(), entries: vec![], error: Some(e.to_string()) },
    };
    let mut dirs = Vec::new();
    let mut media = Vec::new();
    let mut subs: Vec<String> = Vec::new();
    for de in rd.flatten() {
        let name = de.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = de.path();
        // file_type() is free on Linux (d_type); metadata() follows symlinks.
        let meta = match std::fs::metadata(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_dir() {
            dirs.push(Entry {
                name,
                path,
                is_dir: true,
                size: 0,
                modified: meta.modified().ok(),
                subs: vec![],
            });
        } else if is_media(&path) {
            media.push(Entry {
                name,
                path,
                is_dir: false,
                size: meta.len(),
                modified: meta.modified().ok(),
                subs: vec![],
            });
        } else if SUB_EXTS.contains(&ext_of(&path).as_str()) {
            subs.push(name);
        }
    }
    for m in &mut media {
        let stem = Path::new(&m.name).file_stem().map(|s| s.to_string_lossy().into_owned());
        if let Some(stem) = stem {
            let mut found: Vec<String> = subs
                .iter()
                .filter(|s| s.starts_with(&stem))
                .map(|s| {
                    let rest = s[stem.len()..].trim_start_matches(['.', ' ', '_', '-']);
                    if rest.is_empty() { s.clone() } else { rest.to_string() }
                })
                .collect();
            found.sort();
            m.subs = found;
        }
    }
    let key = |e: &Entry| natural_key(&e.name);
    dirs.sort_by_key(key);
    media.sort_by_key(key);
    dirs.extend(media);
    Listing { dir: dir.into(), entries: dirs, error: None }
}

/// Case-insensitive sort key where "Episode 2" comes before "Episode 10".
pub fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut out = Vec::new();
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut n = String::new();
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                n.push(d);
                chars.next();
            }
            out.push((0, n.parse().unwrap_or(u64::MAX), String::new()));
        } else {
            let mut t = String::new();
            while let Some(&d) = chars.peek().filter(|d| !d.is_ascii_digit()) {
                t.extend(d.to_lowercase());
                chars.next();
            }
            out.push((1, 0, t));
        }
    }
    out
}

#[cfg(target_os = "linux")]
fn lower_io_priority() {
    // ioprio_set(IOPRIO_WHO_PROCESS, 0 = this thread, IOPRIO_CLASS_IDLE << 13)
    const IOPRIO_WHO_PROCESS: libc::c_long = 1;
    const IOPRIO_CLASS_IDLE: libc::c_long = 3;
    unsafe {
        libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0 as libc::c_long, IOPRIO_CLASS_IDLE << 13);
    }
}

#[cfg(not(target_os = "linux"))]
fn lower_io_priority() {}

pub struct Scanner {
    tx: Sender<PathBuf>,
    rx: Receiver<Listing>,
}

impl Scanner {
    pub fn new(ctx: egui::Context) -> Self {
        let (req_tx, req_rx) = channel::<PathBuf>();
        let (res_tx, res_rx) = channel::<Listing>();
        std::thread::Builder::new()
            .name("scanner".into())
            .spawn(move || {
                lower_io_priority();
                while let Ok(mut dir) = req_rx.recv() {
                    // Only the newest request matters if several queued up.
                    while let Ok(newer) = req_rx.try_recv() {
                        dir = newer;
                    }
                    let listing = list_dir(&dir);
                    if res_tx.send(listing).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn scanner thread");
        Self { tx: req_tx, rx: res_rx }
    }

    pub fn request(&self, dir: &Path) {
        let _ = self.tx.send(dir.to_path_buf());
    }

    pub fn poll(&self) -> Option<Listing> {
        let mut last = None;
        while let Ok(l) = self.rx.try_recv() {
            last = Some(l);
        }
        last
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort() {
        let mut v = vec!["Ep 10", "ep 2", "Ep 1"];
        v.sort_by_key(|s| natural_key(s));
        assert_eq!(v, ["Ep 1", "ep 2", "Ep 10"]);
    }

    #[test]
    fn lists_media_dirs_and_sidecar_subs() {
        let d = std::env::temp_dir().join(format!("jimmy-test-{}", std::process::id()));
        std::fs::create_dir_all(d.join("Season 1")).unwrap();
        for f in ["Movie (2020).mkv", "Movie (2020).en.srt", "Movie (2020).es.srt", "notes.txt", ".hidden.mkv"] {
            std::fs::write(d.join(f), b"x").unwrap();
        }
        let l = list_dir(&d);
        let names: Vec<_> = l.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Season 1", "Movie (2020).mkv"]);
        assert_eq!(l.entries[1].subs, ["en.srt", "es.srt"]);
        std::fs::remove_dir_all(d).unwrap();
    }
}
