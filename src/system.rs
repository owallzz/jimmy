//! System volume, display backlight and keeping the screen awake.
//!
//! Everything runs on a worker thread through small, always-present system tools, so jimmy
//! needs no extra libraries:
//! - Linux volume: `wpctl` (WirePlumber, part of every PipeWire install).
//! - Linux backlight: read `/sys/class/backlight`, write through systemd-logind (`busctl`), which
//!   works without root for the active session and keeps GNOME's slider in sync.
//! - Linux screen awake (whole time jimmy is open): `gnome-session-inhibit`, falling back to `systemd-inhibit`.
//! - macOS: `osascript` for volume, `caffeinate` to stay awake, no backlight.
//! - Windows: `SetThreadExecutionState` to stay awake; volume is mpv's own, no backlight.

#[cfg(unix)]
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Snapshot {
    /// System output volume 0.0..=1.0, `None` if it can't be controlled.
    pub volume: Option<f32>,
    pub muted: bool,
    /// Backlight 0.0..=1.0, `None` if there is no controllable backlight.
    pub backlight: Option<f32>,
}

enum Cmd {
    SetVolume(f32),
    SetMute(bool),
    SetBacklight(f32),
    Refresh,
    Inhibit(bool),
}

pub struct System {
    tx: Sender<Cmd>,
    shared: Arc<Mutex<Snapshot>>,
    last_refresh: Instant,
}

impl System {
    pub fn new(ctx: egui::Context) -> Self {
        let (tx, rx) = channel();
        let shared = Arc::new(Mutex::new(Snapshot::default()));
        let s = shared.clone();
        let _ = std::thread::Builder::new()
            .name("system".into())
            .spawn(move || worker(rx, s, ctx));
        let _ = tx.send(Cmd::Refresh);
        Self { tx, shared, last_refresh: Instant::now() }
    }

    pub fn snapshot(&self) -> Snapshot {
        *self.shared.lock().unwrap()
    }

    pub fn set_volume(&self, v: f32) {
        let v = v.clamp(0.0, 1.0);
        self.shared.lock().unwrap().volume.replace(v);
        let _ = self.tx.send(Cmd::SetVolume(v));
    }

    pub fn set_mute(&self, m: bool) {
        self.shared.lock().unwrap().muted = m;
        let _ = self.tx.send(Cmd::SetMute(m));
    }

    pub fn set_backlight(&self, v: f32) {
        let v = v.clamp(0.02, 1.0);
        self.shared.lock().unwrap().backlight.replace(v);
        let _ = self.tx.send(Cmd::SetBacklight(v));
    }

    /// Re-read volume/backlight (picks up changes made in GNOME). Rate limited.
    pub fn refresh(&mut self) {
        if self.last_refresh.elapsed() > Duration::from_secs(2) {
            self.last_refresh = Instant::now();
            let _ = self.tx.send(Cmd::Refresh);
        }
    }

    /// Keep the screen on (while playing).
    pub fn inhibit(&self, on: bool) {
        let _ = self.tx.send(Cmd::Inhibit(on));
    }
}

fn worker(rx: Receiver<Cmd>, shared: Arc<Mutex<Snapshot>>, ctx: egui::Context) {
    let mut inhibitor: Option<awake::Guard> = None;
    let backlight = backlight::Device::find();
    while let Ok(first) = rx.recv() {
        // Coalesce bursts (holding a button sends many commands): keep only the latest of each.
        let mut vol = None;
        let mut mute = None;
        let mut bl = None;
        let mut refresh = false;
        let mut inhibit = None;
        for cmd in std::iter::once(first).chain(rx.try_iter()) {
            match cmd {
                Cmd::SetVolume(v) => vol = Some(v),
                Cmd::SetMute(m) => mute = Some(m),
                Cmd::SetBacklight(v) => bl = Some(v),
                Cmd::Refresh => refresh = true,
                Cmd::Inhibit(on) => inhibit = Some(on),
            }
        }
        if let Some(v) = vol {
            volume::set(v);
        }
        if let Some(m) = mute {
            volume::set_mute(m);
        }
        if let (Some(v), Some(dev)) = (bl, &backlight) {
            dev.set(v);
        }
        if let Some(on) = inhibit {
            match (on, inhibitor.is_some()) {
                (true, false) => inhibitor = awake::start(),
                (false, true) => inhibitor = None,
                _ => {}
            }
        }
        if refresh {
            let (volume, muted) = volume::get().map_or((None, false), |(v, m)| (Some(v), m));
            let backlight = backlight.as_ref().and_then(|d| d.get());
            let new = Snapshot { volume, muted, backlight };
            let mut s = shared.lock().unwrap();
            if *s != new {
                *s = new;
                ctx.request_repaint();
            }
        }
    }
    // Dropping `inhibitor` here ends the keep-awake.
}

#[cfg(unix)]
fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd).args(args).stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(target_os = "linux")]
mod volume {
    use super::run;
    const SINK: &str = "@DEFAULT_AUDIO_SINK@";

    /// `wpctl get-volume` prints e.g. `Volume: 0.45` or `Volume: 0.45 [MUTED]`.
    pub fn parse(out: &str) -> Option<(f32, bool)> {
        let v = out.split_whitespace().nth(1)?.parse::<f32>().ok()?;
        Some((v, out.contains("[MUTED]")))
    }

    pub fn get() -> Option<(f32, bool)> {
        parse(&run("wpctl", &["get-volume", SINK])?)
    }

    pub fn set(v: f32) {
        run("wpctl", &["set-volume", SINK, &format!("{v:.3}")]);
    }

    pub fn set_mute(m: bool) {
        run("wpctl", &["set-mute", SINK, if m { "1" } else { "0" }]);
    }
}

