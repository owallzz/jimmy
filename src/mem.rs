//! Keeping the process small on Linux/glibc.

/// Fewer malloc arenas: every thread that allocates otherwise gets its own pool, which
/// holds on to memory. jimmy and mpv run ~20 threads; two pools are plenty.
pub fn init() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        const M_ARENA_MAX: libc::c_int = -8;
        libc::mallopt(M_ARENA_MAX, 2);
    }
}

/// Give freed memory back to the OS (after playback stops, after the prober shuts down).
pub fn trim() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    unsafe {
        libc::malloc_trim(0);
    }
}
