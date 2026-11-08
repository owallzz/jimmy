//! jimmy: a lightweight, gamepad-friendly media player.

mod action;
mod config;
mod input;
mod install;
mod library;
mod player;
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
                            e.g. down,down,accept,wait:2000,screenshot:/tmp/a.png,quit
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

fn main() -> anyhow::Result<()> {
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
            other if other.starts_with('-') => anyhow::bail!("unknown option {other}\n\n{HELP}"),
            path => {
                // Accept file:// URIs from desktop launchers.
                let path = path.strip_prefix("file://").unwrap_or(path);
                let p = PathBuf::from(path);
                open = Some(p.canonicalize().unwrap_or(p));
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

    let viewport = egui::ViewportBuilder::default()
        .with_title("jimmy")
        .with_app_id("jimmy")
        .with_inner_size(size)
        .with_min_inner_size([640.0, 400.0]);
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
