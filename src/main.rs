//! jimmy: a lightweight, gamepad-friendly media player.

// No console window next to the player on Windows (debug builds keep it for logs).
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod action;
mod config;
mod input;
mod install;
mod libdb;
mod library;
mod media;
mod mem;
mod mpv_render;
mod meta;
mod os;
mod player;
mod scrape;
mod subs;
mod playlist;
mod system;
mod theme;
mod ui;

use std::path::PathBuf;
use ui::{Startup, Step};

const HELP: &str = "\
jimmy — lightweight media player

USAGE:
    jimmy [FILE | FOLDER]

OPTIONS:
    --install [--default]   Add jimmy to the app menu (Linux, per user).
                            --default also makes it the default video player.
    --uninstall             Remove it from the app menu.
    --fullscreen            Start fullscreen.
    --windowed              Start in a window.
    --size WxH              Window size.
    --actions LIST          Run comma-separated actions after startup (for testing),
                            e.g. down,accept,wait:2000,screenshot:/tmp/a.png,quit
                            key:j and type:text send real key presses and typing
    -h, --help              Show this help.
    -V, --version           Show the version.
";

fn parse_steps(list: &str) -> Result<Vec<Step>, String> {
    list.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| {
            if let Some(ms) = s.strip_prefix("wait:") {
                ms.parse().map(Step::Wait).map_err(|_| format!("bad wait: {s}"))
            } else if let Some(p) = s.strip_prefix("screenshot:") {
                Ok(Step::Screenshot(PathBuf::from(p)))
            } else if let Some(b) = s.strip_prefix("pad_down:").or_else(|| s.strip_prefix("pad_up:")) {
                let down = s.starts_with("pad_down:");
                toml::Value::String(b.to_string())
                    .try_into::<input::PadInput>()
                    .map(|p| if down { Step::PadDown(p) } else { Step::PadUp(p) })
                    .map_err(|_| format!("unknown gamepad button: {s}"))
            } else if let Some(b) = s.strip_prefix("pad:") {
                toml::Value::String(b.to_string())
                    .try_into::<input::PadInput>()
                    .map(Step::Pad)
                    .map_err(|_| format!("unknown gamepad button: {s}"))
            } else if let Some(k) = s.strip_prefix("key:") {
                k.parse().map(Step::Key).map_err(|e| format!("{s}: {e}"))
            } else if let Some(t) = s.strip_prefix("type:") {
                Ok(Step::Type(t.to_string()))
            } else if s == "quit" {
                Ok(Step::Quit)
            } else {
                toml::Value::String(s.to_string())
                    .try_into::<action::Action>()
                    .map(Step::Act)
                    .map_err(|_| format!("unknown action: {s}"))
            }
        })
        .collect()
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            // GUI-subsystem builds on Windows start without a console: attach to the terminal
            // jimmy was started from (if any) so the error is visible there.
            os::attach_parent_console();
            eprintln!("Error: {e:#}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> anyhow::Result<()> {
    os::attach_console();
    mem::init();
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();

    let mut args = std::env::args().skip(1);
    let mut open = None;
    let mut steps = Vec::new();
    let mut fullscreen: Option<bool> = None;
    let mut size = [1280.0f32, 760.0];
    while let Some(a) = args.next() {
        match a.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return Ok(());
            }
            "-V" | "--version" => {
                println!("jimmy {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--install" => {
                let default = args.next().as_deref() == Some("--default");
                return install::install(default);
            }
            "--uninstall" => return install::uninstall(),
            "--fullscreen" => fullscreen = Some(true),
            "--windowed" => fullscreen = Some(false),
            "--size" => {
                let v = args.next().unwrap_or_default();
                let (w, h) = v.split_once('x').ok_or_else(|| anyhow::anyhow!("--size wants WxH"))?;
                size = [w.parse()?, h.parse()?];
            }
            "--actions" => {
                steps = parse_steps(&args.next().unwrap_or_default()).map_err(|e| anyhow::anyhow!(e))?;
            }
            // Old macOS passes a process serial number when launched from Finder.
            other if other.starts_with("-psn_") => {}
            other if other.starts_with('-') => anyhow::bail!("unknown option {other}\n\n{HELP}"),
            path => {
                // Accept file:// URIs from desktop launchers.
                open = Some(os::absolute(PathBuf::from(os::from_file_uri(path))));
            }
        }
    }

    let mut cfg = config::Config::load();
    if let Some(f) = fullscreen {
        cfg.start_fullscreen = f;
        if !f {
            // Don't overwrite the saved preference, just this run.
        }
    }

    // Inside Flatpak the Wayland app_id must match the Flatpak ID, so the desktop finds our icon.
    let app_id = std::env::var("FLATPAK_ID").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "jimmy".into());
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("jimmy")
        .with_app_id(app_id)
        .with_inner_size(size)
        .with_min_inner_size([640.0, 400.0]);
    if let Some(icon) = window_icon() {
        viewport = viewport.with_icon(std::sync::Arc::new(icon));
    }
    let options = eframe::NativeOptions {
        viewport,
        renderer: eframe::Renderer::Glow,
        persist_window: false,
        ..Default::default()
    };
    let start_fullscreen = cfg.start_fullscreen;
    eframe::run_native(
        "jimmy",
        options,
        Box::new(move |cc| Ok(Box::new(ui::App::new(cc, Startup { open, steps, fullscreen: start_fullscreen })))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

/// Window/taskbar icon (the exe icon on Windows comes from assets/jimmy.rc).
fn window_icon() -> Option<egui::IconData> {
    let mut dec = png::Decoder::new(std::io::Cursor::new(&include_bytes!("../assets/jimmy-256.png")[..]));
    dec.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = dec.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba).then_some(egui::IconData { rgba: buf, width: info.width, height: info.height })
}
