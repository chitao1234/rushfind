use findoxide::entry::EntryContext;
use findoxide::eval::evaluate;
use findoxide::follow::FollowMode;
use findoxide::output::RecordingSink;
use findoxide::planner::{RuntimeExpr, RuntimePredicate};
use findoxide::size::parse_size_argument;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};
use tempfile::tempdir;

#[test]
fn size_uses_gnu_rounded_up_unit_semantics() {
    let root = tempdir().unwrap();
    let empty = root.path().join("empty.bin");
    let one_byte = root.path().join("one-byte.bin");
    let five_thirteen = root.path().join("five-thirteen.bin");
    fs::write(&empty, []).unwrap();
    fs::write(&one_byte, [b'x']).unwrap();
    fs::write(&five_thirteen, vec![b'x'; 513]).unwrap();

    assert!(evaluate_size(&empty, "-1M", FollowMode::Physical));
    assert!(!evaluate_size(&one_byte, "-1M", FollowMode::Physical));
    assert!(!evaluate_size(&five_thirteen, "1b", FollowMode::Physical));
    assert!(evaluate_size(&five_thirteen, "2b", FollowMode::Physical));
}

#[test]
fn size_reads_the_active_follow_mode_view() {
    let root = tempdir().unwrap();
    let target = root.path().join("target.bin");
    let link = root.path().join("target-link");
    fs::write(&target, vec![b'x'; 2049]).unwrap();
    unix_fs::symlink("target.bin", &link).unwrap();

    let physical_len = std::fs::symlink_metadata(&link).unwrap().len();
    let logical_len = std::fs::metadata(&link).unwrap().len();

    assert!(evaluate_size(
        &link,
        &format!("{physical_len}c"),
        FollowMode::Physical,
    ));
    assert!(evaluate_size(
        &link,
        &format!("{logical_len}c"),
        FollowMode::Logical,
    ));
}

fn evaluate_size(path: &Path, raw: &str, follow_mode: FollowMode) -> bool {
    let entry = EntryContext::new(PathBuf::from(path), 0, true);
    let expr = RuntimeExpr::Predicate(RuntimePredicate::Size(
        parse_size_argument(OsStr::new(raw)).unwrap(),
    ));
    let mut sink = RecordingSink::default();

    evaluate(&expr, &entry, follow_mode, &mut sink).unwrap()
}
