use findoxide::birth::read_birth_time;
use findoxide::literal_time::parse_literal_time;
use findoxide::time::Timestamp;
use std::ffi::OsStr;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn parses_supported_literal_time_subset() {
    assert_eq!(
        parse_literal_time(OsStr::new("@1700000000.25")).unwrap(),
        Timestamp::new(1_700_000_000, 250_000_000)
    );
    assert_eq!(
        parse_literal_time(OsStr::new("@-1.25")).unwrap(),
        Timestamp::new(-2, 750_000_000)
    );
    assert!(parse_literal_time(OsStr::new("2026-04-15")).is_ok());
    assert!(parse_literal_time(OsStr::new("2026-04-15T12:34:56Z")).is_ok());
    assert!(parse_literal_time(OsStr::new("2026-04-15 12:34+08:00")).is_ok());
}

#[test]
fn rejects_unsupported_literal_time_forms() {
    for raw in [
        "yesterday",
        "next friday",
        "2026-04",
        "2026-04-15 PST",
        "2026-02-30",
        "2026-04-15T25:00",
    ] {
        let error = parse_literal_time(OsStr::new(raw)).unwrap_err();
        assert!(error.message.contains("unsupported literal time format"));
    }
}

#[test]
fn birth_lookup_errors_for_missing_paths() {
    let error = read_birth_time(Path::new("/definitely/missing/path"), true).unwrap_err();
    assert!(error.message.contains("/definitely/missing/path"));
}

#[test]
fn birth_lookup_on_tempfile_is_stable() {
    let root = tempdir().unwrap();
    let path = root.path().join("file.txt");
    std::fs::write(&path, "hello\n").unwrap();

    let first = read_birth_time(&path, true).unwrap();
    let second = read_birth_time(&path, true).unwrap();

    assert_eq!(first, second);
}
