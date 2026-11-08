//! Links jimmy's own slim, static mpv when JIMMY_MPV_PREFIX points at a build from
//! packaging/build-slim-mpv.sh. Without it, the system libmpv is used (dynamically).
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=JIMMY_MPV_PREFIX");
    let Ok(prefix) = std::env::var("JIMMY_MPV_PREFIX") else { return };
    let lib = format!("{prefix}/lib");
    println!("cargo:rerun-if-changed={lib}/libmpv.a");
    let out = Command::new("pkg-config")
        .env("PKG_CONFIG_PATH", format!("{lib}/pkgconfig"))
        .args(["--static", "--libs", "mpv"])
        .output()
        .expect("running pkg-config");
    assert!(out.status.success(), "pkg-config --static --libs mpv: {}", String::from_utf8_lossy(&out.stderr));
    // Our prefix first, so `-lmpv` (from libmpv2-sys) also resolves to the static archive.
    println!("cargo:rustc-link-search=native={lib}");
    for tok in String::from_utf8_lossy(&out.stdout).split_whitespace() {
        if let Some(dir) = tok.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(name) = tok.strip_prefix("-l") {
            let kind = if Path::new(&format!("{lib}/lib{name}.a")).exists() { "static" } else { "dylib" };
            println!("cargo:rustc-link-lib={kind}={name}");
        } else {
            println!("cargo:rustc-link-arg={tok}");
        }
    }
    // libplacebo has a little C++ (float formatting) and its .pc doesn't list the C++ runtime.
    if Path::new(&format!("{lib}/libplacebo.a")).exists() {
        println!("cargo:rustc-link-lib=dylib=stdc++");
    }
}
