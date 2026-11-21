//! Subtitles on demand from OpenSubtitles.org, with no account or key: its older public
//! interface (`rest.opensubtitles.org`) searches and downloads anonymously.
//!
//! Two searches: by the file's OpenSubtitles hash (subtitles made for this exact release),
//! then by IMDb ID (plus season/episode) or title. Downloads arrive gzipped; ad lines the site
//! adds to anonymous downloads are removed. Files are kept in `<data>/subtitles/<video id>/`
//! and added again whenever that video is played.

use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

const API: &str = "https://rest.opensubtitles.org/search";
/// The legacy API only answers registered user agents; this one is its public, keyless agent.
const AGENT: &str = "TemporaryUserAgent";

#[derive(Clone, Debug, Default)]
pub struct Query {
    pub path: PathBuf,
    /// Comma-separated codes, e.g. "en" or "en,es".
    pub languages: String,
    pub title: String,
    pub season: Option<u32>,
    pub episode: Option<u32>,
    /// The movie's IMDb ID, or the show's for episodes ("tt0303461").
    pub imdb: Option<String>,
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
    /// Gzipped subtitle file.
    pub link: String,
    /// "srt", "ass", "sub"…
    pub format: String,
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

/// GET with curl. Returns the body, or a readable error.
fn get(url: &str) -> Result<Vec<u8>, String> {
    let out = crate::os::command("curl")
        .args(["-sSL", "--max-time", "25", "--proto", "=https", "-A", AGENT, "-H", &format!("X-User-Agent: {AGENT}"), "-w", "\n%{http_code}", url])
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    let mut body = out.stdout;
    let nl = body.iter().rposition(|b| *b == b'\n').unwrap_or(0);
    let code: u32 = String::from_utf8_lossy(&body[nl..]).trim().parse().unwrap_or(0);
    body.truncate(nl);
    match code {
        200..=299 => Ok(body),
        0 => Err("Couldn't reach OpenSubtitles (offline?)".into()),
        407 | 429 => Err("OpenSubtitles download limit reached; try again later".into()),
        c => Err(format!("OpenSubtitles didn't answer (error {c})")),
    }
}

/// "en,es" → "eng,spa" (the legacy API wants ISO 639-2 codes).
fn languages(list: &str) -> String {
    let map = |c: &str| -> String {
        let c = c.trim().to_ascii_lowercase();
        let three = match c.as_str() {
            "en" => "eng", "es" => "spa", "fr" => "fre", "de" => "ger", "it" => "ita", "pt" => "por", "pb" => "pob",
            "nl" => "dut", "sv" => "swe", "no" | "nb" => "nor", "da" => "dan", "fi" => "fin", "pl" => "pol", "ru" => "rus",
            "uk" => "ukr", "ja" => "jpn", "zh" => "chi", "ko" => "kor", "ar" => "ara", "tr" => "tur", "el" => "ell",
            "he" => "heb", "cs" => "cze", "hu" => "hun", "ro" => "rum", "bg" => "bul", "hr" => "hrv", "sr" => "scc",
            "sk" => "slo", "sl" => "slv", "id" => "ind", "vi" => "vie", "th" => "tha", "hi" => "hin", "fa" => "per",
            other => return other.to_string(),
        };
        three.to_string()
    };
    let v: Vec<String> = list.split(',').filter(|c| !c.trim().is_empty()).map(map).collect();
    if v.is_empty() { "eng".into() } else { v.join(",") }
}

fn parse(body: &[u8]) -> Vec<Found> {
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(body) else { return vec![] };
    let s = |x: &serde_json::Value| x.as_str().unwrap_or("").to_string();
    v.as_array()
        .into_iter()
        .flatten()
        .filter_map(|d| {
            Some(Found {
                file_id: s(&d["IDSubtitleFile"]).parse().ok()?,
                language: s(&d["ISO639"]),
                release: s(&d["SubFileName"]),
                downloads: s(&d["SubDownloadsCnt"]).parse().unwrap_or(0),
                exact: d["MatchedBy"] == "moviehash",
                hearing_impaired: d["SubHearingImpaired"] == "1",
                link: d["SubDownloadLink"].as_str().filter(|l| l.starts_with("https://"))?.to_string(),
                format: s(&d["SubFormat"]),
            })
        })
        .collect()
}

pub fn search(q: &Query) -> Result<Vec<Found>, String> {
    let lang = format!("sublanguageid-{}", languages(&q.languages));
    let mut found = Vec::new();
    let mut errors = Vec::new();
    // 1. Made for this exact file (path segments must be in alphabetical order).
    if let (Some(h), Ok(size)) = (hash(&q.path), std::fs::metadata(&q.path).map(|m| m.len())) {
        match get(&format!("{API}/moviebytesize-{size}/moviehash-{h}/{lang}")) {
            Ok(b) => found.extend(parse(&b)),
            Err(e) => errors.push(e),
        }
    }
    // 2. By IMDb ID (the show's, with season/episode), else by title.
    let imdb = q.imdb.as_deref().map(|i| i.trim_start_matches("tt").to_string()).filter(|i| !i.is_empty());
    let url = match (imdb, q.season, q.episode) {
        (Some(id), Some(s), Some(e)) => format!("{API}/episode-{e}/imdbid-{id}/season-{s}/{lang}"),
        (Some(id), _, _) => format!("{API}/imdbid-{id}/{lang}"),
        (None, Some(s), Some(e)) => format!("{API}/episode-{e}/query-{}/season-{s}/{lang}", enc(&q.title.to_lowercase())),
        (None, _, _) => format!("{API}/query-{}/{lang}", enc(&q.title.to_lowercase())),
    };
    match get(&url) {
        Ok(b) => found.extend(parse(&b)),
        Err(e) => errors.push(e),
    }
    if found.is_empty() {
        if let Some(e) = errors.into_iter().next() {
            return Err(e);
        }
    }
    // One row per file; exact matches first, then the most downloaded.
    let mut seen = std::collections::HashSet::new();
    found.sort_by(|a, b| b.exact.cmp(&a.exact).then(b.downloads.cmp(&a.downloads)));
    found.retain(|f| seen.insert(f.file_id));
    Ok(found)
}

/// Drops the cues OpenSubtitles adds to anonymous downloads ("Watch … at osdb.link").
fn strip_ads(text: &str) -> String {
    let ad = |block: &str| {
        let l = block.to_lowercase();
        ["osdb.link", "opensubtitles", "watch online movies", "advertise your product"].iter().any(|w| l.contains(w))
    };
    let normalized = text.replace("\r\n", "\n");
    normalized.split("\n\n").filter(|b| !ad(b)).collect::<Vec<_>>().join("\n\n")
}

/// Downloads one subtitle into the video's cache folder. Returns the saved file.
pub fn download(video: &Path, found: &Found) -> Result<PathBuf, String> {
    let gz = get(&found.link)?;
    let mut raw = Vec::new();
    let ok = flate2::read::GzDecoder::new(&gz[..]).read_to_end(&mut raw).is_ok();
    if !ok || raw.is_empty() {
        return Err("The subtitle file was damaged; try another one".into());
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    let ext = if found.format.is_empty() { "srt".to_string() } else { found.format.to_ascii_lowercase() };
    let body = if ext == "srt" { strip_ads(&text) } else { text };
    let dir = cache_dir(video);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let lang = if found.language.is_empty() { "und".into() } else { found.language.clone() };
    let path = dir.join(format!("{lang}.{}.{ext}", found.file_id));
    std::fs::write(&path, body).map_err(|e| e.to_string())?;
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

    #[test]
    fn languages_and_ads() {
        assert_eq!(languages("en, es"), "eng,spa");
        assert_eq!(languages(""), "eng");
        let srt = "1\n00:00:06,000 --> 00:00:12,074\nWatch Online Movies and Series for FREE\nwww.osdb.link/lm\n\n2\n00:03:38,425 --> 00:03:39,968\nCheck, charge or cash?\n";
        assert_eq!(strip_ads(srt), "2\n00:03:38,425 --> 00:03:39,968\nCheck, charge or cash?\n");
    }
}
