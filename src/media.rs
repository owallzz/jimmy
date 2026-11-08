//! Movies vs. TV: works out shows, seasons and episodes from file and folder names alone
//! (no probing, no network), so it's instant even on slow disks.
//!
//! Recognised: `Show.S01E02.Title.mkv`, `Show - 1x02 - Title.mkv`, `S01E01E02` (double
//! episodes), and `Show/Season 1/03 - Title.mkv` or `.../Episode 3.mkv` folder layouts.

use crate::library::{Entry, natural_key};
use std::collections::HashMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Episode {
    pub show: String,
    pub season: u32,
    pub episode: u32,
    /// Last episode of a multi-episode file (S01E01E02 → 2).
    pub episode_end: Option<u32>,
    /// From the file name, if it has one ("Pilot").
    pub title: Option<String>,
}

impl Episode {
    /// "S01E02" or "S01E01–02".
    pub fn code(&self) -> String {
        match self.episode_end {
            Some(e) => format!("S{:02}E{:02}–{:02}", self.season, self.episode, e),
            None => format!("S{:02}E{:02}", self.season, self.episode),
        }
    }
}

/// Words that mark the end of a title in scene-style names.
const TAGS: &[&str] = &[
    "2160p", "1080p", "1080i", "720p", "576p", "480p", "4k", "uhd", "bluray", "blu-ray", "bdrip", "brrip", "bdremux",
    "remux", "webrip", "web-dl", "webdl", "web", "hdtv", "pdtv", "dvdrip", "dvd", "dvdscr", "hdrip", "x264", "x265",
    "h264", "h265", "h.264", "h.265", "hevc", "avc", "xvid", "divx", "aac", "aac2", "ac3", "eac3", "dd5", "ddp5", "dts",
    "truehd", "atmos", "flac", "mp3", "10bit", "8bit", "hdr", "hdr10", "dv", "sdr", "proper", "repack", "extended",
    "unrated", "internal", "limited", "multi", "subbed", "dubbed", "nf", "amzn", "dsnp", "hmax", "hulu", "atvp",
];

fn is_tag(w: &str) -> bool {
    let l = w.to_ascii_lowercase();
    TAGS.contains(&l.as_str()) || (l.ends_with("ch") && l[..l.len() - 2].chars().all(|c| c.is_ascii_digit() || c == '.'))
}

/// "The.Office.US" → "The Office US"; also drops `[group]` tags and trailing separators.
pub fn clean(s: &str) -> String {
    let mut out = String::new();
    let mut depth = 0;
    for c in s.chars() {
        match c {
            '[' | '{' => depth += 1,
            ']' | '}' => depth = (depth - 1).max(0),
            _ if depth > 0 => {}
            '.' | '_' => out.push(' '),
            _ => out.push(c),
        }
    }
    // Cut at the first quality/source tag.
    let words: Vec<&str> = out.split_whitespace().collect();
    let end = words.iter().position(|w| is_tag(w.trim_matches(|c: char| !c.is_alphanumeric()))).unwrap_or(words.len());
    let s = words[..end].join(" ");
    let mut s = s.trim_matches(|c: char| c == '-' || c == ' ' || c == '–').to_string();
    // Drop a dangling bracket left by the cut ("Alien (" from "Alien (1979)"), keep balanced ones.
    while s.matches('(').count() != s.matches(')').count() {
        if s.ends_with('(') || s.ends_with(')') {
            s.pop();
        } else if s.starts_with('(') || s.starts_with(')') {
            s.remove(0);
        } else {
            break;
        }
        s = s.trim_matches(|c: char| c == '-' || c == ' ' || c == '–').to_string();
    }
    s
}

fn year_at(w: &str) -> Option<u32> {
    let w = w.trim_matches(|c: char| !c.is_ascii_digit());
    let y: u32 = (w.len() == 4).then(|| w.parse().ok()).flatten()?;
    (1900..=2099).contains(&y).then_some(y)
}

