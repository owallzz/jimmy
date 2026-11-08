//! Posters, art, titles and descriptions for shows and movies, and Fix Match searches.
//!
//! Local files come first and need no network: Kodi/Jellyfin/Plex-style `tvshow.nfo`,
//! `movie.nfo` / `<name>.nfo`, `poster.jpg` / `folder.jpg`, `fanart.jpg` / `background.jpg`
//! and `season01-poster.jpg` next to the media. Online: TMDB for movies and shows (needs
//! `tmdb_api_key`); without a key, shows still get TVmaze. Folder ID hints (`{tmdb-…}`,
//! `{tvdb-…}`, `{imdb-…}`) skip the search. Requests go through the system `curl`.
//!
//! Two idle-priority threads: one matches and fetches details, the other downloads images
//! only when they're about to be shown (posters, season posters, episode stills, backdrops)
//! and shrinks them once to small PNGs in `<data>/posters/`.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct EpInfo {
    pub title: String,
    pub overview: String,
    pub aired: String,
    /// Episode still (URL or local file).
    pub still: Option<String>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SeasonInfo {
    pub name: String,
    pub overview: String,
    pub poster: Option<String>,
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
    /// Image sources: a URL or a local file. `Db::image` gives the downloaded thumbnail.
    pub poster: Option<String>,
    pub backdrop: Option<String>,
    /// Where it came from: "local", "TMDB", "TVmaze".
    pub source: String,
    pub seasons: BTreeMap<u32, SeasonInfo>,
    /// "season x episode" → details.
    pub episodes: HashMap<String, EpInfo>,
    pub fetched: u64,
}

pub fn ep_key(season: u32, episode: u32) -> String {
    format!("{season}x{episode}")
}

/// One show or movie to match.
#[derive(Clone, Debug)]
pub struct MatchReq {
    pub key: String,
    pub show: bool,
    pub title: String,
    pub year: Option<u32>,
    pub tmdb: Option<u64>,
    pub tvdb: Option<u64>,
    pub imdb: Option<String>,
    /// Show folder or movie folder (for local .nfo and artwork).
    pub folder: Option<PathBuf>,
    /// A movie's file (for `<name>.nfo` / `<name>-poster.jpg`).
    pub file: Option<PathBuf>,
}

/// A Fix Match search result.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub tmdb: u64,
    pub title: String,
    pub year: Option<u32>,
    pub overview: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImgKind {
    Poster,
    Still,
    Backdrop,
}

pub enum Job {
    Match(MatchReq),
    /// Fix Match: search by title (shows or movies).
    Search { show: bool, query: String, year: Option<u32> },
    /// Fix Match: use this TMDB result.
    Apply { req: MatchReq, tmdb: u64 },
}

pub enum Done {
    Matched { key: String, tmdb: Option<u64>, info: Info, manual: bool },
    Results { query: String, list: Vec<Candidate>, error: Option<String> },
    Image { src: String, path: Option<PathBuf> },
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub online: bool,
    pub tmdb_key: String,
}

pub fn posters_dir() -> PathBuf {
    crate::config::data_dir().join("posters")
}

// ------------------------------------------------------------------ workers

pub struct Scraper {
    jobs: Sender<(Job, Settings)>,
    images: Sender<(String, ImgKind)>,
    rx: Receiver<Done>,
    asked: HashSet<String>,
}

