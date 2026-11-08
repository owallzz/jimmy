//! `jimmy --install` / `--uninstall`: per-user desktop integration (Linux).
//! The launcher and icon are embedded in the binary, so jimmy stays a single file.

const DESKTOP: &str = include_str!("../assets/jimmy.desktop");
const ICON: &str = include_str!("../assets/jimmy.svg");

#[cfg(target_os = "linux")]
mod imp {
    use super::{DESKTOP, ICON};
    use std::path::PathBuf;
    use std::process::Command;

    fn home() -> anyhow::Result<PathBuf> {
        directories::BaseDirs::new()
            .map(|b| b.home_dir().to_path_buf())
            .ok_or_else(|| anyhow::anyhow!("no home directory"))
    }

    fn paths() -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
        let h = home()?;
        Ok((
            h.join(".local/bin/jimmy"),
            h.join(".local/share/applications/jimmy.desktop"),
            h.join(".local/share/icons/hicolor/scalable/apps/jimmy.svg"),
        ))
    }

    fn refresh_caches() {
        let h = match home() {
            Ok(h) => h,
            Err(_) => return,
        };
        let _ = Command::new("update-desktop-database").arg(h.join(".local/share/applications")).status();
        let _ = Command::new("gtk-update-icon-cache").args(["-f", "-t"]).arg(h.join(".local/share/icons/hicolor")).status();
    }

    pub fn install(make_default: bool) -> anyhow::Result<()> {
        let (bin, desktop, icon) = paths()?;
        let me = std::env::current_exe()?;
        for p in [&bin, &desktop, &icon] {
            std::fs::create_dir_all(p.parent().unwrap())?;
        }
        if me.canonicalize().ok() != bin.canonicalize().ok() {
            let tmp = bin.with_extension("new");
            std::fs::copy(&me, &tmp)?;
            std::fs::rename(&tmp, &bin)?;
        }
        // Absolute Exec path so it works even if ~/.local/bin isn't on PATH.
        let entry = DESKTOP.replace("Exec=jimmy %F", &format!("Exec={} %F", bin.display()));
        std::fs::write(&desktop, entry)?;
        std::fs::write(&icon, ICON)?;
        refresh_caches();
        println!("installed:\n  {}\n  {}\n  {}", bin.display(), desktop.display(), icon.display());
        if make_default {
            let types: Vec<&str> = DESKTOP
                .lines()
                .find_map(|l| l.strip_prefix("MimeType="))
                .unwrap_or("")
                .split(';')
                .filter(|t| t.starts_with("video/"))
                .collect();
            let status = Command::new("xdg-mime").args(["default", "jimmy.desktop"]).args(&types).status();
            match status {
                Ok(s) if s.success() => println!("jimmy is now the default video player"),
                _ => println!("could not set default player (is xdg-mime installed?)"),
            }
        }
        println!("jimmy should now appear in the app menu.");
        Ok(())
    }

    pub fn uninstall() -> anyhow::Result<()> {
        let (bin, desktop, icon) = paths()?;
        for p in [&desktop, &icon, &bin] {
            match std::fs::remove_file(p) {
                Ok(()) => println!("removed {}", p.display()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => println!("could not remove {}: {e}", p.display()),
            }
        }
        refresh_caches();
        Ok(())
    }
}

#[cfg(not(target_os = "linux"))]
mod imp {
    pub fn install(_: bool) -> anyhow::Result<()> {
        let _ = (super::DESKTOP, super::ICON);
        anyhow::bail!("--install sets up the Linux desktop launcher; it does nothing on this platform")
    }
    pub fn uninstall() -> anyhow::Result<()> {
        anyhow::bail!("--uninstall only applies on Linux")
    }
}

pub use imp::{install, uninstall};
