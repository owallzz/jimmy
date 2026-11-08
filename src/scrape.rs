//! Posters, art, titles and descriptions for shows and movies, and Fix Match searches.
//!
//! No API keys anywhere. Local files come first: Kodi/Jellyfin/Plex-style `tvshow.nfo`,
//! `movie.nfo` / `<name>.nfo`, `poster.jpg` / `folder.jpg`, `fanart.jpg` / `background.jpg`
//! and `season01-poster.jpg` next to the media. Online: shows from TVmaze; movies from
//! Wikipedia (summary and poster) and Wikidata (year, genres, IMDb ID). Folder ID hints
//! (`{tvdb-…}`, `{imdb-…}`, `{tmdb-…}`) skip the search. Requests go through the system `curl`.
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
    /// IMDb ID ("tt0113277"), for subtitle searches.
    pub imdb: Option<String>,
    /// Where it came from: "local", "TVmaze", "Wikipedia".
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
    /// "tvmaze:123" or "wikidata:Q42198".
    pub id: String,
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
    /// Fix Match: use this result (a `Candidate::id`).
    Apply { req: MatchReq, id: String },
    /// Subtitles: search OpenSubtitles for a video.
    SubSearch { query: crate::subs::Query, key: String },
    /// Subtitles: download one result.
    SubDownload { video: PathBuf, found: crate::subs::Found, key: String },
}

