//! Posters, titles and descriptions for shows and movies.
//!
//! Local files come first and need no network: Kodi/Jellyfin-style `tvshow.nfo`, `movie.nfo`
//! or `<name>.nfo`, and `poster.jpg` / `folder.jpg` / `<name>-poster.jpg` next to the media.
//! Online lookups (optional): TVmaze for shows (no key needed) and TMDB for movies (needs
//! `tmdb_api_key`). Requests go through the system `curl`, so jimmy carries no TLS stack.
//!
//! Only what the user looks at is looked up, one at a time on an idle-priority thread.
//! Posters are shrunk once to small PNGs in `<data>/posters/`, so drawing them is cheap.
//! Everything is cached in `<data>/metadata.json`; failed lookups retry after a few days.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EpInfo {
    pub title: String,
    pub overview: String,
    pub aired: String,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Info {
    pub title: String,
    pub year: Option<u32>,
    pub overview: String,
    /// Out of 10.
    pub rating: Option<f32>,
    pub genres: Vec<String>,
    /// Small PNG made from the poster.
    pub poster: Option<PathBuf>,
    /// Where it came from: "local", "TVmaze", "TMDB".
    pub source: String,
    /// "season x episode" → details.
    pub episodes: HashMap<String, EpInfo>,
    /// Nothing found (or offline); try again after a while.
    pub missing: bool,
    pub fetched: u64,
}

pub fn ep_key(season: u32, episode: u32) -> String {
    format!("{season}x{episode}")
}

/// What to look up.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Query {
    /// Show name and its folder (for local tvshow.nfo / poster.jpg).
    Show { name: String, dir: Option<PathBuf> },
    /// Movie title, year, and the file (for local .nfo / posters next to it).
    Movie { title: String, year: Option<u32>, file: PathBuf },
}

