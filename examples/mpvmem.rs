//! Headless memory probe for libmpv's cost, without any window or GPU:
//!   mpvmem link            # just link libmpv (library startup cost)
//!   mpvmem core            # + create an mpv instance
//!   mpvmem play FILE [k=v] # + play FILE with vo=null (decode cost), extra mpv options
use std::time::Duration;

fn mem() -> String {
    let s = std::fs::read_to_string("/proc/self/smaps").unwrap_or_default();
    let (mut rss, mut pv, mut anon, mut heap, mut in_heap) = (0u64, 0u64, 0u64, 0u64, false);
    for l in s.lines() {
        let mut it = l.split_whitespace();
        let first = it.next().unwrap_or("");
        if first.contains('-') && !first.ends_with(':') {
            in_heap = l.ends_with("[heap]");
            continue;
        }
        let v: u64 = it.next().and_then(|x| x.parse().ok()).unwrap_or(0);
        match first {
            "Rss:" => {
                rss += v;
                if in_heap {
                    heap += v;
                }
            }
            "Private_Clean:" | "Private_Dirty:" => pv += v,
            "Anonymous:" => anon += v,
            _ => {}
        }
    }
    let m = |k: u64| k as f64 / 1024.0;
    format!("RSS {:5.1}  private {:5.1}  anon {:5.1}  heap {:5.1} MB", m(rss), m(pv), m(anon), m(heap))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mode = args.first().map(String::as_str).unwrap_or("link");
    let v = unsafe { libmpv2_sys::mpv_client_api_version() };
    println!("linked     {}  (api {:x})", mem(), v);
    if mode == "link" {
        return;
    }
    let mpv = libmpv2::Mpv::with_initializer(|i| {
        for (k, v) in [
            ("vo", "null"), ("ao", "null"), ("config", "no"), ("load-scripts", "no"), ("ytdl", "no"), ("terminal", "no"), ("idle", "yes"),
            ("load-stats-overlay", "no"), ("load-console", "no"), ("load-osd-console", "no"), ("load-select", "no"),
            ("load-positioning", "no"), ("load-context-menu", "no"), ("load-commands", "no"), ("load-auto-profiles", "no"),
            ("demuxer-max-bytes", "32MiB"), ("demuxer-max-back-bytes", "5MiB"),
        ] {
            let _ = i.set_option(k, v);
        }
        for kv in args.iter().skip(2) {
            if let Some((k, v)) = kv.split_once('=') {
                let _ = i.set_option(k, v);
            }
        }
        Ok(())
    })
    .unwrap();
    std::thread::sleep(Duration::from_secs(2));
    println!("core       {}", mem());
    if mode == "play" {
        mpv.command("loadfile", &[&args[1]]).unwrap();
        for s in [5, 10, 15] {
            std::thread::sleep(Duration::from_secs(5));
            let pos: f64 = mpv.get_property("time-pos").unwrap_or(-1.0);
            println!("play {s:>3}s  {}  pos {pos:.0}", mem());
        }
    }
}
