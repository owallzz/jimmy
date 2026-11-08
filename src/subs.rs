//! Subtitles on demand from OpenSubtitles.com (needs a free API key: `opensubtitles_api_key`).
//!
//! Searches by the file's OpenSubtitles hash (exact release match) plus title / season /
//! episode / TMDB ID. Downloaded files are kept in `<data>/subtitles/<file id>/` and added
//! again whenever that video is played.

use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const API: &str = "https://api.opensubtitles.com/api/v1";

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub path: PathBuf,
    /// Comma-separated codes, e.g. "en" or "en,es".
    pub languages: String,
    pub title: String,
    pub year: Option<u32>,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// The movie's TMDB ID, or the show's for episodes.
    pub tmdb: Option<u64>,
    pub is_episode: bool,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Found {
    pub file_id: u64,
    pub language: String,
    pub release: String,
    pub downloads: u64,
    /// Made for this exact file (hash match).
    pub exact: bool,
    pub hearing_impaired: bool,
}

/// OpenSubtitles hash: file size + the first and last 64 KiB summed as little-endian u64s.
pub fn hash(path: &Path) -> Option<String> {
    const CHUNK: u64 = 65536;
    let mut f = std::fs::File::open(path).ok()?;
    let size = f.metadata().ok()?.len();
    if size < CHUNK * 2 {
        return None;
    }
    let mut h = size;
    let mut buf = vec![0u8; CHUNK as usize];
    for off in [0, size - CHUNK] {
        f.seek(SeekFrom::Start(off)).ok()?;
        f.read_exact(&mut buf).ok()?;
        for w in buf.chunks_exact(8) {
            h = h.wrapping_add(u64::from_le_bytes(w.try_into().ok()?));
        }
    }
    Some(format!("{h:016x}"))
}

/// Where downloaded subtitles for a video are kept (by its path, so nothing is read to find them).
pub fn cache_dir(video: &Path) -> PathBuf {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in video.to_string_lossy().bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    crate::config::data_dir().join("subtitles").join(format!("{h:016x}"))
}

/// Subtitles downloaded earlier for this video.
pub fn cached(video: &Path) -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(cache_dir(video)) else { return vec![] };
    let mut v: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_file()).collect();
    v.sort();
    v
}

fn headers(key: &str) -> Vec<String> {
    vec![format!("Api-Key: {key}"), format!("User-Agent: jimmy v{}", env!("CARGO_PKG_VERSION")), "Accept: application/json".into()]
}

fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b',' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn run(args: &[String]) -> Result<Vec<u8>, String> {
    let out = std::process::Command::new("curl")
        .args(["-sSL", "--max-time", "25", "--proto", "=https", "-w", "\n%{http_code}"])
        .args(args)
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let mut body = out.stdout;
    // Last line is the HTTP status.
    let nl = body.iter().rposition(|b| *b == b'\n').unwrap_or(0);
    let code: u32 = String::from_utf8_lossy(&body[nl..]).trim().parse().unwrap_or(0);
    body.truncate(nl);
    match code {
        200..=299 => Ok(body),
        401 | 403 => Err("OpenSubtitles refused the API key".into()),
        406 | 429 => Err(message(&body).unwrap_or_else(|| "OpenSubtitles download limit reached for today".into())),
        0 => Err("Couldn't reach OpenSubtitles (offline?)".into()),
        c => Err(message(&body).unwrap_or_else(|| format!("OpenSubtitles error {c}"))),
    }
}

fn message(body: &[u8]) -> Option<String> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    v["message"].as_str().or_else(|| v["errors"][0].as_str()).map(String::from)
}