#[cfg(target_os = "macos")]
mod volume {
    use super::run;

    pub fn get() -> Option<(f32, bool)> {
        let out = run("osascript", &["-e", "get volume settings"])?;
        // "output volume:45, input volume:50, alert volume:100, output muted:false"
        let field = |k: &str| out.split(", ").find_map(|p| p.trim().strip_prefix(k).map(str::to_string));
        let v = field("output volume:")?.parse::<f32>().ok()? / 100.0;
        Some((v, field("output muted:").as_deref() == Some("true")))
    }

    pub fn set(v: f32) {
        run("osascript", &["-e", &format!("set volume output volume {}", (v * 100.0).round())]);
    }

    pub fn set_mute(m: bool) {
        run("osascript", &["-e", &format!("set volume output muted {m}")]);
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
mod volume {
    pub fn get() -> Option<(f32, bool)> {
        None
    }
    pub fn set(_: f32) {}
    pub fn set_mute(_: bool) {}
}

#[cfg(target_os = "linux")]
mod backlight {
    use super::run;
    use std::path::PathBuf;

    pub struct Device {
        name: String,
        dir: PathBuf,
        max: u32,
    }

    impl Device {
        /// Same preference as GNOME: firmware, then platform, then raw interfaces.
        pub fn find() -> Option<Self> {
            let mut best: Option<(u8, Device)> = None;
            for e in std::fs::read_dir("/sys/class/backlight").ok()?.flatten() {
                let dir = e.path();
                let read = |f: &str| std::fs::read_to_string(dir.join(f)).ok().map(|s| s.trim().to_string());
                let rank = match read("type").as_deref() {
                    Some("firmware") => 3,
                    Some("platform") => 2,
                    _ => 1,
                };
                let Some(max) = read("max_brightness").and_then(|s| s.parse().ok()).filter(|m| *m > 0) else { continue };
                let dev = Device { name: e.file_name().to_string_lossy().into_owned(), dir: dir.clone(), max };
                if best.as_ref().is_none_or(|(r, _)| rank > *r) {
                    best = Some((rank, dev));
                }
            }
            best.map(|(_, d)| d)
        }

        pub fn get(&self) -> Option<f32> {
            let cur: u32 = std::fs::read_to_string(self.dir.join("brightness")).ok()?.trim().parse().ok()?;
            Some(cur as f32 / self.max as f32)
        }

        pub fn set(&self, v: f32) {
            let raw = ((v * self.max as f32).round() as u32).clamp(1, self.max);
            let ok = run(
                "busctl",
                &[
                    "call",
                    "org.freedesktop.login1",
                    "/org/freedesktop/login1/session/auto",
                    "org.freedesktop.login1.Session",
                    "SetBrightness",
                    "ssu",
                    "backlight",
                    &self.name,
                    &raw.to_string(),
                ],
            );
            if ok.is_none() {
                // Fall back to writing sysfs directly (works if a udev rule allows it).
                let _ = std::fs::write(self.dir.join("brightness"), raw.to_string());
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
mod backlight {
    pub struct Device;
    impl Device {
        pub fn find() -> Option<Self> {
            None
        }
        pub fn get(&self) -> Option<f32> {
            None
        }
        pub fn set(&self, _: f32) {}
    }
}

/// Keeps the screen on while a `Guard` is alive.
mod awake {
    #[cfg(unix)]
    use std::process::{Child, Command, Stdio};

    /// Unix: the inhibitor child process, ended on drop.
    #[cfg(unix)]
    pub struct Guard(Child);

    #[cfg(unix)]
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[cfg(unix)]
    fn spawn(cmd: &str, args: &[&str]) -> Option<Guard> {
        Command::new(cmd)
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok()
            .map(Guard)
    }

    // The inhibitor runs a tiny watch loop instead of `sleep infinity`, so if jimmy ever
    // crashes the inhibit ends within seconds instead of keeping the screen on forever.
    #[cfg(target_os = "linux")]
    pub fn start() -> Option<Guard> {
        let watch = format!("while kill -0 {} 2>/dev/null; do sleep 2; done", std::process::id());
        spawn("gnome-session-inhibit", &["--inhibit", "idle:suspend", "--reason", "jimmy is open", "--app-id", "jimmy", "sh", "-c", &watch])
            .or_else(|| spawn("systemd-inhibit", &["--what=idle:sleep", "--who=jimmy", "--why=jimmy is open", "sh", "-c", &watch]))
    }

    #[cfg(target_os = "macos")]
    pub fn start() -> Option<Guard> {
        // -w: caffeinate exits by itself when jimmy does.
        spawn("caffeinate", &["-d", "-w", &std::process::id().to_string()])
    }

    // The execution state belongs to the calling thread; the long-lived `system` worker both
    // sets and clears it.
    #[cfg(windows)]
    pub struct Guard;

    #[cfg(windows)]
    impl Drop for Guard {
        fn drop(&mut self) {
            use windows_sys::Win32::System::Power::{ES_CONTINUOUS, SetThreadExecutionState};
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
        }
    }

    #[cfg(windows)]
    pub fn start() -> Option<Guard> {
        use windows_sys::Win32::System::Power::{ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED, SetThreadExecutionState};
        let ok = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED | ES_SYSTEM_REQUIRED) } != 0;
        ok.then_some(Guard)
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    #[test]
    fn parses_wpctl() {
        assert_eq!(super::volume::parse("Volume: 0.45\n"), Some((0.45, false)));
        assert_eq!(super::volume::parse("Volume: 1.00 [MUTED]\n"), Some((1.0, true)));
    }
}
