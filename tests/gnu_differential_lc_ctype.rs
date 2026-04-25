#![cfg(unix)]

mod support;

use std::fs;
use support::{
    PRINTF_TIME_TZ, first_available_locale, gnu_find_command, path_arg, rushfind_command,
};
use tempfile::tempdir;

#[test]
fn utf8_lc_ctype_name_and_regex_match_gnu_when_locale_available() {
    let Some(locale) = first_available_locale(&["en_US.utf8", "en_US.UTF-8", "C.utf8", "C.UTF-8"])
    else {
        return;
    };

    let root = tempdir().unwrap();
    fs::write(root.path().join("é"), "accent\n").unwrap();
    fs::write(root.path().join("5"), "digit\n").unwrap();

    for args in [
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-name".into(),
            "[[:alpha:]]".into(),
            "-printf".into(),
            "%f\n".into(),
        ],
        vec![
            path_arg(root.path()),
            "-maxdepth".into(),
            "1".into(),
            "-regextype".into(),
            "posix-extended".into(),
            "-regex".into(),
            ".*/[[:alpha:]]".into(),
            "-printf".into(),
            "%f\n".into(),
        ],
    ] {
        let Some(mut gnu_command) = gnu_find_command() else {
            return;
        };
        let gnu = gnu_command
            .env("LC_ALL", &locale)
            .env("TZ", PRINTF_TIME_TZ)
            .args(&args)
            .output()
            .unwrap();
        let rfd = rushfind_command()
            .env("LC_ALL", &locale)
            .env("TZ", PRINTF_TIME_TZ)
            .env("RUSHFIND_WORKERS", "1")
            .args(&args)
            .output()
            .unwrap();

        assert_eq!(rfd.status.code(), gnu.status.code(), "args: {args:?}");
        assert_eq!(rfd.stdout, gnu.stdout, "args: {args:?}");
    }
}
