//! The matched library: shows, movies and files, saved in `<data>/library.json`.
//!
//! Shows and movies are keyed by their TMDB ID once matched (`tv:1396`, `movie:949`), and by
//! their folder or file until then (`dir:/tv/Firefly`, `file:/movies/Heat.mkv`). Each file
//! remembers its size, mtime, when it was added, and which show/episode or movie it is.
//! A background scan (`library::list_all`) feeds [`Db::merge`], which adds new files, drops
//! deleted ones (unless their drive is missing) and reports what still needs matching.

use crate::config::{Library, LibraryKind};
use crate::library::Entry;
use crate::media::{self, Episode};
use crate::scrape::Info;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MatchState {
    /// Not looked up yet.
    #[default]
    New,
    /// Matched automatically (by ID hint or best search result).
    Auto,
    /// Picked with Fix Match: never replaced automatically.
    Manual,
    /// Looked up, nothing found (retried after a few days).
    NotFound,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Item {
    /// From the folder (or file) name.
    pub title: String,
    pub year: Option<u32>,
    pub hint_tmdb: Option<u64>,
    pub hint_tvdb: Option<u64>,
    pub hint_imdb: Option<String>,
    /// Show folders (several if they matched the same show) or a movie's folder.
    pub folders: Vec<PathBuf>,
    pub state: MatchState,
    pub tmdb: Option<u64>,
    pub info: Info,
}

impl Item {
    /// Matched title, else the folder/file title.
    pub fn name(&self) -> &str {
        if self.info.title.is_empty() { &self.title } else { &self.info.title }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct FileRec {
    pub size: u64,
    pub mtime: u64,
    /// Unix time it showed up in the library (file mtime for the first scan of a library).
    pub added: u64,
    /// Key into `shows` or `movies`.
    pub item: String,
    /// Set for episodes.
    pub ep: Option<Episode>,
    /// Split movies (`cd1`, `- pt2`).
    pub part: Option<u32>,
}

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Db {
    pub shows: BTreeMap<String, Item>,
    pub movies: BTreeMap<String, Item>,
    pub files: BTreeMap<PathBuf, FileRec>,
    /// Downloaded/shrunk images: source (URL or local file) → thumbnail PNG.
    pub images: HashMap<String, PathBuf>,
    #[serde(skip)]
    dirty: bool,
}

fn mtime(e: &Entry) -> u64 {
    crate::meta::stamp(e.size, e.modified).1
}

impl Db {
    pub fn path() -> PathBuf {
        crate::config::data_dir().join("library.json")
    }

    pub fn load() -> Self {
        std::fs::read(Self::path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save_if_dirty(&mut self) {
        if !self.dirty {
            return;
        }
        self.dirty = false;
        let Ok(b) = serde_json::to_vec(self) else { return };
        let p = Self::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("tmp");
        if std::fs::write(&tmp, b).is_ok() {
            let _ = std::fs::rename(tmp, p);
        }
    }

    pub fn touch(&mut self) {
        self.dirty = true;
    }

    pub fn file(&self, path: &Path) -> Option<&FileRec> {
        self.files.get(path)
    }

    pub fn item(&self, key: &str) -> Option<&Item> {
        self.shows.get(key).or_else(|| self.movies.get(key))
    }

    /// Drop the files under a folder so the next scan classifies them again (library type changed).
    pub fn forget_under(&mut self, dir: &Path) {
        self.files.retain(|p, _| !p.starts_with(dir));
        self.dirty = true;
    }

    pub fn is_show(&self, key: &str) -> bool {
        self.shows.contains_key(key)
    }

    pub fn image(&self, src: &str) -> Option<&PathBuf> {
        self.images.get(src).filter(|p| p.is_file())
    }

    /// Finds the existing record for a folder (or creates one with a folder key).
    fn key_for_folder(&mut self, show: bool, folder: &Path, make: impl FnOnce() -> Item) -> String {
        let table = if show { &mut self.shows } else { &mut self.movies };
        if let Some((k, _)) = table.iter().find(|(_, v)| v.folders.iter().any(|f| f == folder)) {
            return k.clone();
        }
        let key = format!("{}{}", if show { "dir:" } else { "file:" }, folder.display());
        let mut it = make();
        it.folders = vec![folder.to_path_buf()];
        table.insert(key.clone(), it);
        self.dirty = true;
        key
    }

    /// Takes a full library scan. Returns keys of shows/movies that need matching.
    pub fn merge(&mut self, entries: &[Entry], libraries: &[Library], now: u64) -> Vec<String> {
        let mut seen = std::collections::HashSet::new();
        let fresh_lib: Vec<bool> = libraries.iter().map(|l| !self.files.keys().any(|f| f.starts_with(&l.path))).collect();
        for e in entries {
            if e.is_dir || crate::playlist::is_playlist(&e.path) || crate::library::is_audio(&e.path) {
                continue;
            }
            let Some((li, lib)) = libraries.iter().enumerate().filter(|(_, l)| e.path.starts_with(&l.path)).max_by_key(|(_, l)| l.path.as_os_str().len()) else { continue };
            seen.insert(e.path.clone());
            let (size, mt) = (e.size, mtime(e));
            if let Some(f) = self.files.get(&e.path) {
                if f.size == size && f.mtime == mt && self.item(&f.item).is_some() {
                    continue;
                }
            }
            let added = self.files.get(&e.path).map(|f| f.added).unwrap_or(if fresh_lib[li] { mt.min(now) } else { now });
            let rec = match lib.kind {
                LibraryKind::Shows => self.classify_episode(&e.path, &lib.path),
                LibraryKind::Movies => Some(self.classify_movie(&e.path)),
            };
            let Some((item, ep, part)) = rec else {
                // Not recognisable as an episode: still playable from My media and folders.
                self.files.remove(&e.path);
                continue;
            };
            self.files.insert(e.path.clone(), FileRec { size, mtime: mt, added, item, ep, part });
            self.dirty = true;
        }
        // Forget files that are gone, but not when their whole library folder is missing
        // (unplugged drive): they come back when it does.
        let present: Vec<&Library> = libraries.iter().filter(|l| l.path.is_dir()).collect();
        let before = self.files.len();
        self.files.retain(|p, _| seen.contains(p) || !present.iter().any(|l| p.starts_with(&l.path)) && libraries.iter().any(|l| p.starts_with(&l.path)));
        if self.files.len() != before {
            self.dirty = true;
        }
        // Shows/movies with no files left go too.
        let used: std::collections::HashSet<&String> = self.files.values().map(|f| &f.item).collect();
        let (ns, nm) = (self.shows.len(), self.movies.len());
        self.shows.retain(|k, _| used.contains(k));
        self.movies.retain(|k, _| used.contains(k));
        if ns != self.shows.len() || nm != self.movies.len() {
            self.dirty = true;
        }
        self.unmatched()
    }

    /// Shows/movies that haven't been looked up (or failed a while ago).
    pub fn unmatched(&self) -> Vec<String> {
        const RETRY: u64 = 3 * 24 * 3600;
        let now = crate::config::now_unix();
        self.shows
            .iter()
            .chain(self.movies.iter())
            .filter(|(_, v)| v.state == MatchState::New || v.state == MatchState::NotFound && now > v.info.fetched + RETRY)
            .map(|(k, _)| k.clone())
            .collect()
    }

    /// A file in a TV library: the top-level folder is the show.
    fn classify_episode(&mut self, path: &Path, root: &Path) -> Option<(String, Option<Episode>, Option<u32>)> {
        let rel = path.strip_prefix(root).ok()?;
        let comps: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
        let stem = path.file_stem()?.to_string_lossy().into_owned();
        let (folder, dirs) = if comps.len() >= 2 {
            (root.join(&comps[0]), comps[1..comps.len() - 1].to_vec())
        } else {
            // Loose file at the library's top: group by the show name in the file name.
            let ep = media::episode(path)?;
            (root.join(&ep.show), vec![])
        };
        let ft = media::folder_title(&folder.file_name()?.to_string_lossy());
        let mut ep = media::tv_episode(&ft.title, &dirs, &stem)?;
        let key = self.key_for_folder(true, &folder, || Item { title: ft.title.clone(), year: ft.year, hint_tmdb: ft.tmdb, hint_tvdb: ft.tvdb, hint_imdb: ft.imdb.clone(), ..Default::default() });
        ep.key = key.clone();
        if let Some(show) = self.shows.get(&key) {
            ep.show = show.name().to_string();
            resolve(&mut ep, &show.info);
        }
        Some((key, Some(ep), None))
    }

    /// A file in a Movies library: one movie per `Title (Year)` folder, else per file
    /// (split files `cd1`/`cd2` share one).
    fn classify_movie(&mut self, path: &Path) -> (String, Option<Episode>, Option<u32>) {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let (base, part) = media::split_part(&stem);
        let parent = path.parent().unwrap_or(Path::new("/"));
        let pft = media::folder_title(&parent.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        // A "Title (Year)" folder (or one with an ID hint) is the movie; otherwise the file is.
        let folder_is_movie = pft.year.is_some() || pft.tmdb.is_some() || pft.imdb.is_some();
        let (folder, ft) = if folder_is_movie {
            (parent.to_path_buf(), pft)
        } else {
            let (t, y) = media::movie_title(&format!("{base}.x"));
            let hinted = media::folder_title(base);
            (parent.join(base), media::FolderTitle { title: t, year: y, ..hinted })
        };
        let key = self.key_for_folder(false, &folder, || Item { title: ft.title.clone(), year: ft.year, hint_tmdb: ft.tmdb, hint_imdb: ft.imdb.clone(), ..Default::default() });
        (key, None, part)
    }

    /// Stores a match. With a TMDB ID the record moves to `tv:ID` / `movie:ID` (merging with a
    /// record already there), and its files follow.
    pub fn apply_match(&mut self, key: &str, tmdb: Option<u64>, info: Info, manual: bool) -> String {
        let show = self.is_show(key);
        let table = if show { &mut self.shows } else { &mut self.movies };
        let Some(mut it) = table.remove(key) else { return key.to_string() };
        if it.state == MatchState::Manual && !manual {
            table.insert(key.to_string(), it);
            return key.to_string();
        }
        let found = !info.title.is_empty() || info.poster.is_some();
        it.state = if manual { MatchState::Manual } else if found { MatchState::Auto } else { MatchState::NotFound };
        it.tmdb = tmdb.or(it.tmdb);
        it.info = info;
        let new_key = match tmdb {
            Some(id) => format!("{}:{id}", if show { "tv" } else { "movie" }),
            None => key.to_string(),
        };
        if let Some(existing) = table.get_mut(&new_key).filter(|_| new_key != key) {
            for f in it.folders {
                if !existing.folders.contains(&f) {
                    existing.folders.push(f);
                }
            }
            existing.info = it.info;
            existing.state = it.state;
        } else {
            table.insert(new_key.clone(), it);
        }
        let name = self.item(&new_key).map(|i| i.name().to_string()).unwrap_or_default();
        let info = self.item(&new_key).map(|i| i.info.clone()).unwrap_or_default();
        for f in self.files.values_mut().filter(|f| f.item == key || f.item == new_key) {
            f.item = new_key.clone();
            if let Some(ep) = f.ep.as_mut() {
                ep.key = new_key.clone();
                ep.show = name.clone();
                resolve(ep, &info);
            }
        }
        self.dirty = true;
        new_key
    }

    /// Everything as browser rows: shows (with their episodes) and movies (first part).
    pub fn catalog(&self, entries: &HashMap<PathBuf, Entry>) -> media::Catalog {
        let mut shows: HashMap<&String, media::Show> = HashMap::new();
        let mut movies: HashMap<&String, Vec<(Option<u32>, Entry)>> = HashMap::new();
        for (path, f) in &self.files {
            let Some(entry) = entries.get(path) else { continue };
            match (&f.ep, self.shows.get(&f.item)) {
                (Some(ep), Some(show)) => {
                    let s = shows.entry(&f.item).or_insert_with(|| media::Show { key: f.item.clone(), name: show.name().to_string(), episodes: vec![] });
                    s.episodes.push((ep.clone(), entry.clone()));
                }
                (None, _) if self.movies.contains_key(&f.item) => movies.entry(&f.item).or_default().push((f.part, entry.clone())),
                _ => {}
            }
        }
        let mut shows: Vec<media::Show> = shows.into_values().collect();
        for s in &mut shows {
            s.episodes.sort_by(|a, b| (a.0.season, a.0.episode, a.0.part).cmp(&(b.0.season, b.0.episode, b.0.part)).then_with(|| crate::library::natural_key(&a.1.name).cmp(&crate::library::natural_key(&b.1.name))));
        }
        shows.sort_by_cached_key(|s| crate::library::natural_key(s.name.trim_start_matches("The ")));
        let mut mv: Vec<(String, Entry)> = movies
            .into_iter()
            .filter_map(|(k, mut parts)| {
                parts.sort_by_key(|p| p.0);
                let name = self.movies.get(k).map(|m| m.name().to_string()).unwrap_or_default();
                parts.into_iter().next().map(|(_, e)| (name, e))
            })
            .collect();
        mv.sort_by_cached_key(|(n, _)| crate::library::natural_key(n.trim_start_matches("The ")));
        media::Catalog { shows, movies: mv.into_iter().map(|(_, e)| e).collect() }
    }

    /// All parts of the movie a file belongs to, in order (just the file if it isn't split).
    pub fn movie_parts(&self, path: &Path) -> Vec<PathBuf> {
        let Some(f) = self.files.get(path).filter(|f| f.ep.is_none()) else { return vec![path.to_path_buf()] };
        let mut v: Vec<(Option<u32>, PathBuf)> = self.files.iter().filter(|(_, g)| g.item == f.item && g.ep.is_none()).map(|(p, g)| (g.part, p.clone())).collect();
        if v.len() < 2 || v.iter().any(|(p, _)| p.is_none()) {
            return vec![path.to_path_buf()];
        }
        v.sort();
        v.into_iter().map(|(_, p)| p).collect()
    }

    /// Newest files first (for Recently added).
    pub fn recently_added(&self, n: usize) -> Vec<PathBuf> {
        let mut v: Vec<(&PathBuf, u64)> = self.files.iter().map(|(p, f)| (p, f.added)).collect();
        v.sort_by(|a, b| b.1.cmp(&a.1));
        v.into_iter().take(n).map(|(p, _)| p.clone()).collect()
    }
}

/// Maps date-based and absolute-numbered episodes to real seasons/episodes once the show's
/// episode list is known.
pub fn resolve(ep: &mut Episode, info: &Info) {
    if info.episodes.is_empty() {
        return;
    }
    if let Some(d) = &ep.date {
        if let Some(k) = info.episodes.iter().find(|(_, e)| e.aired == *d).map(|(k, _)| k.clone()) {
            if let Some((s, e)) = parse_ep_key(&k) {
                ep.season = s;
                ep.episode = e;
            }
        }
    } else if let Some(abs) = ep.absolute {
        // Count episodes through the regular seasons in order.
        let mut keys: Vec<(u32, u32)> = info.episodes.keys().filter_map(|k| parse_ep_key(k)).filter(|(s, _)| *s > 0).collect();
        keys.sort();
        if let Some(&(s, e)) = keys.get(abs as usize - 1).filter(|_| abs > 0) {
            ep.season = s;
            ep.episode = e;
            ep.absolute = None;
        }
    }
}

fn parse_ep_key(k: &str) -> Option<(u32, u32)> {
    let (s, e) = k.split_once('x')?;
    Some((s.parse().ok()?, e.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(p: &str) -> Entry {
        Entry { name: Path::new(p).file_name().unwrap().to_string_lossy().into(), path: p.into(), is_dir: false, size: 10, modified: None, subs: vec![] }
    }

    #[test]
    fn merges_shows_movies_and_rekeys_on_match() {
        let root = std::env::temp_dir().join(format!("jimmy-db-{}", std::process::id()));
        let (tv, mov) = (root.join("tv"), root.join("mov"));
        std::fs::create_dir_all(&tv).unwrap();
        std::fs::create_dir_all(&mov).unwrap();
        let e = |p: &str| entry(&root.join(p.trim_start_matches('/')).to_string_lossy());
        let libs = vec![Library { path: tv.clone(), kind: LibraryKind::Shows }, Library { path: mov.clone(), kind: LibraryKind::Movies }];
        let mut db = Db::default();
        let todo = db.merge(
            &[
                e("/tv/Firefly (2002) {tmdb-1437}/Season 1/Firefly - S01E01.mkv"),
                e("/tv/Firefly (2002) {tmdb-1437}/Season 1/Firefly - S01E02.mkv"),
                e("/tv/Firefly (2002) {tmdb-1437}/Specials/S00E01 - Serenity.mkv"),
                e("/mov/Heat (1995)/Heat.1995.cd1.mkv"),
                e("/mov/Heat (1995)/Heat.1995.cd2.mkv"),
                e("/mov/Alien.1979.mkv"),
            ],
            &libs,
            1000,
        );
        assert_eq!(db.shows.len(), 1);
        assert_eq!(db.movies.len(), 2);
        assert_eq!(todo.len(), 3);
        let key = db.shows.keys().next().unwrap().clone();
        assert_eq!(db.shows[&key].hint_tmdb, Some(1437));
        let parts = db.movie_parts(&mov.join("Heat (1995)/Heat.1995.cd2.mkv"));
        assert_eq!(parts.len(), 2);
        let info = Info { title: "Firefly".into(), ..Default::default() };
        let nk = db.apply_match(&key, Some(1437), info, false);
        assert_eq!(nk, "tv:1437");
        assert!(db.files.values().filter(|f| f.ep.is_some()).all(|f| f.item == "tv:1437"));
        // A rescan keeps the match.
        db.merge(&[e("/tv/Firefly (2002) {tmdb-1437}/Season 1/Firefly - S01E03.mkv")], &libs, 2000);
        assert!(db.shows.contains_key("tv:1437"));
        assert_eq!(db.files.len(), 1);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn absolute_numbers_map_to_seasons() {
        let mut info = Info::default();
        for (s, e) in [(1, 1), (1, 2), (2, 1), (0, 1)] {
            info.episodes.insert(crate::scrape::ep_key(s, e), Default::default());
        }
        let mut ep = media::tv_episode("One Piece", &[], "One Piece - 003").unwrap();
        resolve(&mut ep, &info);
        assert_eq!((ep.season, ep.episode), (2, 1));
    }
}