pub enum Done {
    Matched { key: String, id: Option<String>, info: Info, manual: bool },
    Results { query: String, list: Vec<Candidate>, error: Option<String> },
    Image { src: String, path: Option<PathBuf> },
    SubResults { video: PathBuf, result: Result<Vec<crate::subs::Found>, String> },
    SubSaved { video: PathBuf, result: Result<PathBuf, String> },
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub online: bool,
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
                            let (id, info) = lookup(&req, &s);
                            Done::Matched { key: req.key, id, info, manual: false }
                        }
                        Job::Search { show, query, year } => {
                            let r = search(show, &query, year, &s);
                            let error = r.is_err().then(|| r.clone().err().unwrap_or_default());
                            Done::Results { query, list: r.unwrap_or_default(), error }
                        }
                        Job::Apply { req, id } => {
                            let local = local(&req).unwrap_or_default();
                            let online = by_id(&id);
                            let info = online.map(|o| merge(local.clone(), o)).unwrap_or(local);
                            Done::Matched { key: req.key, id: Some(id), info: stamp(info), manual: true }
                        }
                        Job::SubSearch { query, key } => Done::SubResults { video: query.path.clone(), result: crate::subs::search(&query, &key) },
                        Job::SubDownload { video, found, key } => {
                            let result = crate::subs::download(&video, &found, &key);
                            Done::SubSaved { video, result }
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

/// Local files, then TVmaze (shows) or Wikipedia/Wikidata (movies), by ID hint or search.
fn lookup(req: &MatchReq, s: &Settings) -> (Option<String>, Info) {
    let mut info = local(req).unwrap_or_default();
    let mut id = None;
    let has_local_meta = !info.title.is_empty() && info.poster.is_some();
    if s.online && !has_local_meta {
        id = if req.show { find_show(req) } else { find_movie(req) };
        if let Some(o) = id.as_deref().and_then(by_id) {
            info = merge(info, o);
        }
    }
    (id, stamp(info))
}

/// Details for a `Candidate::id`.
fn by_id(id: &str) -> Option<Info> {
    match id.split_once(':')? {
        ("tvmaze", n) => tvmaze_show(n.parse().ok()?),
        ("wikidata", q) => wiki_movie(q),
        _ => None,
    }
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
        imdb: local.imdb.or(online.imdb),
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
    // Wikimedia asks for a User-Agent that names the app and where it lives.
    cmd.args(["-fsSL", "--max-time", "20", "--proto", "=https", "-A", concat!("jimmy/", env!("CARGO_PKG_VERSION"), " (media player; https://github.com/owallzz/jimmy)")]);
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


// ------------------------------------------------------------------ search (Fix Match)

fn st(v: &serde_json::Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn year_of(v: &serde_json::Value) -> Option<u32> {
    v.as_str().and_then(|d| d.trim_start_matches('+').get(..4)?.parse().ok())
}

/// First plausible year in a text ("1995 crime drama film by Michael Mann" → 1995).
fn year_in(s: &str) -> Option<u32> {
    s.split(|c: char| !c.is_ascii_digit()).filter(|w| w.len() == 4).filter_map(|w| w.parse().ok()).find(|y| (1880..=2100).contains(y))
}

fn search(show: bool, query: &str, year: Option<u32>, s: &Settings) -> Result<Vec<Candidate>, String> {
    if !s.online {
        return Err("Online lookups are off (Settings → Library)".into());
    }
    let mut list = if show { tvmaze_search(query) } else { wiki_search(query, year) }.ok_or("No answer (offline?)")?;
    if let Some(y) = year {
        list.sort_by_key(|c| c.year.map(|cy| cy.abs_diff(y)).unwrap_or(99));
    }
    Ok(list)
}

// ------------------------------------------------------------------ TVmaze (shows)

const TVMAZE_EMBED: &str = "embed[]=episodes&embed[]=seasons&embed[]=images";

fn tvmaze_search(query: &str) -> Option<Vec<Candidate>> {
    let v = json(&format!("https://api.tvmaze.com/search/shows?q={}", urlencode(query)), &[])?;
    Some(
        v.as_array()?
            .iter()
            .filter_map(|r| {
                let s = &r["show"];
                Some(Candidate { id: format!("tvmaze:{}", s["id"].as_u64()?), title: st(&s["name"]), year: year_of(&s["premiered"]), overview: strip_html(&st(&s["summary"])) })
            })
            .collect(),
    )
}

/// TVmaze ID for a show: by TVDB / IMDb hint (a TMDB hint goes through Wikidata), else the
/// best name match (preferring the folder's year).
fn find_show(req: &MatchReq) -> Option<String> {
    let lookup = |q: String| json(&format!("https://api.tvmaze.com/lookup/shows?{q}"), &[]).and_then(|s| s["id"].as_u64());
    let mut tvdb = req.tvdb;
    let mut imdb = req.imdb.clone();
    if let (None, None, Some(t)) = (tvdb, &imdb, req.tmdb) {
        if let Some(e) = wikidata_by_claim("P4983", &t.to_string()).and_then(|q| wikidata_entity(&q)) {
            tvdb = claim_str(&e, "P4835").and_then(|v| v.parse().ok());
            imdb = claim_str(&e, "P345");
        }
    }
    let by_hint = tvdb.and_then(|t| lookup(format!("thetvdb={t}"))).or_else(|| imdb.as_ref().and_then(|i| lookup(format!("imdb={}", urlencode(i)))));
    if let Some(id) = by_hint {
        return Some(format!("tvmaze:{id}"));
    }
    let mut list = tvmaze_search(&req.title)?;
    if let Some(y) = req.year {
        // Same score order otherwise; an exact year wins ("Doctor Who (2005)").
        if let Some(i) = list.iter().position(|c| c.year == Some(y)) {
            return Some(list.remove(i).id);
        }
    }
    list.into_iter().next().map(|c| c.id)
}

fn tvmaze_show(id: u64) -> Option<Info> {
    let v = json(&format!("https://api.tvmaze.com/shows/{id}?{TVMAZE_EMBED}"), &[])?;
    let medium = |x: &serde_json::Value| x["image"]["medium"].as_str().map(String::from);
    let mut info = Info {
        title: st(&v["name"]),
        year: year_of(&v["premiered"]),
        overview: strip_html(&st(&v["summary"])),
        rating: v["rating"]["average"].as_f64().map(|r| r as f32),
        genres: v["genres"].as_array().into_iter().flatten().filter_map(|g| g.as_str().map(String::from)).collect(),
        poster: medium(&v),
        imdb: v["externals"]["imdb"].as_str().map(String::from),
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
    info.backdrop = v["_embedded"]["images"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|i| i["type"] == "background")
        .and_then(|i| i["resolutions"]["medium"]["url"].as_str().or(i["resolutions"]["original"]["url"].as_str()).map(String::from));
    Some(info)
}

// ------------------------------------------------------------------ Wikipedia + Wikidata (movies)

/// QIDs of items with a property value, e.g. ("P345", "tt0113277") → ["Q42198"].
fn wikidata_by_claim(prop: &str, value: &str) -> Option<String> {
    let v = json(&format!("https://www.wikidata.org/w/api.php?action=query&list=search&srsearch=haswbstatement:{prop}={}&format=json", urlencode(value)), &[])?;
    v["query"]["search"].as_array()?.first()?["title"].as_str().map(String::from)
}

fn wikidata_entity(qid: &str) -> Option<serde_json::Value> {
    let v = json(&format!("https://www.wikidata.org/w/api.php?action=wbgetentities&ids={qid}&props=labels%7Cclaims%7Csitelinks%7Cdescriptions&languages=en%7Cmul&sitefilter=enwiki&format=json"), &[])?;
    Some(v["entities"][qid].clone())
}

fn claim_str(e: &serde_json::Value, prop: &str) -> Option<String> {
    e["claims"][prop].as_array()?.first()?["mainsnak"]["datavalue"]["value"].as_str().map(String::from)
}

/// Films matching a title on English Wikipedia (its search ranks far better than Wikidata's).
fn wiki_search(query: &str, year: Option<u32>) -> Option<Vec<Candidate>> {
    let q = match year {
        Some(y) => format!("{query} {y} film"),
        None => format!("{query} film"),
    };
    let v = json(&format!("https://en.wikipedia.org/w/api.php?action=query&generator=search&gsrsearch={}&gsrlimit=10&prop=pageprops%7Cdescription&ppprop=wikibase_item&format=json&formatversion=2", urlencode(&q)), &[])?;
    let mut pages: Vec<&serde_json::Value> = v["query"]["pages"].as_array()?.iter().collect();
    pages.sort_by_key(|p| p["index"].as_u64().unwrap_or(99));
    Some(
        pages
            .into_iter()
            .filter_map(|p| {
                let desc = st(&p["description"]);
                // Films only ("1995 crime drama film by …"), not soundtracks or novels.
                if !desc.to_lowercase().contains("film") || desc.to_lowercase().contains("soundtrack") {
                    return None;
                }
                let qid = p["pageprops"]["wikibase_item"].as_str()?;
                let title = st(&p["title"]);
                let title = match title.rfind(" (") {
                    Some(i) if title.ends_with(')') && title[i..].contains("film") => title[..i].to_string(),
                    _ => title,
                };
                Some(Candidate { id: format!("wikidata:{qid}"), title, year: year_in(&desc), overview: desc })
            })
            .collect(),
    )
}

/// Wikidata ID for a movie: by IMDb / TMDB hint, else the best Wikipedia search result.
fn find_movie(req: &MatchReq) -> Option<String> {
    let hinted = req.imdb.as_ref().and_then(|i| wikidata_by_claim("P345", i)).or_else(|| req.tmdb.and_then(|t| wikidata_by_claim("P4947", &t.to_string())));
    if let Some(q) = hinted {
        return Some(format!("wikidata:{q}"));
    }
    let list = wiki_search(&req.title, req.year)?;
    let best = match req.year {
        Some(y) => list.iter().find(|c| c.year == Some(y)).or_else(|| list.iter().find(|c| c.year.is_some_and(|cy| cy.abs_diff(y) <= 1))),
        None => list.first(),
    };
    best.map(|c| c.id.clone())
}

fn wiki_movie(qid: &str) -> Option<Info> {
    let e = wikidata_entity(qid)?;
    let year = e["claims"]["P577"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|c| year_of(&c["mainsnak"]["datavalue"]["value"]["time"]))
        .min();
    // Genre labels ("crime drama film" → "Crime drama").
    let genre_ids: Vec<String> = e["claims"]["P136"].as_array().into_iter().flatten().filter_map(|c| c["mainsnak"]["datavalue"]["value"]["id"].as_str().map(String::from)).take(4).collect();
    let mut genres = Vec::new();
    if !genre_ids.is_empty() {
        if let Some(g) = json(&format!("https://www.wikidata.org/w/api.php?action=wbgetentities&ids={}&props=labels&languages=en%7Cmul&format=json", genre_ids.join("%7C")), &[]) {
            for id in &genre_ids {
                let gl = &g["entities"][id]["labels"];
                if let Some(l) = gl["en"]["value"].as_str().or(gl["mul"]["value"].as_str()) {
                    let l = l.trim_end_matches(" film").trim();
                    let mut c = l.chars();
                    genres.push(c.next().map(|f| f.to_uppercase().collect::<String>() + c.as_str()).unwrap_or_default());
                }
            }
        }
    }
    // Names are in English, or (newer items) the language-neutral "mul" label.
    let label = |v: &serde_json::Value| v["labels"]["en"]["value"].as_str().or(v["labels"]["mul"]["value"].as_str()).unwrap_or("").to_string();
    let mut info = Info {
        title: label(&e),
        year,
        genres,
        imdb: claim_str(&e, "P345"),
        source: "Wikipedia".into(),
        ..Default::default()
    };
    // Summary paragraph and poster from the English article.
    if let Some(page) = e["sitelinks"]["enwiki"]["title"].as_str() {
        let page = page.replace(' ', "_");
        if let Some(sum) = json(&format!("https://en.wikipedia.org/api/rest_v1/page/summary/{}", urlencode(&page).replace('+', "_")), &[]) {
            info.overview = st(&sum["extract"]);
            if info.title.is_empty() {
                // "Alien (film)" → "Alien"
                let t = st(&sum["title"]);
                info.title = match t.rfind(" (") {
                    Some(i) if t.ends_with(')') => t[..i].to_string(),
                    _ => t,
                };
            }
            info.poster = sum["originalimage"]["source"].as_str().or(sum["thumbnail"]["source"].as_str()).map(String::from);
        }
    }
    if info.overview.is_empty() {
        info.overview = st(&e["descriptions"]["en"]["value"]);
    }
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
    fn years() {
        let v: serde_json::Value = serde_json::from_str(r#"{"a":"2002-09-20","b":"+1995-12-15T00:00:00Z"}"#).unwrap();
        assert_eq!(year_of(&v["a"]), Some(2002));
        assert_eq!(year_of(&v["b"]), Some(1995));
        assert_eq!(year_in("1995 crime drama film by Michael Mann"), Some(1995));
    }

    #[test]
    fn resize_averages() {
        let src = [0, 0, 0, 255, 255, 255, 0, 0, 0, 255, 255, 255];
        assert_eq!(resize(&src, 2, 2, 1, 1), [127, 127, 127]);
    }
}