/// Title and year from a movie file name: `Blade.Runner.1982.1080p.mkv` → ("Blade Runner", 1982).
pub fn movie_title(file_name: &str) -> (String, Option<u32>) {
    let stem = Path::new(file_name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    let spaced: String = stem.chars().map(|c| if c == '.' || c == '_' { ' ' } else { c }).collect();
    let words: Vec<&str> = spaced.split_whitespace().collect();
    // The last plausible year that isn't the first word ("2001 A Space Odyssey 1968").
    if let Some(i) = (1..words.len()).rev().find(|&i| year_at(words[i]).is_some()) {
        let title = clean(&words[..i].join(" "));
        if !title.is_empty() {
            return (title, year_at(words[i]));
        }
    }
    (clean(&stem), None)
}

/// Finds `S01E02` (optionally `E03` after it) in `s`: (start, end, season, episode, end_episode).
fn find_sxxexx(s: &str) -> Option<(usize, usize, u32, u32, Option<u32>)> {
    let b = s.as_bytes();
    let digits = |from: usize, max: usize| -> (usize, Option<u32>) {
        let mut j = from;
        while j < b.len() && j - from < max && b[j].is_ascii_digit() {
            j += 1;
        }
        (j, (j > from).then(|| s[from..j].parse().ok()).flatten())
    };
    for i in 0..b.len() {
        if !(b[i] == b'S' || b[i] == b's') || (i > 0 && b[i - 1].is_ascii_alphanumeric()) {
            continue;
        }
        let (j, Some(season)) = digits(i + 1, 2) else { continue };
        let mut k = j;
        if k < b.len() && matches!(b[k], b' ' | b'.' | b'_' | b'-') {
            k += 1;
        }
        if k >= b.len() || !(b[k] == b'E' || b[k] == b'e') {
            continue;
        }
        let (mut end, Some(ep)) = digits(k + 1, 3) else { continue };
        let mut ep_end = None;
        let mut m = end;
        if m < b.len() && b[m] == b'-' {
            m += 1;
        }
        if m < b.len() && (b[m] == b'E' || b[m] == b'e') {
            if let (e2_end, Some(e2)) = digits(m + 1, 3) {
                if e2 > ep {
                    ep_end = Some(e2);
                    end = e2_end;
                }
            }
        }
        return Some((i, end, season, ep, ep_end));
    }
    None
}

/// Finds `1x02` in `s`.
fn find_nxnn(s: &str) -> Option<(usize, usize, u32, u32)> {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if !(b[i] == b'x' || b[i] == b'X') || i == 0 {
            continue;
        }
        let mut a = i;
        while a > 0 && b[a - 1].is_ascii_digit() && i - a < 2 {
            a -= 1;
        }
        let mut e = i + 1;
        while e < b.len() && b[e].is_ascii_digit() && e - i <= 3 {
            e += 1;
        }
        let before_ok = a == 0 || !b[a - 1].is_ascii_alphanumeric();
        let after_ok = e == b.len() || !b[e].is_ascii_alphanumeric();
        if a < i && e - i >= 3 && before_ok && after_ok {
            return Some((a, e, s[a..i].parse().ok()?, s[i + 1..e].parse().ok()?));
        }
    }
    None
}

/// "Season 2", "S02", "Series 2" → 2.
fn season_folder(name: &str) -> Option<u32> {
    let l = name.to_ascii_lowercase();
    let rest = l.strip_prefix("season").or_else(|| l.strip_prefix("series")).or_else(|| l.strip_prefix('s'))?;
    let rest = rest.trim_matches(|c: char| c == ' ' || c == '.' || c == '_' || c == '-');
    (!rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())).then(|| rest.parse().ok()).flatten()
}

