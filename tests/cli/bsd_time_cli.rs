//! BSD time spellings: unit durations such as `-mtime 1h30m`, and the NetBSD
//! `-since` family.

#![cfg(unix)]

use crate::support::{path_arg, rushfind_command_with_workers};
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::tempdir;

fn run(args: &[OsString]) -> Output {
    rushfind_command_with_workers(1)
        .args(args)
        .output()
        .unwrap()
}

fn names(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_owned)
        .collect()
}

/// Sets a file's modification time `seconds` before now.
fn set_age(path: &Path, seconds: i64) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let target = now - seconds;
    let times = [
        libc::timespec {
            tv_sec: target as libc::time_t,
            tv_nsec: 0,
        },
        libc::timespec {
            tv_sec: target as libc::time_t,
            tv_nsec: 0,
        },
    ];
    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `c_path` and `times` are valid for the duration of the call.
    let rc = unsafe { libc::utimensat(libc::AT_FDCWD, c_path.as_ptr(), times.as_ptr(), 0) };
    assert_eq!(rc, 0, "utimensat failed for {}", path.display());
}

fn aged_tree(seconds: i64) -> tempfile::TempDir {
    let root = tempdir().unwrap();
    let path = root.path().join("file");
    fs::write(&path, "content\n").unwrap();
    set_age(&path, seconds);
    root
}

/// BSD compares unit durations as raw seconds, so `+`/`-` are strict. The
/// ages stay well clear of the value: an exact match is a one-second window.
#[test]
fn unit_durations_compare_raw_seconds() {
    for (age, spec, expected) in [
        (7200, "+1h", true),
        (7200, "-1h", false),
        (1800, "+1h", false),
        (1800, "-1h", true),
        (7200, "+30m", true),
        (900, "+30m", false),
        (900, "-30m", true),
        (7200, "-1d", true),
    ] {
        let tree = aged_tree(age);
        let output = run(&[
            path_arg(tree.path()),
            OsString::from("-maxdepth"),
            OsString::from("1"),
            OsString::from("-type"),
            OsString::from("f"),
            OsString::from("-mtime"),
            OsString::from(spec),
        ]);

        assert_eq!(
            !names(&output).is_empty(),
            expected,
            "-mtime {spec} on a file aged {age}s"
        );
    }
}

/// Every spelling of the same duration has to behave identically.
#[test]
fn compound_durations_add_their_components() {
    for age in [1800, 7200, 90000] {
        let tree = aged_tree(age);
        let mut seen: Option<(String, bool)> = None;

        for spec in ["1h30m", "90m", "5400s", "0w1h30m", "1h1800s"] {
            let output = run(&[
                path_arg(tree.path()),
                OsString::from("-maxdepth"),
                OsString::from("1"),
                OsString::from("-type"),
                OsString::from("f"),
                OsString::from("-mtime"),
                OsString::from(format!("+{spec}")),
            ]);
            let matched = !names(&output).is_empty();

            match &seen {
                Some((first, first_matched)) => assert_eq!(
                    matched, *first_matched,
                    "{spec} disagrees with {first} for a file aged {age}s"
                ),
                None => seen = Some((spec.to_owned(), matched)),
            }
        }
    }
}

