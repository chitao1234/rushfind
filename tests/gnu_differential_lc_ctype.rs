#![cfg(unix)]

mod support;

use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::process::Output;
use support::{
    PRINTF_TIME_TZ, available_lc_ctype_locales, gnu_find_command, path_arg, rushfind_command,
};
use tempfile::tempdir;

const LC_CTYPE_CANDIDATES: &[&str] = &[
    "C",
    "C.utf8",
    "C.UTF-8",
    "en_US.utf8",
    "en_US.UTF-8",
    "ru_RU.KOI8-R",
    "ja_JP.eucJP",
    "ja_JP.SJIS",
    "ko_KR.eucKR",
    "zh_CN.GB18030",
];

fn os_from_encoded_text(label: &str, text: &str) -> OsString {
    let encoding = encoding_rs::Encoding::for_label(label.as_bytes()).unwrap();
    let (bytes, _, had_errors) = encoding.encode(text);
    assert!(!had_errors, "encoding {label} cannot represent {text:?}");
    OsString::from_vec(bytes.into_owned())
}

fn encoded_alpha_name(locale: &str) -> Option<OsString> {
    let normalized = locale.to_ascii_lowercase().replace(['-', '_'], "");
    let (label, text) = if normalized.contains("utf8") {
        ("utf-8", "é")
    } else if normalized.contains("koi8r") {
        ("koi8-r", "Ж")
    } else if normalized.contains("eucjp") {
        ("euc-jp", "あ")
    } else if normalized.contains("sjis") {
        ("shift-jis", "あ")
    } else if normalized.contains("euckr") {
        ("euc-kr", "가")
    } else if normalized.contains("gb18030") {
        ("gb18030", "中")
    } else {
        return None;
    };

    Some(os_from_encoded_text(label, text))
}

fn gnu_output(locale: &str, args: &[OsString]) -> Option<Output> {
    let Some(mut command) = gnu_find_command() else {
        return None;
    };
    Some(
        command
            .env("LC_ALL", locale)
            .env("TZ", PRINTF_TIME_TZ)
            .args(args)
            .output()
            .unwrap(),
    )
}

fn rfd_output(locale: &str, args: &[OsString]) -> Output {
    rushfind_command()
        .env("LC_ALL", locale)
        .env("TZ", PRINTF_TIME_TZ)
        .env("RUSHFIND_WORKERS", "1")
        .args(args)
        .output()
        .unwrap()
}

fn assert_matches_gnu(locale: &str, args: &[OsString]) {
    let Some(gnu) = gnu_output(locale, args) else {
        return;
    };
    let rfd = rfd_output(locale, args);

    assert_eq!(
        rfd.status.code(),
        gnu.status.code(),
        "locale={locale} args={args:?}"
    );
    assert_eq!(rfd.stdout, gnu.stdout, "locale={locale} args={args:?}");
    assert_eq!(rfd.stderr, gnu.stderr, "locale={locale} args={args:?}");
}

#[test]
fn lc_ctype_candidate_matrix_matches_gnu_for_ascii_classes() {
    let root = tempdir().unwrap();
    fs::write(root.path().join("alpha"), "alpha\n").unwrap();
    fs::write(root.path().join("5"), "digit\n").unwrap();

    for locale in available_lc_ctype_locales(LC_CTYPE_CANDIDATES) {
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
                "-iname".into(),
                "ALPHA".into(),
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
                ".*/[[:alpha:]]+".into(),
                "-printf".into(),
                "%f\n".into(),
            ],
        ] {
            assert_matches_gnu(&locale, &args);
        }
    }
}

#[test]
fn lc_ctype_candidate_matrix_matches_gnu_for_encoded_single_characters() {
    for locale in available_lc_ctype_locales(LC_CTYPE_CANDIDATES) {
        let Some(alpha_name) = encoded_alpha_name(&locale) else {
            continue;
        };

        let root = tempdir().unwrap();
        fs::write(root.path().join(&alpha_name), "alpha\n").unwrap();
        fs::write(root.path().join("5"), "digit\n").unwrap();

        for args in [
            vec![
                path_arg(root.path()),
                "-maxdepth".into(),
                "1".into(),
                "-name".into(),
                "?".into(),
                "-printf".into(),
                "%f\n".into(),
            ],
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
            assert_matches_gnu(&locale, &args);
        }
    }
}
