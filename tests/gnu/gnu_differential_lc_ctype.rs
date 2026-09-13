#![cfg(unix)]

use crate::support::{
    PRINTF_TIME_TZ, available_lc_ctype_locales, ensure_gnu_find, gnu_find_command, path_arg,
    rushfind_command,
};
use std::ffi::OsString;
use std::fs;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::symlink;
use std::process::Output;
use std::sync::OnceLock;
use tempfile::tempdir;

const LC_CTYPE_CANDIDATES: &[&str] = &[
    "C",
    "C.utf8",
    "C.UTF-8",
    "en_US.utf8",
    "en_US.UTF-8",
    "ru_RU.KOI8-R",
    "ru_RU.koi8r",
    "ja_JP.eucJP",
    "ja_JP.eucjp",
    "ja_JP.SJIS",
    "ko_KR.eucKR",
    "ko_KR.euckr",
    "zh_CN.GB18030",
    "zh_CN.gb18030",
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

// Cache the oracle qualification once, including when the three matrix tests
// run concurrently. C/POSIX and UTF-8 always retain their existing assertions.
fn oracle_lc_ctype_locales() -> &'static [String] {
    static LOCALES: OnceLock<Vec<String>> = OnceLock::new();
    LOCALES.get_or_init(|| {
        if !ensure_gnu_find() {
            return Vec::new();
        }
        available_lc_ctype_locales(LC_CTYPE_CANDIDATES)
            .into_iter()
            .filter(|locale| {
                let normalized = locale.to_ascii_lowercase().replace(['-', '_'], "");
                if normalized.contains("utf8") || encoded_alpha_name(locale).is_none() {
                    return true;
                }
                if gnu_supports_non_utf8_locale(locale) {
                    true
                } else {
                    eprintln!(
                        "skipping GNU locale oracle cases for locale={locale}: \
                         GNU find cannot classify the encoded non-ASCII letter as one \
                         alphabetic character; C/POSIX, UTF-8 and rushfind-owned tests remain enabled"
                    );
                    false
                }
            })
            .collect()
    })
}

fn gnu_supports_non_utf8_locale(locale: &str) -> bool {
    let alpha = encoded_alpha_name(locale).unwrap();
    let root = tempdir().unwrap();
    let probe = root.path().join("locale-probe");
    // The filename is ASCII. Only the symlink payload carries legacy bytes,
    // so this also works on filesystems that reject non-UTF-8 filenames.
    symlink(&alpha, &probe).unwrap();
    let args = [
        path_arg(&probe),
        "-lname".into(),
        "?".into(),
        "-lname".into(),
        "[[:alpha:]]".into(),
        "-printf".into(),
        "aware".into(),
    ];
    let probe_output = |locale: &str| {
        let output = gnu_output(locale, &args).unwrap();
        assert!(
            output.status.success(),
            "GNU locale probe failed: locale={locale} {output:?}"
        );
        assert!(
            output.stderr.is_empty(),
            "GNU locale probe diagnostic: locale={locale} {output:?}"
        );
        output.stdout
    };
    // A byte-only C matcher must not classify a legacy non-ASCII letter as
    // alphabetic. The probe never consults rushfind's result to decide a skip.
    assert!(
        probe_output("C").is_empty(),
        "invalid C-locale oracle control"
    );
    let encoded = probe_output(locale);
    assert!(
        encoded.is_empty() || encoded == b"aware",
        "invalid GNU locale probe output: {encoded:?}"
    );
    encoded == b"aware"
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

    for locale in oracle_lc_ctype_locales() {
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
    for locale in oracle_lc_ctype_locales() {
        let Some(alpha_name) = encoded_alpha_name(&locale) else {
            continue;
        };

        let root = tempdir().unwrap();
        match fs::write(root.path().join(&alpha_name), "alpha\n") {
            Ok(()) => {}
            // Some filesystems, including APFS, reject non-UTF-8 names.
            // Keep testing the other locales and cover legacy bytes in
            // symlink targets below, which do not need to name a real file.
            Err(error) if error.raw_os_error() == Some(libc::EILSEQ) => {
                eprintln!("skipping locale={locale}: temp filesystem rejects {alpha_name:?}");
                continue;
            }
            Err(error) => panic!("locale={locale} cannot create {alpha_name:?}: {error}"),
        }
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

#[test]
fn lc_ctype_candidate_matrix_matches_gnu_for_encoded_symlink_targets() {
    for locale in oracle_lc_ctype_locales() {
        let Some(alpha_target) = encoded_alpha_name(&locale) else {
            continue;
        };

        let root = tempdir().unwrap();
        symlink(&alpha_target, root.path().join("alpha-link")).unwrap();
        symlink("5", root.path().join("digit-link")).unwrap();
        symlink("two", root.path().join("long-link")).unwrap();

        for flag in ["-lname", "-ilname"] {
            for pattern in [
                OsString::from("?"),
                "[[:alpha:]]".into(),
                "[[:digit:]]".into(),
                alpha_target.clone(),
            ] {
                assert_matches_gnu(
                    &locale,
                    &[
                        path_arg(root.path()),
                        "-maxdepth".into(),
                        "1".into(),
                        flag.into(),
                        pattern,
                        "-printf".into(),
                        "%f\n".into(),
                    ],
                );
            }
        }
    }
}