#[test]
fn malformed_durations_are_diagnostics_not_wildcards() {
    let root = tempdir().unwrap();
    for (spec, needle) in [
        ("1x", "invalid numeric argument for `-mtime`"),
        ("1h3", "invalid numeric argument for `-mtime`"),
        ("1.5h", "invalid numeric argument for `-mtime`"),
        ("h", "invalid numeric argument for `-mtime`"),
        (
            "99999999999999999999s",
            "invalid numeric argument for `-mtime`",
        ),
        (
            "1h99999999999999999999s",
            "invalid numeric argument for `-mtime`",
        ),
    ] {
        let output = run(&[
            path_arg(root.path()),
            OsString::from("-mtime"),
            OsString::from(spec),
        ]);
        assert_eq!(output.status.code(), Some(1), "-mtime {spec}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(needle),
            "-mtime {spec} -> {:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

/// FreeBSD spells units on the day-based primaries only.
#[test]
fn unit_durations_stay_out_of_the_minute_primaries() {
    let root = tempdir().unwrap();
    let output = run(&[
        path_arg(root.path()),
        OsString::from("-mmin"),
        OsString::from("1h"),
    ]);

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("invalid numeric argument for `-mmin`")
    );
}

/// NetBSD's `-since DATE` is `-newermt DATE`, and the access and change forms
/// line up the same way.
#[test]
fn netbsd_since_aliases_match_their_newerxy_spellings() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("file"), "content\n").unwrap();

    for (alias, newer) in [
        ("-since", "-newermt"),
        ("-asince", "-newerat"),
        ("-csince", "-newerct"),
    ] {
        for date in ["2020-01-01", "@1600000000", "2999-01-01"] {
            let aliased = run(&[
                path_arg(root.path()),
                OsString::from(alias),
                OsString::from(date),
            ]);
            let spelled = run(&[
                path_arg(root.path()),
                OsString::from(newer),
                OsString::from(date),
            ]);

            assert_eq!(
                names(&aliased),
                names(&spelled),
                "{alias} {date} should agree with {newer} {date}"
            );
        }
    }
}

/// Birth-time predicates read the same timestamp `%B` renders. They used to
/// abort the walk.
#[test]
fn birth_time_predicates_find_fresh_files() {
    let root = tempdir().unwrap();
    let path = root.path().join("file");
    fs::write(&path, "content\n").unwrap();

    if rushfind::birth::read_birth_time(&path, false)
        .unwrap()
        .is_none()
    {
        eprintln!("host reports no birth time; skipping");
        return;
    }

    let matched = run(&[
        path_arg(root.path()),
        OsString::from("-name"),
        OsString::from("file"),
        OsString::from("-Bmin"),
        OsString::from("-1"),
    ]);
    assert_eq!(
        names(&matched),
        vec![format!("{}/file", root.path().display())]
    );

    let unmatched = run(&[
        path_arg(root.path()),
        OsString::from("-name"),
        OsString::from("file"),
        OsString::from("-Btime"),
        OsString::from("+1"),
    ]);
    assert!(names(&unmatched).is_empty());
}

fn platform_bsd_find() -> Option<PathBuf> {
    for candidate in ["/usr/bin/find", "/bin/find"] {
        let path = PathBuf::from(candidate);
        if !path.exists() {
            continue;
        }

        // GNU find rejects the unit spelling; a BSD one accepts it.
        let probe = Command::new(&path)
            .args([".", "-maxdepth", "0", "-mtime", "1h"])
            .output()
            .ok()?;
        if probe.stderr.is_empty() {
            return Some(path);
        }
    }

    None
}

/// Where the platform ships a BSD `find`, the unit forms have to agree with it
/// edge for edge. Ages sit at least three seconds away from a boundary, since
/// the two runs are separate processes.
#[test]
fn unit_durations_agree_with_the_platforms_bsd_find() {
    let Some(bsd_find) = platform_bsd_find() else {
        eprintln!("no BSD find on this platform; skipping");
        return;
    };

    let root = tempdir().unwrap();
    let path = root.path().join("file");
    fs::write(&path, "content\n").unwrap();

    for spec in [
        "1s", "1m", "1h", "2h", "1h30m", "1d", "1w", "+1h", "-1h", "+1d", "-1d",
    ] {
        for age in [
            5, 55, 65, 3594, 3607, 5330, 5470, 86000, 86800, 600000, 610000,
        ] {
            set_age(&path, age);

            let theirs = Command::new(&bsd_find)
                .arg(root.path())
                .args(["-maxdepth", "1", "-type", "f", "-mtime", spec])
                .output()
                .unwrap();
            let ours = run(&[
                path_arg(root.path()),
                OsString::from("-maxdepth"),
                OsString::from("1"),
                OsString::from("-type"),
                OsString::from("f"),
                OsString::from("-mtime"),
                OsString::from(spec),
            ]);

            assert_eq!(
                !String::from_utf8_lossy(&theirs.stdout).trim().is_empty(),
                !names(&ours).is_empty(),
                "-mtime {spec} at age {age}s disagrees with {}",
                bsd_find.display()
            );
        }
    }
}
