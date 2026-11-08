//! Movies vs. TV: works out shows, seasons and episodes from file and folder names alone
//! (no probing, no network), so it's instant even on slow disks.
//!
//! In a TV library (Plex-style) each top-level folder is a show: `Show (2008) {tmdb-1396}`.
//! Inside it: `S01E02`, `1x02`, `S01E01-E02` / `S01E01E02` (multi-episode), dates
//! (`2024-03-05`), absolute numbers (`Show - 105`), `Season 1/03 - Title`, `Specials/` =
//! season 0, and split files (`- pt1`, `cd2`). Outside libraries the file name alone is used.

use crate::library::Entry;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Episode {
    pub show: String,
    /// The show's key in the library database (empty outside libraries).
    pub key: String,
    /// Part of a split episode (`- pt2`).
    pub part: Option<u32>,
    /// Date-based shows: "2024-03-05" (season = year until matched to a real episode).
    pub date: Option<String>,
    /// Absolute-numbered shows: the number in the file (episode = it until mapped).
    pub absolute: Option<u32>,
    pub season: u32,
    pub episode: u32,
    /// Last episode of a multi-episode file (S01E01E02 → 2).
    pub episode_end: Option<u32>,
    /// From the file name, if it has one ("Pilot").
    pub title: Option<String>,
}