impl Query {
    pub fn key(&self) -> String {
        match self {
            Query::Show { name, .. } => format!("show:{}", crate::media::show_key(name)),
            Query::Movie { title, year, .. } => format!("movie:{}:{}", crate::media::show_key(title), year.unwrap_or(0)),
        }
    }
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Cache {
    items: HashMap<String, Info>,
    #[serde(skip)]
    dirty: bool,
}

impl Cache {
    pub fn path() -> PathBuf {
        crate::config::data_dir().join("metadata.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn get(&self, key: &str) -> Option<&Info> {
        self.items.get(key).filter(|i| !i.missing)
    }

    /// Known (found, or recently tried and not found).
    pub fn settled(&self, key: &str) -> bool {
        const RETRY: u64 = 3 * 24 * 3600;
        self.items.get(key).is_some_and(|i| !i.missing || crate::config::now_unix() < i.fetched + RETRY)
    }

    pub fn insert(&mut self, key: String, info: Info) {
        self.items.insert(key, info);
        self.dirty = true;
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.dirty = true;
        let _ = std::fs::remove_dir_all(posters_dir());
    }

    pub fn save_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        if let Ok(b) = serde_json::to_vec(self) {
            let p = Self::path();
            if let Some(d) = p.parent() {
                let _ = std::fs::create_dir_all(d);
            }
            let tmp = p.with_extension("tmp");
            if std::fs::write(&tmp, b).is_ok() {
                let _ = std::fs::rename(tmp, p);
            }
        }
    }
}

pub fn posters_dir() -> PathBuf {
    crate::config::data_dir().join("posters")
}

// ------------------------------------------------------------------ worker

pub struct Scraper {
    tx: Sender<(Query, Settings)>,
    rx: Receiver<(String, Info)>,
    asked: HashSet<String>,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub online: bool,
    pub tmdb_key: String,
}

impl Scraper {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, req_rx) = channel::<(Query, Settings)>();
        let (res_tx, rx) = channel();
        std::thread::Builder::new()
            .name("scraper".into())
            .spawn(move || {
                crate::library::lower_io_priority();
                while let Ok((q, s)) = req_rx.recv() {
                    let info = lookup(&q, &s);
                    if res_tx.send((q.key(), info)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn scraper thread");
        Self { tx, rx, asked: HashSet::new() }
    }

    /// Ask once per run (the cache decides whether it's needed at all).
    pub fn request(&mut self, q: Query, s: Settings) {
        if self.asked.insert(q.key()) {
            let _ = self.tx.send((q, s));
        }
    }

    pub fn forget(&mut self) {
        self.asked.clear();
    }

    pub fn poll(&self) -> Vec<(String, Info)> {
        self.rx.try_iter().collect()
    }
}

fn lookup(q: &Query, s: &Settings) -> Info {
    let key = q.key();
    let mut info = local(q, &key).unwrap_or_default();
    let needs_online = info.title.is_empty() || info.poster.is_none() || (matches!(q, Query::Show { .. }) && info.episodes.is_empty());
    if s.online && needs_online {
        let online = match q {
            Query::Show { name, .. } => tvmaze(name, &key),
            Query::Movie { title, year, .. } if !s.tmdb_key.trim().is_empty() => tmdb_movie(title, *year, s.tmdb_key.trim(), &key),
            Query::Movie { .. } => None,
        };
        if let Some(o) = online {
            info = merge(info, o);
        }
    }
    info.fetched = crate::config::now_unix();
    info.missing = info.title.is_empty() && info.poster.is_none();
    info
}

/// Local values win; online fills the gaps.
fn merge(local: Info, online: Info) -> Info {
    let pick = |a: String, b: String| if a.is_empty() { b } else { a };
    let source = if local.source.is_empty() { online.source.clone() } else { format!("{} + {}", local.source, online.source) };
    let mut episodes = online.episodes;
    episodes.extend(local.episodes);
    Info {
        title: pick(local.title, online.title),
        year: local.year.or(online.year),
        overview: pick(local.overview, online.overview),
        rating: local.rating.or(online.rating),
        genres: if local.genres.is_empty() { online.genres } else { local.genres },
        poster: local.poster.or(online.poster),
        source,
        episodes,
        ..Default::default()
    }
}

// ------------------------------------------------------------------ local files

fn local(q: &Query, key: &str) -> Option<Info> {
    let (nfos, posters): (Vec<PathBuf>, Vec<PathBuf>) = match q {
        Query::Show { dir: Some(d), .. } => (
            vec![d.join("tvshow.nfo")],
            ["poster.jpg", "poster.png", "folder.jpg", "folder.png", "cover.jpg"].iter().map(|n| d.join(n)).collect(),
        ),
        Query::Show { dir: None, .. } => return None,
        Query::Movie { file, .. } => {
            let dir = file.parent()?;
            let stem = file.file_stem()?.to_string_lossy().into_owned();
            (
                vec![dir.join(format!("{stem}.nfo")), dir.join("movie.nfo")],
                [format!("{stem}-poster.jpg"), format!("{stem}-poster.png"), format!("{stem}.jpg"), "poster.jpg".into(), "poster.png".into(), "folder.jpg".into(), "cover.jpg".into()]
                    .iter()
                    .map(|n| dir.join(n))
                    .collect(),
            )
        }
    };
    let mut info = Info::default();
    if let Some(xml) = nfos.iter().find_map(|p| std::fs::read_to_string(p).ok()) {
        info.title = tag(&xml, "title").unwrap_or_default();
        info.overview = tag(&xml, "plot").or_else(|| tag(&xml, "outline")).unwrap_or_default();
        info.year = tag(&xml, "year").and_then(|y| y.parse().ok()).or_else(|| tag(&xml, "premiered").and_then(|d| d.get(..4)?.parse().ok()));
        info.rating = tag(&xml, "rating").and_then(|r| r.parse().ok()).or_else(|| tag(&xml, "value").and_then(|r| r.parse().ok()));
        info.genres = all_tags(&xml, "genre");
        info.source = "local".into();
    }
    if let Some(src) = posters.iter().find(|p| p.is_file()) {
        info.poster = make_thumb(&std::fs::read(src).ok()?, key);
        if info.source.is_empty() {
            info.source = "local".into();
        }
    }
    (!info.source.is_empty()).then_some(info)
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&#39;", "'").replace("&apos;", "'").replace("&nbsp;", " ").replace("&amp;", "&")
}

/// Text of the first `<name>…</name>` (good enough for .nfo files).
fn tag(xml: &str, name: &str) -> Option<String> {
    all_tags(xml, name).into_iter().next()
}

fn all_tags(xml: &str, name: &str) -> Vec<String> {
    let open = format!("<{name}");
    let close = format!("</{name}>");
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(i) = rest.find(&open) {
        let after = &rest[i + open.len()..];
        // Skip <titleFoo>; allow attributes.
        if !after.starts_with('>') && !after.starts_with(' ') {
            rest = after;
            continue;
        }
        let Some(gt) = after.find('>') else { break };
        let body = &after[gt + 1..];
        let Some(end) = body.find(&close) else { break };
        let v = unescape(body[..end].trim().trim_start_matches("<![CDATA[").trim_end_matches("]]>"));
        if !v.is_empty() {
            out.push(v);
        }
        rest = &body[end..];
    }
    out
}

/// "<p>Hi <b>there</b></p>" → "Hi there".
fn strip_html(s: &str) -> String {
    let mut out = String::new();
    let mut in_tag = false;
    for c in s.chars() {
        match c {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    unescape(out.trim())
}

// ------------------------------------------------------------------ online

fn urlencode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn curl(url: &str, headers: &[String]) -> Option<Vec<u8>> {
    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-fsSL", "--max-time", "20", "--proto", "=https", "-A", concat!("jimmy/", env!("CARGO_PKG_VERSION"))]);
    for h in headers {
        cmd.args(["-H", h]);
    }
    let out = cmd.arg(url).stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null()).output().ok()?;
    if !out.status.success() {
        log::info!("metadata: {url} failed ({})", out.status);
        return None;
    }
    Some(out.stdout)
}

fn json(url: &str, headers: &[String]) -> Option<serde_json::Value> {
    serde_json::from_slice(&curl(url, headers)?).ok()
}

fn tvmaze(name: &str, key: &str) -> Option<Info> {
    let v = json(&format!("https://api.tvmaze.com/singlesearch/shows?q={}&embed=episodes", urlencode(name)), &[])?;
    let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
    let mut episodes = HashMap::new();
    for e in v["_embedded"]["episodes"].as_array().into_iter().flatten() {
        let (Some(se), Some(ep)) = (e["season"].as_u64(), e["number"].as_u64()) else { continue };
        episodes.insert(ep_key(se as u32, ep as u32), EpInfo { title: s(&e["name"]), overview: strip_html(&s(&e["summary"])), aired: s(&e["airdate"]) });
    }
    let poster = v["image"]["medium"].as_str().and_then(|u| curl(u, &[])).and_then(|b| make_thumb(&b, key));
    Some(Info {
        title: s(&v["name"]),
        year: v["premiered"].as_str().and_then(|d| d.get(..4)?.parse().ok()),
        overview: strip_html(&s(&v["summary"])),
        rating: v["rating"]["average"].as_f64().map(|r| r as f32),
        genres: v["genres"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(String::from)).collect(),
        poster,
        source: "TVmaze".into(),
        episodes,
        ..Default::default()
    })
}

fn tmdb_movie(title: &str, year: Option<u32>, api_key: &str, key: &str) -> Option<Info> {
    // A v4 "read access token" is a long JWT and goes in a header; a v3 key is a URL parameter.
    let (auth_param, headers) = if api_key.len() > 40 {
        (String::new(), vec![format!("Authorization: Bearer {api_key}")])
    } else {
        (format!("&api_key={}", urlencode(api_key)), vec![])
    };
    let year_param = year.map(|y| format!("&year={y}")).unwrap_or_default();
    let v = json(&format!("https://api.themoviedb.org/3/search/movie?query={}{year_param}{auth_param}", urlencode(title)), &headers)?;
    let m = v["results"].as_array()?.first()?.clone();
    let s = |v: &serde_json::Value| v.as_str().unwrap_or("").to_string();
    let poster = m["poster_path"].as_str().and_then(|p| curl(&format!("https://image.tmdb.org/t/p/w342{p}"), &[])).and_then(|b| make_thumb(&b, key));
    Some(Info {
        title: s(&m["title"]),
        year: m["release_date"].as_str().and_then(|d| d.get(..4)?.parse().ok()),
        overview: s(&m["overview"]),
        rating: m["vote_average"].as_f64().filter(|r| *r > 0.0).map(|r| r as f32),
        poster,
        source: "TMDB".into(),
        ..Default::default()
    })
}

// ------------------------------------------------------------------ posters

/// Height of stored poster thumbnails (about 2× what the info panel shows).
const THUMB_H: usize = 480;

/// Decodes a JPEG/PNG, shrinks it to `THUMB_H` tall and saves it as a PNG. Returns its path.
fn make_thumb(bytes: &[u8], key: &str) -> Option<PathBuf> {
    let (w, h, rgb) = decode(bytes)?;
    let (tw, th) = if h > THUMB_H { ((w * THUMB_H / h).max(1), THUMB_H) } else { (w, h) };
    let small = resize(&rgb, w, h, tw, th);
    let dir = posters_dir();
    std::fs::create_dir_all(&dir).ok()?;
    let name: String = key.chars().map(|c| if c.is_alphanumeric() { c } else { '_' }).collect();
    let path = dir.join(format!("{name}.png"));
    let file = std::io::BufWriter::new(std::fs::File::create(&path).ok()?);
    let mut enc = png::Encoder::new(file, tw as u32, th as u32);
    enc.set_color(png::ColorType::Rgb);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().ok()?.write_image_data(&small).ok()?;
    Some(path)
}

/// RGB8 pixels.
fn decode(bytes: &[u8]) -> Option<(usize, usize, Vec<u8>)> {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        let mut dec = png::Decoder::new(std::io::Cursor::new(bytes));
        dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut r = dec.read_info().ok()?;
        let mut buf = vec![0; r.output_buffer_size()?];
        let info = r.next_frame(&mut buf).ok()?;
        let (w, h) = (info.width as usize, info.height as usize);
        let ch = info.color_type.samples();
        let rgb = buf[..w * h * ch]
            .chunks(ch)
            .flat_map(|p| match ch {
                1 | 2 => [p[0], p[0], p[0]],
                _ => [p[0], p[1], p[2]],
            })
            .collect();
        return Some((w, h, rgb));
    }
    let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
    let px = dec.decode().ok()?;
    let info = dec.info()?;
    let (w, h) = (info.width as usize, info.height as usize);
    let rgb = match info.pixel_format {
        jpeg_decoder::PixelFormat::RGB24 => px,
        jpeg_decoder::PixelFormat::L8 => px.iter().flat_map(|&l| [l, l, l]).collect(),
        jpeg_decoder::PixelFormat::CMYK32 => px.chunks(4).flat_map(|c| {
            let k = c[3] as u32;
            [(c[0] as u32 * k / 255) as u8, (c[1] as u32 * k / 255) as u8, (c[2] as u32 * k / 255) as u8]
        }).collect(),
        jpeg_decoder::PixelFormat::L16 => px.chunks(2).flat_map(|c| [c[0], c[0], c[0]]).collect(),
    };
    Some((w, h, rgb))
}

/// Box-filter downscale (each output pixel averages the source pixels it covers).
fn resize(src: &[u8], w: usize, h: usize, tw: usize, th: usize) -> Vec<u8> {
    if (tw, th) == (w, h) {
        return src.to_vec();
    }
    let mut out = vec![0u8; tw * th * 3];
    for y in 0..th {
        let y0 = y * h / th;
        let y1 = ((y + 1) * h / th).max(y0 + 1).min(h);
        for x in 0..tw {
            let x0 = x * w / tw;
            let x1 = ((x + 1) * w / tw).max(x0 + 1).min(w);
            let mut acc = [0u32; 3];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = (sy * w + sx) * 3;
                    acc[0] += src[i] as u32;
                    acc[1] += src[i + 1] as u32;
                    acc[2] += src[i + 2] as u32;
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u32;
            let o = (y * tw + x) * 3;
            out[o] = (acc[0] / n) as u8;
            out[o + 1] = (acc[1] / n) as u8;
            out[o + 2] = (acc[2] / n) as u8;
        }
    }
    out
}

/// Loads a stored thumbnail for drawing.
pub fn load_thumb(path: &Path) -> Option<egui::ColorImage> {
    let (w, h, rgb) = decode(&std::fs::read(path).ok()?)?;
    Some(egui::ColorImage::from_rgb([w, h], &rgb))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nfo_tags() {
        let x = "<movie><title>Heat</title><plot><![CDATA[Cops &amp; robbers.]]></plot><year>1995</year><genre>Crime</genre><genre>Drama</genre><ratings><rating name=\"imdb\"><value>8.3</value></rating></ratings></movie>";
        assert_eq!(tag(x, "title").as_deref(), Some("Heat"));
        assert_eq!(tag(x, "plot").as_deref(), Some("Cops & robbers."));
        assert_eq!(all_tags(x, "genre"), ["Crime", "Drama"]);
        assert_eq!(tag(x, "value").as_deref(), Some("8.3"));
    }

    #[test]
    fn html_and_urls() {
        assert_eq!(strip_html("<p>Mal &amp; <b>crew</b></p>"), "Mal & crew");
        assert_eq!(urlencode("The Office (US)"), "The+Office+%28US%29");
    }

    #[test]
    fn resize_averages() {
        let src = [0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255];
        assert_eq!(resize(&src, 2, 2, 1, 1), [127, 127, 127]);
    }
}
