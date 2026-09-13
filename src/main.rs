fn main() {
    restore_default_sigpipe();

    std::process::exit(rushfind::cli::run(std::env::args_os().skip(1)));
}

/// Rust ignores `SIGPIPE` process-wide, which turns `rfd . -print | head` into
/// a broken-pipe diagnostic; `find` exits quietly when its reader goes away.
#[cfg(unix)]
fn restore_default_sigpipe() {
    // SAFETY: `signal` with `SIG_DFL` is async-signal-safe and is called before
    // any other thread exists.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

#[cfg(not(unix))]
fn restore_default_sigpipe() {}
