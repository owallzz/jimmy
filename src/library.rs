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

pub fn is_audio(p: &Path) -> bool {
    AUDIO_EXTS.contains(&ext_of(p).as_str())
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
    /// True for the "My media" listing (every root, recursively).
    pub all: bool,
    pub dir: PathBuf,
    pub entries: Vec<Entry>,
    pub error: Option<String>,
}

pub fn list_dir(dir: &Path) -> Listing {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(e) => return Listing { all: false, dir: dir.into(), entries: vec![], error: Some(e.to_string()) },
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
    Listing { all: false, dir: dir.into(), entries: dirs, error: None }
}

/// Every media file under the given folders (for "My media"). One `read_dir` per folder,
/// still no file probing. Stops descending after `MAX_DEPTH` levels.
pub fn list_all(roots: &[PathBuf]) -> Listing {
    const MAX_DEPTH: usize = 8;
    let mut files: Vec<Entry> = Vec::new();
    let mut stack: Vec<(PathBuf, usize)> = roots.iter().map(|r| (r.clone(), 0)).collect();
    let mut seen = std::collections::HashSet::new();
    let mut errors = Vec::new();
    while let Some((dir, depth)) = stack.pop() {
        // Avoid symlink loops and roots nested inside other roots.
        let key = dir.canonicalize().unwrap_or_else(|_| dir.clone());
        if !seen.insert(key) {
            continue;
        }
        let l = list_dir(&dir);
        if let Some(e) = l.error {
            if depth == 0 {
                errors.push(format!("{}: {e}", dir.display()));
            }
            continue;
        }
        for e in l.entries {
            if e.is_dir {
                if depth < MAX_DEPTH {
                    stack.push((e.path, depth + 1));
                }
            } else {
                files.push(e);
            }
        }
    }
    files.sort_by_cached_key(|e| natural_key(&e.name));
    // A missing library folder (unplugged drive, deleted folder) only matters if nothing was found.
    let error = if files.is_empty() { errors.first().cloned() } else { None };
    Listing { all: true, dir: PathBuf::new(), entries: files, error }
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
pub fn lower_io_priority() {
    // ioprio_set(IOPRIO_WHO_PROCESS, 0 = this thread, IOPRIO_CLASS_IDLE << 13)
    const IOPRIO_WHO_PROCESS: libc::c_long = 1;
    const IOPRIO_CLASS_IDLE: libc::c_long = 3;
    unsafe {
        libc::syscall(libc::SYS_ioprio_set, IOPRIO_WHO_PROCESS, 0 as libc::c_long, IOPRIO_CLASS_IDLE << 13);
    }
}

#[cfg(not(target_os = "linux"))]
pub fn lower_io_priority() {}

#[derive(Clone, Debug)]
enum Request {
    Dir(PathBuf),
    All(Vec<PathBuf>),
}

pub struct Scanner {
    tx: Sender<Request>,
    rx: Receiver<Listing>,
}

impl Scanner {
    pub fn new(ctx: egui::Context) -> Self {
        let (req_tx, req_rx) = channel::<Request>();
        let (res_tx, res_rx) = channel::<Listing>();
        std::thread::Builder::new()
            .name("scanner".into())
            .spawn(move || {
                lower_io_priority();
                while let Ok(mut req) = req_rx.recv() {
                    // Only the newest request matters if several queued up.
                    while let Ok(newer) = req_rx.try_recv() {
                        req = newer;
                    }
                    let listing = match &req {
                        Request::Dir(d) => list_dir(d),
                        Request::All(roots) => list_all(roots),
                    };
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
        let _ = self.tx.send(Request::Dir(dir.to_path_buf()));
    }

    pub fn request_all(&self, roots: &[PathBuf]) {
        let _ = self.tx.send(Request::All(roots.to_vec()));
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
        std::fs::write(d.join("Season 1/Ep 2.mkv"), b"x").unwrap();
        std::fs::write(d.join("Season 1/Ep 10.mkv"), b"x").unwrap();
        // Listing a root twice (or nested roots) must not duplicate files.
        let all = list_all(&[d.clone(), d.join("Season 1")]);
        let names: Vec<_> = all.entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["Ep 2.mkv", "Ep 10.mkv", "Movie (2020).mkv"]);
        std::fs::remove_dir_all(d).unwrap();
    }
}