impl Scraper {
    pub fn new(ctx: egui::Context) -> Self {
        let (jobs, job_rx) = channel::<(Job, Settings)>();
        let (images, img_rx) = channel::<(String, ImgKind)>();
        let (tx, rx) = channel();
        let (tx2, ctx2) = (tx.clone(), ctx.clone());
        std::thread::Builder::new()
            .name("matcher".into())
            .spawn(move || {
                crate::library::lower_io_priority();
                while let Ok((job, s)) = job_rx.recv() {
                    let done = match job {
                        Job::Match(req) => {
                            let (tmdb, info) = lookup(&req, &s);
                            Done::Matched { key: req.key, tmdb, info, manual: false }
                        }
                        Job::Search { show, query, year } => {
                            let r = search(show, &query, year, &s);
                            let error = r.is_err().then(|| r.clone().err().unwrap_or_default());
                            Done::Results { query, list: r.unwrap_or_default(), error }
                        }
                        Job::Apply { req, tmdb } => {
                            let local = local(&req).unwrap_or_default();
                            let online = if req.show { tmdb_tv(tmdb, s.tmdb_key.trim()) } else { tmdb_movie(tmdb, s.tmdb_key.trim()) };
                            let info = online.map(|o| merge(local.clone(), o)).unwrap_or(local);
                            Done::Matched { key: req.key, tmdb: Some(tmdb), info: stamp(info), manual: true }
                        }
                    };
                    if tx.send(done).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("spawn matcher thread");
        std::thread::Builder::new()
            .name("images".into())
            .spawn(move || {
                crate::library::lower_io_priority();
                while let Ok((src, kind)) = img_rx.recv() {
                    let path = fetch_image(&src, kind);
                    if tx2.send(Done::Image { src, path }).is_err() {
                        break;
                    }
                    ctx2.request_repaint();
                }
            })
            .expect("spawn image thread");
        Self { jobs, images, rx, asked: HashSet::new() }
    }

    /// Match a show/movie (once per run; the database decides whether it's needed).
    pub fn request(&mut self, req: MatchReq, s: Settings) {
        if self.asked.insert(format!("m:{}", req.key)) {
            let _ = self.jobs.send((Job::Match(req), s));
        }
    }

    pub fn job(&mut self, job: Job, s: Settings) {
        let _ = self.jobs.send((job, s));
    }

    /// Download and shrink an image (once per run).
    pub fn image(&mut self, src: &str, kind: ImgKind) {
        if self.asked.insert(format!("i:{src}")) {
            let _ = self.images.send((src.to_string(), kind));
        }
    }

    pub fn forget(&mut self) {
        self.asked.clear();
    }

    pub fn poll(&self) -> Vec<Done> {
        self.rx.try_iter().collect()
    }
}

fn stamp(mut info: Info) -> Info {
    info.fetched = crate::config::now_unix();
    info
}

/// Local files, then TMDB (by ID hint, else search) or TVmaze for shows without a key.
fn lookup(req: &MatchReq, s: &Settings) -> (Option<u64>, Info) {
    let mut info = local(req).unwrap_or_default();
    let mut tmdb = None;
    let has_local_meta = !info.title.is_empty() && info.poster.is_some();
    if s.online && !has_local_meta {
        let key = s.tmdb_key.trim();
        if !key.is_empty() {
            tmdb = req.tmdb.or_else(|| find_id(req, key)).or_else(|| search(req.show, &req.title, req.year, s).ok()?.first().map(|c| c.tmdb));
            let online = tmdb.and_then(|id| if req.show { tmdb_tv(id, key) } else { tmdb_movie(id, key) });
            if let Some(o) = online {
                info = merge(info, o);
            }
        } else if req.show {
            if let Some(o) = tvmaze(&req.title, req.tvdb, req.imdb.as_deref()) {
                info = merge(info, o);
            }
        }
    }
    (tmdb, stamp(info))
}

/// Local values win; online fills the gaps.
fn merge(local: Info, online: Info) -> Info {
    let pick = |a: String, b: String| if a.is_empty() { b } else { a };
    let source = match (local.source.is_empty(), online.source.is_empty()) {
        (true, _) => online.source.clone(),
        (false, true) => local.source.clone(),
        (false, false) => format!("{} + {}", local.source, online.source),
    };
    let mut episodes = online.episodes;
    episodes.extend(local.episodes);
    let mut seasons = online.seasons;
    for (n, ls) in local.seasons {
        let e = seasons.entry(n).or_default();
        if ls.poster.is_some() {
            e.poster = ls.poster;
        }
    }
    Info {
        title: pick(local.title, online.title),
        year: local.year.or(online.year),
        overview: pick(local.overview, online.overview),
        rating: local.rating.or(online.rating),
        genres: if local.genres.is_empty() { online.genres } else { local.genres },
        poster: local.poster.or(online.poster),
        backdrop: local.backdrop.or(online.backdrop),
        source,
        seasons,
        episodes,
        fetched: 0,
    }
}

// ------------------------------------------------------------------ local files

fn first_file(dir: &Path, names: &[String]) -> Option<String> {
    names.iter().map(|n| dir.join(n)).find(|p| p.is_file()).map(|p| p.to_string_lossy().into_owned())
}

fn local(req: &MatchReq) -> Option<Info> {
    let dir = req.folder.clone().or_else(|| req.file.as_ref()?.parent().map(Path::to_path_buf))?;
    let stem = req.file.as_ref().and_then(|f| f.file_stem()).map(|s| s.to_string_lossy().into_owned());
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
    let mut nfos = Vec::new();
    let mut posters = s(&["poster.jpg", "poster.png", "folder.jpg", "folder.png", "cover.jpg", "show.jpg"]);
    let fanart = s(&["fanart.jpg", "fanart.png", "background.jpg", "backdrop.jpg", "art.jpg"]);
    if req.show {
        nfos.push(dir.join("tvshow.nfo"));
    } else {
        if let Some(st) = &stem {
            nfos.push(dir.join(format!("{st}.nfo")));
            posters.insert(0, format!("{st}-poster.jpg"));
            posters.insert(1, format!("{st}-poster.png"));
        }
        nfos.push(dir.join("movie.nfo"));
    }
    let mut info = Info::default();
    if let Some(xml) = nfos.iter().find_map(|p| std::fs::read_to_string(p).ok()) {
        info.title = tag(&xml, "title").unwrap_or_default();
        info.overview = tag(&xml, "plot").or_else(|| tag(&xml, "outline")).unwrap_or_default();
        info.year = tag(&xml, "year").and_then(|y| y.parse().ok()).or_else(|| tag(&xml, "premiered").and_then(|d| d.get(..4)?.parse().ok()));
        info.rating = tag(&xml, "rating").and_then(|r| r.parse().ok()).or_else(|| tag(&xml, "value").and_then(|r| r.parse().ok()));
        info.genres = all_tags(&xml, "genre");
        info.source = "local".into();
    }
    info.poster = first_file(&dir, &posters);
    info.backdrop = first_file(&dir, &fanart);
    if req.show {
        // season01-poster.jpg, season01.jpg, season-specials-poster.jpg, Season 1/poster.jpg
        for n in 0..=50u32 {
            let names = if n == 0 {
                s(&["season-specials-poster.jpg", "season00-poster.jpg", "season00.jpg", "Specials/poster.jpg"])
            } else {
                vec![format!("season{n:02}-poster.jpg"), format!("season{n:02}.jpg"), format!("Season {n}/poster.jpg"), format!("Season {n:02}/poster.jpg")]
            };
            if let Some(p) = first_file(&dir, &names) {
                info.seasons.entry(n).or_default().poster = Some(p);
            }
        }
    }
    if info.source.is_empty() && (info.poster.is_some() || info.backdrop.is_some() || !info.seasons.is_empty()) {
        info.source = "local".into();
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


// ------------------------------------------------------------------ TMDB

const IMG: &str = "https://image.tmdb.org/t/p/";

/// GET a TMDB v3 path. A v4 "read access token" (a long JWT) goes in a header, a v3 key in
/// the URL.
fn tmdb(path: &str, key: &str) -> Option<serde_json::Value> {
    let sep = if path.contains('?') { '&' } else { '?' };
    if key.len() > 40 {
        json(&format!("https://api.themoviedb.org/3{path}"), &[format!("Authorization: Bearer {key}")])
    } else {
        json(&format!("https://api.themoviedb.org/3{path}{sep}api_key={}", urlencode(key)), &[])
    }
}

fn img(path: &serde_json::Value, size: &str) -> Option<String> {
    path.as_str().filter(|p| !p.is_empty()).map(|p| format!("{IMG}{size}{p}"))
}

fn year_of(v: &serde_json::Value) -> Option<u32> {
    v.as_str().and_then(|d| d.get(..4)?.parse().ok())
}

fn st(v: &serde_json::Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

/// TVDB/IMDb ID hint → TMDB ID.
fn find_id(req: &MatchReq, key: &str) -> Option<u64> {
    let (id, source) = match (&req.tvdb, &req.imdb) {
        (Some(t), _) => (t.to_string(), "tvdb_id"),
        (None, Some(i)) => (i.clone(), "imdb_id"),
        _ => return None,
    };
    let v = tmdb(&format!("/find/{}?external_source={source}", urlencode(&id)), key)?;
    let list = if req.show { &v["tv_results"] } else { &v["movie_results"] };
    list.as_array()?.first()?["id"].as_u64()
}

fn search(show: bool, query: &str, year: Option<u32>, s: &Settings) -> Result<Vec<Candidate>, String> {
    let key = s.tmdb_key.trim();
    if key.is_empty() {
        return Err("Fix Match needs a TMDB key (tmdb_api_key in config.toml)".into());
    }
    let (kind, yp, title_f, date_f) = if show { ("tv", "first_air_date_year", "name", "first_air_date") } else { ("movie", "year", "title", "release_date") };
    let year_q = year.map(|y| format!("&{yp}={y}")).unwrap_or_default();
    let v = tmdb(&format!("/search/{kind}?query={}{year_q}", urlencode(query)), key).ok_or("TMDB didn't answer (offline?)")?;
    let mut list: Vec<Candidate> = v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| Some(Candidate { tmdb: r["id"].as_u64()?, title: st(&r[title_f]), year: year_of(&r[date_f]), overview: st(&r["overview"]) }))
        .collect();
    // With a year, an exact-year result beats a more popular one.
    if let Some(y) = year {
        list.sort_by_key(|c| c.year != Some(y));
    }
    Ok(list)
}

fn genres(v: &serde_json::Value) -> Vec<String> {
    v["genres"].as_array().into_iter().flatten().filter_map(|g| g["name"].as_str().map(String::from)).collect()
}

fn tmdb_movie(id: u64, key: &str) -> Option<Info> {
    let m = tmdb(&format!("/movie/{id}"), key)?;
    Some(Info {
        title: st(&m["title"]),
        year: year_of(&m["release_date"]),
        overview: st(&m["overview"]),
        rating: m["vote_average"].as_f64().filter(|r| *r > 0.0).map(|r| r as f32),
        genres: genres(&m),
        poster: img(&m["poster_path"], "w342"),
        backdrop: img(&m["backdrop_path"], "w1280"),
        source: "TMDB".into(),
        ..Default::default()
    })
}

/// Show details plus every season's episodes (20 seasons per request).
fn tmdb_tv(id: u64, key: &str) -> Option<Info> {
    let v = tmdb(&format!("/tv/{id}"), key)?;
    let mut info = Info {
        title: st(&v["name"]),
        year: year_of(&v["first_air_date"]),
        overview: st(&v["overview"]),
        rating: v["vote_average"].as_f64().filter(|r| *r > 0.0).map(|r| r as f32),
        genres: genres(&v),
        poster: img(&v["poster_path"], "w342"),
        backdrop: img(&v["backdrop_path"], "w1280"),
        source: "TMDB".into(),
        ..Default::default()
    };
    let numbers: Vec<u64> = v["seasons"].as_array().into_iter().flatten().filter_map(|s| s["season_number"].as_u64()).collect();
    for s in v["seasons"].as_array().into_iter().flatten() {
        let Some(n) = s["season_number"].as_u64() else { continue };
        info.seasons.insert(n as u32, SeasonInfo { name: st(&s["name"]), overview: st(&s["overview"]), poster: img(&s["poster_path"], "w342") });
    }
    for chunk in numbers.chunks(20) {
        let append: Vec<String> = chunk.iter().map(|n| format!("season/{n}")).collect();
        let Some(d) = tmdb(&format!("/tv/{id}?append_to_response={}", append.join(",")), key) else { continue };
        for n in chunk {
            for e in d[format!("season/{n}")]["episodes"].as_array().into_iter().flatten() {
                let Some(en) = e["episode_number"].as_u64() else { continue };
                info.episodes.insert(
                    ep_key(*n as u32, en as u32),
                    EpInfo { title: st(&e["name"]), overview: st(&e["overview"]), aired: st(&e["air_date"]), still: img(&e["still_path"], "w300") },
                );
            }
        }
    }
    Some(info)
}

// ------------------------------------------------------------------ TVmaze (shows, no key)

fn tvmaze(name: &str, tvdb: Option<u64>, imdb: Option<&str>) -> Option<Info> {
    let embed = "embed[]=episodes&embed[]=seasons&embed[]=images";
    let v = match (tvdb, imdb) {
        (Some(t), _) => json(&format!("https://api.tvmaze.com/lookup/shows?thetvdb={t}"), &[])
            .and_then(|s| s["id"].as_u64())
            .and_then(|id| json(&format!("https://api.tvmaze.com/shows/{id}?{embed}"), &[])),
        (None, Some(i)) => json(&format!("https://api.tvmaze.com/lookup/shows?imdb={}", urlencode(i)), &[])
            .and_then(|s| s["id"].as_u64())
            .and_then(|id| json(&format!("https://api.tvmaze.com/shows/{id}?{embed}"), &[])),
        _ => None,
    }
    .or_else(|| json(&format!("https://api.tvmaze.com/singlesearch/shows?q={}&{embed}", urlencode(name)), &[]))?;
    let medium = |x: &serde_json::Value| x["image"]["medium"].as_str().map(String::from);
    let mut info = Info {
        title: st(&v["name"]),
        year: year_of(&v["premiered"]),
        overview: strip_html(&st(&v["summary"])),
        rating: v["rating"]["average"].as_f64().map(|r| r as f32),
        genres: v["genres"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(String::from)).collect(),
        poster: medium(&v),
        source: "TVmaze".into(),
        ..Default::default()
    };
    for e in v["_embedded"]["episodes"].as_array().into_iter().flatten() {
        let (Some(se), Some(ep)) = (e["season"].as_u64(), e["number"].as_u64()) else { continue };
        info.episodes.insert(ep_key(se as u32, ep as u32), EpInfo { title: st(&e["name"]), overview: strip_html(&st(&e["summary"])), aired: st(&e["airdate"]), still: medium(e) });
    }
    for s in v["_embedded"]["seasons"].as_array().into_iter().flatten() {
        let Some(n) = s["number"].as_u64() else { continue };
        info.seasons.insert(n as u32, SeasonInfo { name: st(&s["name"]), overview: strip_html(&st(&s["summary"])), poster: medium(s) });
    }
    info.backdrop = v["_embedded"]["images"].as_array().into_iter().flatten().find(|i| i["type"] == "background").and_then(|i| i["resolutions"]["original"]["url"].as_str().map(String::from));
    Some(info)
}

// ------------------------------------------------------------------ images

/// Downloads (or reads) an image and saves a small PNG. Posters: 480 px tall; stills:
/// 400 px wide; backdrops: 1280 px wide.
fn fetch_image(src: &str, kind: ImgKind) -> Option<PathBuf> {
    let bytes = if src.starts_with("https://") || src.starts_with("http://") { curl(src, &[])? } else { std::fs::read(src).ok()? };
    let (w, h, rgb) = decode(&bytes)?;
    let (tw, th) = match kind {
        ImgKind::Poster if h > 480 => (w * 480 / h, 480),
        ImgKind::Still if w > 400 => (400, h * 400 / w),
        ImgKind::Backdrop if w > 1280 => (1280, h * 1280 / w),
        _ => (w, h),
    };
    let (tw, th) = (tw.max(1), th.max(1));
    let small = resize(&rgb, w, h, tw, th);
    let dir = posters_dir();
    std::fs::create_dir_all(&dir).ok()?;
    // Name by a hash of the source so the same image is never stored twice.
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in src.bytes() {
        hash = (hash ^ b as u64).wrapping_mul(0x100000001b3);
    }
    let path = dir.join(format!("{hash:016x}.png"));
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
    fn tmdb_images_and_years() {
        let v: serde_json::Value = serde_json::from_str(r#"{"poster_path":"/a.jpg","first_air_date":"2002-09-20","empty":""}"#).unwrap();
        assert_eq!(img(&v["poster_path"], "w342").as_deref(), Some("https://image.tmdb.org/t/p/w342/a.jpg"));
        assert_eq!(img(&v["empty"], "w342"), None);
        assert_eq!(year_of(&v["first_air_date"]), Some(2002));
    }

    #[test]
    fn resize_averages() {
        let src = [0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255];
        assert_eq!(resize(&src, 2, 2, 1, 1), [127, 127, 127]);
    }
}
