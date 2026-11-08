//! Playlists are plain `.m3u8` files: jimmy's own live in `<data dir>/playlists/`, and any
//! `.m3u`/`.m3u8` in a library folder works too (paths may be relative to the playlist).

use std::path::{Path, PathBuf};

pub const EXTS: &[&str] = &["m3u", "m3u8"];

pub fn is_playlist(p: &Path) -> bool {
    p.extension().is_some_and(|e| EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Where playlists made in jimmy are saved.
pub fn dir() -> PathBuf {
    crate::config::data_dir().join("playlists")
}

pub fn is_own(p: &Path) -> bool {
    p.parent() == Some(dir().as_path())
}

pub fn name(p: &Path) -> String {
    p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
}

/// Entries in order. Comments (`#EXTINF`…), blank lines and URLs are skipped.
pub fn read(p: &Path) -> Vec<PathBuf> {
    let Ok(text) = std::fs::read_to_string(p) else { return vec![] };
    let base = p.parent().unwrap_or(Path::new("/"));
    text.lines()
        .map(|l| l.trim().trim_start_matches('\u{feff}'))
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| {
            let l = l.strip_prefix("file://").unwrap_or(l);
            if l.contains("://") {
                return None;
            }
            let path = PathBuf::from(l.replace('\\', "/"));
            Some(if path.is_absolute() { path } else { base.join(path) })
        })
        .collect()
}

pub fn write(p: &Path, entries: &[PathBuf]) -> std::io::Result<()> {
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    let mut text = String::from("#EXTM3U\n");
    for e in entries {
        text.push_str(&e.to_string_lossy());
        text.push('\n');
    }
    let tmp = p.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, p)
}

/// jimmy's own playlists, newest first.
pub fn list_own() -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(dir()) else { return vec![] };
    let mut v: Vec<(std::time::SystemTime, PathBuf)> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| is_playlist(p))
        .map(|p| (std::fs::metadata(&p).and_then(|m| m.modified()).unwrap_or(std::time::UNIX_EPOCH), p))
        .collect();
    v.sort_by(|a, b| b.0.cmp(&a.0));
    v.into_iter().map(|(_, p)| p).collect()
}

/// A new playlist file named `base` (or `base 2`, `base 3`… if taken).
pub fn new_path(base: &str) -> PathBuf {
    let clean: String = base.chars().map(|c| if c == '/' || c == '\\' || c == ':' { ' ' } else { c }).collect();
    let clean = clean.trim();
    let clean = if clean.is_empty() { "Playlist" } else { clean };
    let d = dir();
    let mut n = 1;
    loop {
        let name = if n == 1 { format!("{clean}.m3u8") } else { format!("{clean} {n}.m3u8") };
        let p = d.join(name);
        if !p.exists() {
            return p;
        }
        n += 1;
    }
}

/// Appends files that aren't already in the playlist. Returns how many were added.
pub fn add(p: &Path, files: &[PathBuf]) -> std::io::Result<usize> {
    let mut entries = if p.exists() { read(p) } else { vec![] };
    let before = entries.len();
    for f in files {
        if !entries.contains(f) {
            entries.push(f.clone());
        }
    }
    write(p, &entries)?;
    Ok(entries.len() - before)
}

pub fn remove(p: &Path, file: &Path) -> std::io::Result<()> {
    let entries: Vec<PathBuf> = read(p).into_iter().filter(|e| e != file).collect();
    write(p, &entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_relative_and_absolute() {
        let d = std::env::temp_dir().join(format!("jimmy-pl-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("x.m3u");
        std::fs::write(&p, "#EXTM3U\n#EXTINF:12,A\na.mkv\n\n/abs/b.mp4\nhttp://x/y.mp4\nsub\\c.avi\n").unwrap();
        assert_eq!(read(&p), [d.join("a.mkv"), PathBuf::from("/abs/b.mp4"), d.join("sub/c.avi")]);
        assert_eq!(add(&p, &[d.join("a.mkv"), PathBuf::from("/new.mkv")]).unwrap(), 1);
        assert_eq!(read(&p).last().unwrap(), &PathBuf::from("/new.mkv"));
        std::fs::remove_dir_all(d).unwrap();
    }
}
