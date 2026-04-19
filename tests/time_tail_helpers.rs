use findoxide::birth::read_birth_time;
use findoxide::literal_time::parse_literal_time;
use findoxide::time::Timestamp;
use std::ffi::OsStr;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn parses_expanded_literal_time_forms() {
    assert_eq!(
        parse_literal_time(OsStr::new("@1700000000.25")).unwrap(),
        Timestamp::new(1_700_000_000, 250_000_000)
    );
    assert_eq!(
        parse_literal_time(OsStr::new("@-1.25")).unwrap(),
        Timestamp::new(-2, 750_000_000)
    );
    assert_eq!(
        parse_literal_time(OsStr::new("@1.123456789987")).unwrap(),
        Timestamp::new(1, 123_456_789)
    );
    assert_eq!(
        parse_literal_time(OsStr::new("@1.2")).unwrap(),
        Timestamp::new(1, 200_000_000)
    );

    for raw in [
        "2026-04-15",
        "20260415",
        "2026-04-15 1234",
        "2026-04-15 12:34",
        "2026-04-15 12:34:56",
        "2026-04-15T12:34",
        "2026-04-15T12:34:56",
        "20260415 1234",
        "20260415 12:34",
        "20260415 12:34:56",
        "20260415T1234",
        "20260415T12:34",
        "20260415T12:34:56",
        "20260415T12:34:56.25",
        "2026-04-15 12:34+08:00",
        "2026-04-15T12:34:56+08",
        "2026-04-15T12:34:56+0800",
        "2026-04-15T12:34:56+08:00",
        "2026-04-15T12:34:56.123456789",
    ] {
        assert!(parse_literal_time(OsStr::new(raw)).is_ok(), "{raw}");
    }

    assert_eq!(
        parse_literal_time(OsStr::new("2026-04-15T12:34:56.25Z")).unwrap(),
        Timestamp::new(1_776_256_496, 250_000_000)
    );
    assert_eq!(
        parse_literal_time(OsStr::new("2026-04-15T20:34:56.25+08:00")).unwrap(),
        Timestamp::new(1_776_256_496, 250_000_000)
    );
}

#[test]
fn rejects_expanded_unsupported_literal_time_forms() {
    for raw in [
        "yesterday",
        "next friday",
        "2026-04",
        "2026-04-15T12:34.5",
        "2026-04-15T1234",
        "2026-04-15T123456",
        "2026-04-15 123456",
        "202604151234",
        "20260415123456",
        "202604151234.56",
        "20260415 123456",
        "20260415 123456.25",
        "20260415T12:34Z",
        "20260415T12:34:56Z",
        "20260415T12:34+08:00",
        "20260415T12:34:56+08:00",
        "20260415 1234+08:00",
        "2026-04-15 PST",
        "2026-02-30",
        "2026-04-15T25:00",
    ] {
        let error = parse_literal_time(OsStr::new(raw)).unwrap_err();
        assert!(
            error.message.contains("unsupported literal time format"),
            "{raw}"
        );
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
