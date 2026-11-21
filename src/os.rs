//! Small platform helpers shared across modules.

use std::path::PathBuf;
use std::process::Command;

/// A child process that never flashes a console window on Windows (jimmy is a GUI app there).
pub fn command(program: &str) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// `file:///home/a%20b.mkv` → `/home/a b.mkv` (`file:///C:/x.mkv` → `C:/x.mkv` on Windows).
/// Anything that isn't a file:// URI comes back unchanged.
pub fn from_file_uri(s: &str) -> String {
    let Some(rest) = s.strip_prefix("file://") else { return s.to_string() };
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let bytes = rest.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = bytes.get(i + 1..i + 3).and_then(|h| u8::from_str_radix(std::str::from_utf8(h).ok()?, 16).ok());
        match (bytes[i], hex) {
            (b'%', Some(b)) => {
                out.push(b);
                i += 3;
            }
            (b, _) => {
                out.push(b);
                i += 1;
            }
        }
    }
    let path = String::from_utf8_lossy(&out).into_owned();
    if cfg!(windows) && path.as_bytes().get(2) == Some(&b':') && path.starts_with('/') {
        return path[1..].to_string();
    }
    path
}

/// Absolute form of a path given on the command line. Windows: no `\\?\` prefix (which
/// `canonicalize` adds), so it matches paths from the file browser in history and caches.
pub fn absolute(p: PathBuf) -> PathBuf {
    #[cfg(windows)]
    return std::path::absolute(&p).unwrap_or(p);
    #[cfg(not(windows))]
    return p.canonicalize().unwrap_or(p);
}

/// GUI-subsystem builds have no console: reuse the terminal jimmy was started from (if any),
/// so `--help` and `--version` still show there. Only for commands that print and exit: a
/// player attached to a terminal would be killed by Ctrl+C or closing that terminal.
#[cfg(windows)]
pub fn attach_console() {
    let prints = std::env::args().skip(1).any(|a| {
        matches!(a.as_str(), "-h" | "--help" | "-V" | "--version" | "--install" | "--uninstall")
    });
    if prints {
        attach_parent_console();
    }
}

/// Attach to the parent's console (no-op if there is none or one is already attached).
#[cfg(windows)]
pub fn attach_parent_console() {
    use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

#[cfg(not(windows))]
pub fn attach_parent_console() {}

#[cfg(not(windows))]
pub fn attach_console() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uris() {
        assert_eq!(from_file_uri("file:///home/a%20b.mkv"), "/home/a b.mkv");
        assert_eq!(from_file_uri("file://localhost/x/%C3%A9.mp4"), "/x/é.mp4");
        assert_eq!(from_file_uri("/plain/100%.mkv"), "/plain/100%.mkv");
        assert_eq!(from_file_uri("file:///bad%zz"), "/bad%zz");
        let win = from_file_uri("file:///C:/v/x.mkv");
        assert_eq!(win, if cfg!(windows) { "C:/v/x.mkv" } else { "/C:/v/x.mkv" });
    }
}
