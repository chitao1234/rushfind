#![cfg(windows)]

mod support;

use std::fs;
use std::time::Duration;
use support::windows::{
    file_group_sid, file_owner_name, file_owner_sid, normalize_stdout_path,
    ownership_probe_available,
};
use support::{cargo_bin_output_with_timeout, path_arg};
use tempfile::tempdir;

fn skip_without_ownership_probe() -> bool {
    if ownership_probe_available() {
        return false;
    }

    eprintln!("skipping Windows ownership CLI test: ownership probe unavailable");
    true
}

#[test]
fn owner_matches_current_file_owner_name() {
    if skip_without_ownership_probe() {
        return;
    }

    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();
    let owner = file_owner_name(&file);

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(file.as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-owner".into(),
            owner.into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("{}\n", normalize_stdout_path(&file.display().to_string())),
    );
}

#[test]
fn owner_sid_and_group_sid_match_current_file_security_descriptor() {
    if skip_without_ownership_probe() {
        return;
    }

    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();
    let owner_sid = file_owner_sid(&file);
    let group_sid = file_group_sid(&file);

    for (flag, value) in [("-owner-sid", owner_sid), ("-group-sid", group_sid)] {
        let output = cargo_bin_output_with_timeout(
            &[
                path_arg(file.as_path()),
                "-maxdepth".into(),
                "0".into(),
                flag.into(),
                value.into(),
                "-print".into(),
            ],
            1,
            Duration::from_secs(5),
        );

        assert_eq!(output.status.code(), Some(0), "{flag}");
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            format!("{}\n", normalize_stdout_path(&file.display().to_string())),
            "{flag}"
        );
    }
}

#[test]
fn malformed_owner_sid_is_rejected_before_runtime() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    let output = cargo_bin_output_with_timeout(
        &[
            path_arg(file.as_path()),
            "-maxdepth".into(),
            "0".into(),
            "-owner-sid".into(),
            "not-a-sid".into(),
            "-print".into(),
        ],
        1,
        Duration::from_secs(5),
    );

    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("invalid SID")
    );
}

#[test]
fn uid_and_gid_nudge_toward_sid_predicates_on_windows() {
    let root = tempdir().unwrap();
    let file = root.path().join("alpha.txt");
    fs::write(&file, b"alpha").unwrap();

    for (flag, needle) in [
        ("-uid", "use -owner-sid for SID matching"),
        ("-gid", "use -group-sid for SID matching"),
    ] {
        let output = cargo_bin_output_with_timeout(
            &[
                path_arg(file.as_path()),
                "-maxdepth".into(),
                "0".into(),
                flag.into(),
                "0".into(),
                "-print".into(),
            ],
            1,
            Duration::from_secs(5),
        );

        assert_eq!(output.status.code(), Some(1), "{flag}");
        assert!(
            String::from_utf8(output.stderr).unwrap().contains(needle),
            "{flag}"
        );
    }
}
