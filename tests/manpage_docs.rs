use std::path::Path;
use std::process::{Command, Stdio};

#[test]
fn scdoc_source_contains_standalone_command_reference() {
    let source = std::fs::read_to_string("doc/rfd.1.scd").expect("read doc/rfd.1.scd");

    for expected in [
        "rfd(1)",
        "# NAME",
        "# SYNOPSIS",
        "# DESCRIPTION",
        "# OPTIONS",
        "# EXPRESSIONS",
        "# TESTS",
        "# ACTIONS",
        "# OPERATORS",
        "# TRAVERSAL CONTROLS",
        "# COMPATIBILITY OPTIONS",
        "# PRINTF FORMAT",
        "# PARALLEL EXECUTION",
        "# EXIT STATUS",
        "# EXAMPLES",
        "# CAVEATS",
        "# SEE ALSO",
        "*-files0-from* _FILE_",
        "*-printf* _FORMAT_",
        "*-exec* _COMMAND_ *;*",
        "*-context*",
        "*-type* _LIST_",
        "*-xtype* _LIST_",
        "Lightweight debug diagnostics",
    ] {
        assert!(source.contains(expected), "missing {expected:?}");
    }

    assert!(
        !source.contains("see GNU find"),
        "manpage must be standalone instead of deferring to GNU find docs"
    );
}

#[test]
fn generated_manpage_contains_key_sections() {
    let generated = std::fs::read_to_string("doc/rfd.1").expect("read doc/rfd.1");

    for expected in [
        ".TH \"rfd\" \"1\"",
        ".SH NAME",
        ".SH SYNOPSIS",
        ".SH DESCRIPTION",
        ".SH OPTIONS",
        ".SH EXPRESSIONS",
        ".SH TESTS",
        ".SH ACTIONS",
        ".SH OPERATORS",
        ".SH TRAVERSAL CONTROLS",
        ".SH COMPATIBILITY OPTIONS",
        ".SH PRINTF FORMAT",
        ".SH PARALLEL EXECUTION",
        ".SH EXIT STATUS",
        ".SH EXAMPLES",
        ".SH CAVEATS",
        ".SH SEE ALSO",
        "rfd - find files with GNU-style expressions and parallel traversal",
    ] {
        assert!(generated.contains(expected), "missing {expected:?}");
    }
}

#[test]
fn manpage_generation_script_exists_and_is_executable_on_unix() {
    let script = Path::new("scripts/generate_manpage.sh");
    let metadata = std::fs::metadata(script).expect("stat scripts/generate_manpage.sh");
    assert!(metadata.is_file());

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(
            metadata.permissions().mode() & 0o111,
            0,
            "scripts/generate_manpage.sh must be executable"
        );
    }
}

#[test]
fn generated_manpage_is_in_sync_when_scdoc_is_installed() {
    if Command::new("scdoc")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_err()
    {
        eprintln!("skipping manpage drift test because scdoc is not installed");
        return;
    }

    let output = Command::new("sh")
        .arg("-c")
        .arg("scdoc < doc/rfd.1.scd")
        .output()
        .expect("run scdoc");
    assert!(
        output.status.success(),
        "scdoc failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let checked_in = std::fs::read("doc/rfd.1").expect("read checked-in doc/rfd.1");
    assert_eq!(output.stdout, checked_in, "doc/rfd.1 is stale");
}
