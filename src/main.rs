fn main() {
    restore_default_sigpipe();

    std::process::exit(rushfind::cli::run(std::env::args_os().skip(1)));
}

/// Rust ignores `SIGPIPE` for the whole process, which would turn the usual
/// `rfd . -print | head` pipeline into a `failed to write stdout: Broken pipe`
/// diagnostic plus a nonzero exit status. `find` exits quietly when its reader
/// goes away, so restore the default disposition before doing any work.
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