/// The episode a file is, if it looks like one.
pub fn episode(path: &Path) -> Option<Episode> {
    let stem = path.file_stem()?.to_string_lossy().into_owned();
    let parent = path.parent();
    let parent_name = parent.and_then(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let in_season = season_folder(&parent_name);
    // Show folder: the parent, or the grandparent when the parent is "Season N".
    let show_folder = || {
        let dir = if in_season.is_some() { parent.and_then(|p| p.parent()) } else { parent };
        dir.and_then(|d| d.file_name()).map(|n| clean(&n.to_string_lossy())).filter(|s| !s.is_empty())
    };
    let title_after = |s: &str| Some(clean(s)).filter(|t| !t.is_empty() && !t.chars().all(|c| c.is_ascii_digit()));

    let found = find_sxxexx(&stem).map(|(a, b, s, e, e2)| (a, b, s, e, e2)).or_else(|| find_nxnn(&stem).map(|(a, b, s, e)| (a, b, s, e, None)));
    if let Some((a, b, season, ep, ep_end)) = found {
        let before = clean(&stem[..a]);
        let show = if before.is_empty() { show_folder()? } else { before };
        return Some(Episode { show, season, episode: ep, episode_end: ep_end, title: title_after(&stem[b..]) });
    }

    // Inside a "Season N" folder: "03 - Title", "E03", "Episode 3", "Ep 03".
    let season = in_season?;
    let l = stem.to_ascii_lowercase();
    let after_word = ["episode", "ep", "e"].iter().find_map(|w| l.strip_prefix(w).map(|r| stem.len() - r.len()));
    let start = after_word.unwrap_or(0);
    let rest = &stem[start..];
    let rest_t = rest.trim_start_matches([' ', '.', '_', '-']);
    let num_len = rest_t.chars().take_while(|c| c.is_ascii_digit()).count();
    if num_len == 0 || num_len > 3 {
        return None;
    }
    let ep: u32 = rest_t[..num_len].parse().ok()?;
    Some(Episode { show: show_folder()?, season, episode: ep, episode_end: None, title: title_after(&rest_t[num_len..]) })
}

/// Lowercase letters and digits only, so "The Office (US)" and "the.office.us" group together.
pub fn show_key(name: &str) -> String {
    name.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

#[derive(Clone, Debug)]
pub struct Show {
    pub key: String,
    pub name: String,
    /// Sorted by season, then episode.
    pub episodes: Vec<(Episode, Entry)>,
}

impl Show {
    pub fn seasons(&self) -> Vec<u32> {
        let mut v: Vec<u32> = self.episodes.iter().map(|(e, _)| e.season).collect();
        v.dedup();
        v
    }
}

/// My media split into shows and movies.
#[derive(Clone, Debug, Default)]
pub struct Catalog {
    pub shows: Vec<Show>,
    pub movies: Vec<Entry>,
}

impl Catalog {
    pub fn build(entries: &[Entry]) -> Self {
        let mut shows: HashMap<String, Show> = HashMap::new();
        let mut movies = Vec::new();
        for e in entries {
            if crate::library::is_audio(&e.path) {
                continue;
            }
            match episode(&e.path) {
                Some(ep) => {
                    let key = show_key(&ep.show);
                    let show = shows.entry(key.clone()).or_insert_with(|| Show { key, name: ep.show.clone(), episodes: vec![] });
                    show.episodes.push((ep, e.clone()));
                }
                None => movies.push(e.clone()),
            }
        }
        let mut shows: Vec<Show> = shows.into_values().collect();
        for s in &mut shows {
            s.episodes.sort_by(|a, b| (a.0.season, a.0.episode).cmp(&(b.0.season, b.0.episode)).then_with(|| natural_key(&a.1.name).cmp(&natural_key(&b.1.name))));
        }
        shows.sort_by_cached_key(|s| natural_key(s.name.trim_start_matches("The ")));
        movies.sort_by_cached_key(|e| natural_key(&movie_title(&e.name).0));
        Catalog { shows, movies }
    }

    pub fn show(&self, key: &str) -> Option<&Show> {
        self.shows.iter().find(|s| s.key == key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn ep(p: &str) -> Option<(String, u32, u32, Option<String>)> {
        episode(&PathBuf::from(p)).map(|e| (e.show, e.season, e.episode, e.title))
    }

    #[test]
    fn scene_names() {
        assert_eq!(ep("/tv/Breaking.Bad.S01E02.Cats.in.the.Bag.720p.BluRay.x264.mkv"), Some(("Breaking Bad".into(), 1, 2, Some("Cats in the Bag".into()))));
        assert_eq!(ep("/tv/The Office (US) - s03e10 - A Benihana Christmas.mkv"), Some(("The Office (US)".into(), 3, 10, Some("A Benihana Christmas".into()))));
        assert_eq!(ep("/tv/Firefly - 1x05 - Safe.avi"), Some(("Firefly".into(), 1, 5, Some("Safe".into()))));
        assert_eq!(ep("/tv/Show.S02E01E02.1080p.mkv").map(|e| e.2), Some(1));
        assert_eq!(episode(&PathBuf::from("/tv/Show.S02E01E02.mkv")).unwrap().episode_end, Some(2));
    }

    #[test]
    fn folder_layouts() {
        assert_eq!(ep("/tv/Firefly/Season 1/03 - Bushwhacked.mkv"), Some(("Firefly".into(), 1, 3, Some("Bushwhacked".into()))));
        assert_eq!(ep("/tv/Firefly/Season 01/Episode 4.mkv"), Some(("Firefly".into(), 1, 4, None)));
        assert_eq!(ep("/tv/Firefly/S1/S01E06.mkv"), Some(("Firefly".into(), 1, 6, None)));
        // No season folder and no SxxExx: a movie.
        assert_eq!(ep("/movies/Alien (1979)/Alien.1979.1080p.mkv"), None);
        assert_eq!(ep("/movies/1917.2019.mkv"), None);
    }

    #[test]
    fn movies() {
        assert_eq!(movie_title("Blade.Runner.1982.Final.Cut.1080p.BluRay.mkv"), ("Blade Runner".into(), Some(1982)));
        assert_eq!(movie_title("2001 A Space Odyssey (1968).mkv"), ("2001 A Space Odyssey".into(), Some(1968)));
        assert_eq!(movie_title("Heat.mkv"), ("Heat".into(), None));
        assert_eq!(movie_title("[YTS] Arrival.2016.720p.mp4"), ("Arrival".into(), Some(2016)));
    }

    #[test]
    fn catalog_groups_shows() {
        let e = |p: &str| Entry { name: Path::new(p).file_name().unwrap().to_string_lossy().into(), path: p.into(), is_dir: false, size: 1, modified: None, subs: vec![] };
        let c = Catalog::build(&[e("/a/the.office.us.s01e02.mkv"), e("/b/The Office US - S01E01.mkv"), e("/m/Heat.1995.mkv")]);
        assert_eq!(c.shows.len(), 1);
        assert_eq!(c.shows[0].episodes.iter().map(|x| x.0.episode).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(c.movies.len(), 1);
    }
}
