//! File metadata (duration, codecs, tracks), read on demand in the background.
//!
//! A second, hidden libmpv instance only opens a file's headers: no video/audio output, no
//! decoding, no read-ahead. It runs at idle I/O priority and is only asked about library files
//! the user lingers on, so slow disks stay quiet. Results are cached in `meta.toml`.

use libmpv2::Mpv;
use libmpv2::events::Event;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    /// Seconds (0 = unknown).
    pub duration: f64,
    /// e.g. "H.264 720p".
    pub video: Option<String>,
    pub audio_tracks: u32,
    /// Audio language codes, e.g. ["eng", "spa"].
    pub audio_langs: Vec<String>,
    pub sub_tracks: u32,
    /// File size and mtime when probed; a mismatch means the file changed.
    pub size: u64,
    pub mtime: u64,
}

fn mtime_secs(m: &std::fs::Metadata) -> u64 {
    m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0)
}

pub fn stamp(size: u64, modified: Option<SystemTime>) -> (u64, u64) {
    (size, modified.and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map(|d| d.as_secs()).unwrap_or(0))
}

fn codec_name(c: &str) -> String {
    match c {
        "h264" => "H.264",
        "hevc" => "HEVC",
        "av1" => "AV1",
        "vp9" => "VP9",
        "vp8" => "VP8",
        "mpeg2video" => "MPEG-2",
        "mpeg4" => "MPEG-4",
        "msmpeg4v3" => "DivX 3",
        "vc1" => "VC-1",
        "wmv3" => "WMV",
        "theora" => "Theora",
        other => return other.to_ascii_uppercase(),
    }
    .to_string()
}

fn resolution(h: i64) -> String {
    match h {
        h if h >= 2000 => "4K".into(),
        h if h > 0 => format!("{h}p"),
        _ => String::new(),
    }
}

// ------------------------------------------------------------------ cache

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MetaCache {
    files: HashMap<String, Meta>,
    #[serde(skip)]
    dirty: bool,
}

impl MetaCache {
    pub fn path() -> PathBuf {
        crate::config::data_dir().join("meta.toml")
    }

    pub fn load() -> Self {
        std::fs::read_to_string(Self::path()).ok().and_then(|t| toml::from_str(&t).ok()).unwrap_or_default()
    }

    /// Cached metadata, only if the file hasn't changed since it was probed.
    pub fn get(&self, path: &Path, size: u64, modified: Option<SystemTime>) -> Option<&Meta> {
        let (s, m) = stamp(size, modified);
        self.files.get(path.to_string_lossy().as_ref()).filter(|e| e.size == s && e.mtime == m)
    }

    pub fn insert(&mut self, path: &Path, meta: Meta) {
        self.files.insert(path.to_string_lossy().into_owned(), meta);
        self.dirty = true;
    }

    pub fn save_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let path = Self::path();
        if let Some(d) = path.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        match toml::to_string(self) {
            Ok(t) => {
                let tmp = path.with_extension("tmp");
                if std::fs::write(&tmp, t).is_ok() {
                    let _ = std::fs::rename(tmp, path);
                }
            }
            Err(e) => log::error!("saving metadata cache: {e}"),
        }
    }
}

// ------------------------------------------------------------------ prober

pub enum ProbeResult {
    Ok(PathBuf, Meta),
    Failed(PathBuf),
}

pub struct Prober {
    tx: Sender<PathBuf>,
    rx: Receiver<ProbeResult>,
}

impl Prober {
    pub fn new(ctx: egui::Context) -> Self {
        let (req_tx, req_rx) = channel::<PathBuf>();
        let (res_tx, res_rx) = channel::<ProbeResult>();
        let _ = std::thread::Builder::new().name("probe".into()).spawn(move || {
            crate::library::lower_io_priority();
            // The hidden mpv is created on first use, so idle jimmy doesn't pay for it.
            let mut mpv: Option<Mpv> = None;
            loop {
                // Free the hidden mpv after a quiet spell; it's recreated on the next request.
                let mut path = match req_rx.recv_timeout(Duration::from_secs(15)) {
                    Ok(p) => p,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if mpv.take().is_some() {
                            crate::mem::trim();
                        }
                        continue;
                    }
                    Err(_) => break,
                };
                if mpv.is_none() {
                    match probe_mpv() {
                        Ok(m) => mpv = Some(m),
                        Err(e) => return log::warn!("metadata probing unavailable: {e}"),
                    }
                }
                let Some(mpv) = mpv.as_ref() else { break };
                // Only the newest request matters (the user moved on).
                while let Ok(newer) = req_rx.try_recv() {
                    path = newer;
                }
                log::debug!("probing {}", path.display());
                let started = Instant::now();
                let res = match probe(mpv, &path) {
                    Some(m) => ProbeResult::Ok(path, m),
                    None => ProbeResult::Failed(path),
                };
                log::debug!("probe done in {:?}: {}", started.elapsed(), matches!(res, ProbeResult::Ok(..)));
                if res_tx.send(res).is_err() {
                    break;
                }
                ctx.request_repaint();
            }
        });
        Self { tx: req_tx, rx: res_rx }
    }

    pub fn request(&self, path: &Path) {
        let _ = self.tx.send(path.to_path_buf());
    }

    pub fn poll(&self) -> Vec<ProbeResult> {
        self.rx.try_iter().collect()
    }
}

