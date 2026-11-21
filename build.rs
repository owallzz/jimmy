//! Links jimmy's own slim, static mpv when JIMMY_MPV_PREFIX points at a build from
//! packaging/build-slim-mpv.sh. Without it, the system libmpv is used (dynamically).
//! On Windows it also embeds the exe icon and manifest (assets/jimmy.rc).
use std::path::Path;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=JIMMY_MPV_PREFIX");
    println!("cargo:rerun-if-env-changed=JIMMY_CXX_LIB");
    // build.rs runs on the host, so ask cargo about the target instead of using cfg!.
    let os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let env = std::env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let prefix = std::env::var("JIMMY_MPV_PREFIX").ok();

    if os == "windows" {
        for f in ["jimmy.rc", "jimmy.manifest", "jimmy.ico"] {
            println!("cargo:rerun-if-changed=assets/{f}");
        }
        match embed_resource::compile("assets/jimmy.rc", embed_resource::NONE) {
            embed_resource::CompilationResult::Ok | embed_resource::CompilationResult::NotWindows => {}
            // Release builds (static mpv) must have the icon; a quick cross `cargo check` may lack windres.
            e if prefix.is_some() => panic!("embedding assets/jimmy.rc: {e}"),
            e => eprintln!("not embedding the icon: {e}"),
        }
    }

    let Some(prefix) = prefix else {
        // Development on a Mac links Homebrew's libmpv. Never with JIMMY_MPV_PREFIX (release
        // builds), so those can't pick up a Homebrew dylib.
        if os == "macos" && std::env::var("HOST").unwrap_or_default().contains("apple") {
            let arch = std::env::var("CARGO_CFG_TARGET_ARCH").unwrap_or_default();
            let brew = if arch == "aarch64" { "/opt/homebrew/lib" } else { "/usr/local/lib" };
            if Path::new(brew).exists() {
                println!("cargo:rustc-link-search=native={brew}");
            }
        }
        return;
    };
    if os == "windows" && env == "msvc" {
        panic!("the static mpv build needs a windows-gnu target (MSYS2), not msvc");
    }
    let lib = format!("{prefix}/lib");
    println!("cargo:rerun-if-changed={lib}/libmpv.a");
    let mut pkg = Command::new("pkg-config");
    if os == "macos" {
        // Only our prefix: a Homebrew .pc would pull in dylibs the app can't ship with.
        pkg.env("PKG_CONFIG_LIBDIR", format!("{lib}/pkgconfig")).env_remove("PKG_CONFIG_PATH");
    } else {
        pkg.env("PKG_CONFIG_PATH", format!("{lib}/pkgconfig"));
    }
    let out = pkg.args(["--static", "--libs", "mpv"]).output().expect("running pkg-config");
    assert!(out.status.success(), "pkg-config --static --libs mpv: {}", String::from_utf8_lossy(&out.stderr));
    // Our prefix first, so `-lmpv` (from libmpv2-sys) also resolves to the static archive.
    println!("cargo:rustc-link-search=native={lib}");
    let flags = String::from_utf8_lossy(&out.stdout).into_owned();
    let mut toks = flags.split_whitespace();
    while let Some(tok) = toks.next() {
        if let Some(dir) = tok.strip_prefix("-L") {
            println!("cargo:rustc-link-search=native={dir}");
        } else if let Some(name) = tok.strip_prefix("-l") {
            // Windows (MinGW): everything static, so the exe needs only system DLLs. Windows'
            // own libraries (kernel32, opengl32…) are .a import archives there, so this works.
            // Linux and macOS: static only for archives we built into the prefix.
            let kind = if os == "windows" || Path::new(&format!("{lib}/lib{name}.a")).exists() { "static" } else { "dylib" };
            if kind == "dylib" && os == "macos" && !MACOS_SDK_LIBS.contains(&name) {
                panic!("lib{name} is not in {lib}; the app would need it installed on every Mac");
            }
            println!("cargo:rustc-link-lib={kind}={name}");
        } else if tok == "-framework" {
            let name = toks.next().expect("-framework without a name");
            println!("cargo:rustc-link-lib=framework={name}");
        } else if let Some(name) = tok.strip_prefix("-Wl,-framework,") {
            println!("cargo:rustc-link-lib=framework={name}");
        } else {
            println!("cargo:rustc-link-arg={tok}");
        }
    }
    // libplacebo has a little C++ (float formatting) and its .pc doesn't list the C++ runtime.
    if Path::new(&format!("{lib}/libplacebo.a")).exists() {
        let cxx = match std::env::var("JIMMY_CXX_LIB") {
            // e.g. "static=c++,static=unwind" for an LLVM MinGW toolchain.
            Ok(v) => v.split(',').map(str::to_string).collect(),
            Err(_) => match os.as_str() {
                "macos" => vec!["dylib=c++".to_string()],
                // GCC MinGW: no libstdc++-6.dll or libwinpthread-1.dll next to the exe.
                "windows" => vec!["static=stdc++".to_string(), "static=winpthread".to_string()],
                _ => vec!["dylib=stdc++".to_string()],
            },
        };
        for l in cxx {
            println!("cargo:rustc-link-lib={l}");
        }
    }
    // FFmpeg's VideoToolbox code uses `@available`, which needs `__isPlatformVersionAtLeast`
    // from clang's runtime. clang links it by itself, rustc doesn't.
    if os == "macos" {
        let out = Command::new("cc").arg("-print-resource-dir").output().expect("running cc -print-resource-dir");
        let rt = format!("{}/lib/darwin/libclang_rt.osx.a", String::from_utf8_lossy(&out.stdout).trim());
        assert!(Path::new(&rt).exists(), "clang runtime not found at {rt}");
        println!("cargo:rustc-link-arg={rt}");
    }
}

/// Libraries every Mac has (in the SDK), fine to link dynamically.
const MACOS_SDK_LIBS: &[&str] = &["z", "iconv", "charset", "bz2", "c++", "m", "pthread", "dl", "objc", "resolv", "System"];