impl Episode {
    /// "S01E02", "S01E01–02", "2024-03-05", "E105", "S00E01"; plus " pt2" for parts.
    pub fn code(&self) -> String {
        let base = if let (Some(d), true) = (&self.date, self.season >= 1900) {
            d.clone()
        } else if let (Some(a), true) = (self.absolute, self.season == 1 && self.episode == self.absolute.unwrap_or(0)) {
            format!("E{a:02}")
        } else {
            match self.episode_end {
                Some(e) => format!("S{:02}E{:02}–{:02}", self.season, self.episode, e),
                None => format!("S{:02}E{:02}", self.season, self.episode),
            }
        };
        match self.part {
            Some(p) => format!("{base} pt{p}"),
            None => base,
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

/// Quality words in a file name, for telling copies apart: "1080p · x265 · HDR".
pub fn quality_tags(name: &str) -> String {
    let spaced: String = name.chars().map(|c| if c.is_alphanumeric() || c == '-' { c } else { ' ' }).collect();
    let mut tags: Vec<String> = Vec::new();
    for w in spaced.split_whitespace() {
        let l = w.to_ascii_lowercase();
        if ["mkv", "mp4", "avi", "m4v"].contains(&l.as_str()) {
            continue;
        }
        if is_tag(w) && !tags.iter().any(|t| t.eq_ignore_ascii_case(w)) {
            tags.push(w.to_string());
        }
    }
    tags.join(" · ")
}

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
pub fn season_folder(name: &str) -> Option<u32> {
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
        return Some(Episode { show, season, episode: ep, episode_end: ep_end, title: title_after(&stem[b..]), ..Default::default() });
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
    Some(Episode { show: show_folder()?, season, episode: ep, title: title_after(&rest_t[num_len..]), ..Default::default() })
}

/// A Plex-style folder name: `The Office (US) (2005) {tvdb-73244}` or `Heat (1995) [tmdb-949]`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FolderTitle {
    pub title: String,
    pub year: Option<u32>,
    pub tmdb: Option<u64>,
    pub tvdb: Option<u64>,
    pub imdb: Option<String>,
}

pub fn folder_title(name: &str) -> FolderTitle {
    let mut out = FolderTitle::default();
    let mut rest = String::new();
    // {tmdb-123} / [tvdb-456] / {imdb-tt0113277} hints.
    let mut chars = name.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '{' || c == '[' {
            let close = if c == '{' { '}' } else { ']' };
            if let Some(j) = name[i + 1..].find(close) {
                let inner = name[i + 1..i + 1 + j].trim().to_ascii_lowercase();
                let inner = inner.replace(' ', "");
                let id = inner.split_once(['-', '=']).map(|(k, v)| (k.to_string(), v.to_string()));
                match id {
                    Some((k, v)) if k == "tmdb" || k == "tmdbid" => out.tmdb = v.parse().ok(),
                    Some((k, v)) if k == "tvdb" || k == "tvdbid" => out.tvdb = v.parse().ok(),
                    Some((k, v)) if k == "imdb" || k == "imdbid" => out.imdb = Some(v),
                    _ => {}
                }
                for _ in 0..=j {
                    chars.next();
                }
                continue;
            }
        }
        rest.push(c);
    }
    // A trailing "(2005)" is the year; other parentheses stay ("The Office (US)").
    let t = rest.trim();
    if t.ends_with(')') {
        if let Some(open) = t.rfind('(') {
            if let Some(y) = year_at(&t[open + 1..t.len() - 1]) {
                out.year = Some(y);
                out.title = clean(&t[..open]);
                return out;
            }
        }
    }
    let (title, year) = movie_title(&format!("{t}.x"));
    out.title = if title.is_empty() { t.to_string() } else { title };
    out.year = out.year.or(year);
    out
}

/// "Movie - pt2", "Movie.cd1", "Show S01E01 part 2" → (base, part).
pub fn split_part(stem: &str) -> (&str, Option<u32>) {
    let l = stem.to_ascii_lowercase();
    for marker in ["part", "pt", "cd", "disc", "disk", "dvd"] {
        if let Some(i) = l.rfind(marker) {
            let before_ok = i == 0 || matches!(l.as_bytes()[i - 1], b' ' | b'.' | b'_' | b'-');
            let rest = l[i + marker.len()..].trim_start_matches([' ', '.', '_', '-']);
            if before_ok && !rest.is_empty() && rest.len() <= 2 && rest.chars().all(|c| c.is_ascii_digit()) {
                let base = stem[..i].trim_end_matches([' ', '.', '_', '-']);
                return (base, rest.parse().ok());
            }
        }
    }
    (stem, None)
}

/// `2024-03-05`, `2024.03.05`, `2024 03 05` or `05-03-2024` → ("2024-03-05", year, month, day).
fn find_date(s: &str) -> Option<(String, u32, u32, u32)> {
    let words: Vec<&str> = s.split(|c: char| !c.is_ascii_digit()).filter(|w| !w.is_empty()).collect();
    for w in words.windows(3) {
        let (a, b, c) = (w[0], w[1], w[2]);
        let parse = |x: &str| x.parse::<u32>().ok();
        let (y, m, d) = if a.len() == 4 && b.len() == 2 && c.len() == 2 {
            (parse(a)?, parse(b)?, parse(c)?)
        } else if a.len() == 2 && b.len() == 2 && c.len() == 4 {
            (parse(c)?, parse(b)?, parse(a)?)
        } else {
            continue;
        };
        if (1900..=2099).contains(&y) && (1..=12).contains(&m) && (1..=31).contains(&d) {
            return Some((format!("{y:04}-{m:02}-{d:02}"), y, m, d));
        }
    }
    None
}

/// An episode file inside a TV library show folder. `dirs` are the folders between the
/// show folder and the file ("Season 2", "Specials"…).
pub fn tv_episode(show: &str, dirs: &[String], stem: &str) -> Option<Episode> {
    let (base, part) = split_part(stem);
    let dir_season = dirs.iter().rev().find_map(|d| {
        let l = d.to_ascii_lowercase();
        if l == "specials" || l == "special" {
            Some(0)
        } else {
            season_folder(d)
        }
    });
    let title_after = |s: &str| Some(clean(s)).filter(|t| !t.is_empty() && !t.chars().all(|c| c.is_ascii_digit()) && show_key(t) != show_key(show));
    let mk = |season, episode, end, title| Some(Episode { show: show.to_string(), season, episode, episode_end: end, title, part, ..Default::default() });

    if let Some((_, b, se, ep, end)) = find_sxxexx(base) {
        return mk(se, ep, end, title_after(&base[b..]));
    }
    if let Some((_, b, se, ep)) = find_nxnn(base) {
        return mk(se, ep, None, title_after(&base[b..]));
    }
    if let Some((date, y, m, d)) = find_date(base) {
        let mut e = mk(y, m * 100 + d, None, None)?;
        e.date = Some(date);
        return Some(e);
    }
    // Drop the show's name from the front ("Show - 105", "Show Ep 3").
    let mut rest = base.to_string();
    let rk = show_key(show);
    let mut cut = 0;
    for (i, _) in base.char_indices() {
        if show_key(&base[..i]) == rk {
            cut = i;
        }
    }
    if !rk.is_empty() && show_key(base).starts_with(&rk) && cut > 0 {
        rest = base[cut..].to_string();
    }
    let rest_t = rest.trim_start_matches(|c: char| !c.is_alphanumeric());
    let l = rest_t.to_ascii_lowercase();
    let after_word = ["episode", "ep", "e"].iter().find_map(|w| {
        let r = l.strip_prefix(w)?;
        r.starts_with(|c: char| c.is_ascii_digit() || c == ' ' || c == '.').then(|| rest_t.len() - r.len())
    });
    let num_from = rest_t[after_word.unwrap_or(0)..].trim_start_matches([' ', '.', '_', '-']);
    let n = num_from.chars().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 || n > 4 {
        return None;
    }
    let num: u32 = num_from[..n].parse().ok()?;
    if n == 4 && year_at(&num_from[..n]).is_some() {
        return None;
    }
    let title = title_after(&num_from[n..]);
    match dir_season {
        Some(season) => mk(season, num, None, title),
        None => {
            // No season anywhere: absolute numbering (mapped to real seasons once matched).
            let mut e = mk(1, num, None, title)?;
            e.absolute = Some(num);
            Some(e)
        }
    }
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
    fn plex_folders_and_episodes() {
        let f = folder_title("The Office (US) (2005) {tvdb-73244}");
        assert_eq!((f.title.as_str(), f.year, f.tvdb, f.tmdb), ("The Office (US)", Some(2005), Some(73244), None));
        let f = folder_title("Heat (1995) [tmdb-949]");
        assert_eq!((f.title.as_str(), f.year, f.tmdb), ("Heat", Some(1995), Some(949)));
        assert_eq!(folder_title("Breaking Bad").title, "Breaking Bad");
        let d = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let e = tv_episode("Firefly", &d(&["Season 1"]), "Firefly - S01E02 - Bushwhacked").unwrap();
        assert_eq!((e.season, e.episode, e.title.as_deref()), (1, 2, Some("Bushwhacked")));
        let e = tv_episode("Firefly", &d(&["Specials"]), "03 - Serenity Featurette").unwrap();
        assert_eq!((e.season, e.episode), (0, 3));
        let e = tv_episode("The Daily Show", &[], "The Daily Show 2024-03-05").unwrap();
        assert_eq!((e.date.as_deref(), e.code()), (Some("2024-03-05"), "2024-03-05".to_string()));
        let e = tv_episode("One Piece", &[], "One Piece - 105 - The Ultimate Battle").unwrap();
        assert_eq!((e.absolute, e.season, e.episode, e.code()), (Some(105), 1, 105, "E105".to_string()));
        let e = tv_episode("Show", &d(&["Season 2"]), "Show S02E01-E02 - pt2").unwrap();
        assert_eq!((e.episode, e.episode_end, e.part), (1, Some(2), Some(2)));
        assert_eq!(split_part("Movie (2000) - cd2"), ("Movie (2000)", Some(2)));
        assert_eq!(split_part("Apartment 1"), ("Apartment 1", None));
    }

    #[test]
    fn movies() {
        assert_eq!(movie_title("Blade.Runner.1982.Final.Cut.1080p.BluRay.mkv"), ("Blade Runner".into(), Some(1982)));
        assert_eq!(movie_title("2001 A Space Odyssey (1968).mkv"), ("2001 A Space Odyssey".into(), Some(1968)));
        assert_eq!(movie_title("Heat.mkv"), ("Heat".into(), None));
        assert_eq!(movie_title("[YTS] Arrival.2016.720p.mp4"), ("Arrival".into(), Some(2016)));
    }

}