fn probe_mpv() -> anyhow::Result<Mpv> {
    Mpv::with_initializer(|init| {
        for (k, v) in [
            ("vo", "null"),
            ("ao", "null"),
            // Tracks stay selectable (mpv refuses files with nothing to play), but output goes
            // nowhere and playback stays paused, so only headers and at most a frame are read.
            ("sid", "no"),
            ("hwdec", "no"),
            ("config", "no"),
            ("load-scripts", "no"),
            ("load-stats-overlay", "no"),
            ("load-console", "no"),
            ("load-osd-console", "no"),
            ("load-select", "no"),
            ("load-positioning", "no"),
            ("load-context-menu", "no"),
            ("load-commands", "no"),
            ("load-auto-profiles", "no"),
            ("ytdl", "no"),
            ("terminal", "no"),
            ("idle", "yes"),
            ("pause", "yes"),
            ("audio-display", "no"),
            ("sub-auto", "no"),
            ("audio-file-auto", "no"),
            ("cache", "no"),
            ("demuxer-readahead-secs", "0"),
            ("demuxer-max-bytes", "4MiB"),
            ("hr-seek", "no"),
        ] {
            let _ = init.set_option(k, v);
        }
        Ok(())
    })
    .map_err(|e| anyhow::anyhow!("{e}"))
}

fn probe(mpv: &Mpv, path: &Path) -> Option<Meta> {
    let fs = std::fs::metadata(path).ok()?;
    // Drop anything left over from the previous file.
    while mpv.wait_event(0.0).is_some() {}
    mpv.command("loadfile", &[&path.to_string_lossy()]).ok()?;
    let deadline = Instant::now() + Duration::from_secs(8);
    let mut loaded = false;
    while Instant::now() < deadline {
        match mpv.wait_event(0.25) {
            Some(Ok(Event::FileLoaded)) => {
                loaded = true;
                break;
            }
            Some(Ok(Event::EndFile(r))) => {
                log::debug!("probe end-file {r}");
                break;
            }
            Some(Err(e)) => log::debug!("probe event error: {e}"),
            _ => {}
        }
    }
    let meta = loaded.then(|| {
        let n: i64 = mpv.get_property("track-list/count").unwrap_or(0);
        let mut m = Meta {
            duration: mpv.get_property("duration").unwrap_or(0.0),
            size: fs.len(),
            mtime: mtime_secs(&fs),
            ..Default::default()
        };
        for i in 0..n {
            let s = |k: &str| mpv.get_property::<String>(&format!("track-list/{i}/{k}")).ok();
            match s("type").as_deref() {
                Some("video") if m.video.is_none() && !mpv.get_property::<bool>(&format!("track-list/{i}/image")).unwrap_or(false) => {
                    let codec = codec_name(&s("codec").unwrap_or_default());
                    let h: i64 = mpv.get_property(&format!("track-list/{i}/demux-h")).unwrap_or(0);
                    let res = resolution(h);
                    m.video = Some(if res.is_empty() { codec } else { format!("{codec} {res}") });
                }
                Some("audio") => {
                    m.audio_tracks += 1;
                    if let Some(l) = s("lang") {
                        if !m.audio_langs.contains(&l) {
                            m.audio_langs.push(l);
                        }
                    }
                }
                Some("sub") => m.sub_tracks += 1,
                _ => {}
            }
        }
        m
    });
    let _ = mpv.command("stop", &[]);
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_ignores_changed_files() {
        let mut c = MetaCache::default();
        let t = UNIX_EPOCH + Duration::from_secs(1000);
        c.insert(Path::new("/m/a.mkv"), Meta { duration: 60.0, size: 10, mtime: 1000, ..Default::default() });
        assert!(c.get(Path::new("/m/a.mkv"), 10, Some(t)).is_some());
        assert!(c.get(Path::new("/m/a.mkv"), 11, Some(t)).is_none(), "size changed");
        assert!(c.get(Path::new("/m/a.mkv"), 10, Some(t + Duration::from_secs(5))).is_none(), "mtime changed");
    }

    #[test]
    fn cache_serializes() {
        let mut c = MetaCache::default();
        c.insert(Path::new("/m/a b.mkv"), Meta { duration: 30.0, video: Some("H.264 720p".into()), audio_tracks: 1, audio_langs: vec!["eng".into()], sub_tracks: 0, size: 1, mtime: 2 });
        c.insert(Path::new("/m/c.mp4"), Meta { duration: f64::NAN, ..Default::default() });
        let t = toml::to_string(&c).expect("serialize");
        let back: MetaCache = toml::from_str(&t).expect("parse");
        assert_eq!(back.files.len(), 2);
    }

    #[test]
    fn names() {
        assert_eq!(codec_name("h264"), "H.264");
        assert_eq!(resolution(720), "720p");
        assert_eq!(resolution(2160), "4K");
    }
}