pub fn search(q: &Query, key: &str) -> Result<Vec<Found>, String> {
    if key.trim().is_empty() {
        return Err("Downloading subtitles needs an OpenSubtitles API key (opensubtitles_api_key in config.toml)".into());
    }
    // Parameters must be lowercase and in alphabetical order (the API redirects otherwise).
    let mut params: Vec<(String, String)> = vec![("languages".into(), q.languages.to_ascii_lowercase().replace(' ', ""))];
    if let Some(h) = hash(&q.path) {
        params.push(("moviehash".into(), h));
    }
    match (q.is_episode, q.tmdb) {
        (true, Some(id)) => params.push(("parent_tmdb_id".into(), id.to_string())),
        (false, Some(id)) => params.push(("tmdb_id".into(), id.to_string())),
        _ => params.push(("query".into(), q.title.to_lowercase())),
    }
    if let Some(s) = q.season {
        params.push(("season_number".into(), s.to_string()));
    }
    if let Some(e) = q.episode {
        params.push(("episode_number".into(), e.to_string()));
    }
    if let (Some(y), false) = (q.year, q.is_episode) {
        params.push(("year".into(), y.to_string()));
    }
    params.sort();
    let qs: Vec<String> = params.iter().map(|(k, v)| format!("{k}={}", enc(v))).collect();
    let mut args: Vec<String> = headers(key).into_iter().flat_map(|h| ["-H".to_string(), h]).collect();
    args.push(format!("{API}/subtitles?{}", qs.join("&")));
    let body = run(&args)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| "OpenSubtitles sent something unexpected".to_string())?;
    let mut list: Vec<Found> = v["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            let a = &d["attributes"];
            Some(Found {
                file_id: a["files"][0]["file_id"].as_u64()?,
                language: a["language"].as_str().unwrap_or("").to_string(),
                release: a["release"].as_str().or(a["files"][0]["file_name"].as_str()).unwrap_or("").to_string(),
                downloads: a["download_count"].as_u64().unwrap_or(0),
                exact: a["moviehash_match"].as_bool().unwrap_or(false),
                hearing_impaired: a["hearing_impaired"].as_bool().unwrap_or(false),
            })
        })
        .collect();
    // Exact-file matches first, then the most downloaded.
    list.sort_by(|a, b| b.exact.cmp(&a.exact).then(b.downloads.cmp(&a.downloads)));
    Ok(list)
}

/// Downloads one subtitle into the video's cache folder. Returns the saved file.
pub fn download(video: &Path, found: &Found, key: &str) -> Result<PathBuf, String> {
    let mut args: Vec<String> = headers(key).into_iter().flat_map(|h| ["-H".to_string(), h]).collect();
    args.extend(["-H".into(), "Content-Type: application/json".into(), "-X".into(), "POST".into(), "-d".into(), format!("{{\"file_id\":{}}}", found.file_id)]);
    args.push(format!("{API}/download"));
    let body = run(&args)?;
    let v: serde_json::Value = serde_json::from_slice(&body).map_err(|_| "OpenSubtitles sent something unexpected".to_string())?;
    let link = v["link"].as_str().ok_or_else(|| message(&body).unwrap_or_else(|| "No download link".into()))?;
    let name = v["file_name"].as_str().unwrap_or("subtitle.srt");
    let ext = Path::new(name).extension().map(|e| e.to_string_lossy().into_owned()).unwrap_or_else(|| "srt".into());
    let text = run(&[link.to_string()])?;
    let dir = cache_dir(video);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let lang = if found.language.is_empty() { "und".into() } else { found.language.clone() };
    let path = dir.join(format!("{lang}.{}.{ext}", found.file_id));
    std::fs::write(&path, text).map_err(|e| e.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_reference_algorithm() {
        // 128 KiB of zeros plus 8 bytes of 0x01: size + one non-zero word in the tail.
        let p = std::env::temp_dir().join(format!("jimmy-oshash-{}", std::process::id()));
        let mut data = vec![0u8; 131072];
        data.extend_from_slice(&[1u8; 8]);
        std::fs::write(&p, &data).unwrap();
        let expect = (data.len() as u64).wrapping_add(u64::from_le_bytes([1; 8]));
        assert_eq!(hash(&p), Some(format!("{expect:016x}")));
        std::fs::remove_file(p).unwrap();
        assert_eq!(enc("the office"), "the%20office");
    }
}
