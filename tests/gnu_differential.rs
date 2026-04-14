use assert_cmd::cargo::CommandCargoExt;
use std::collections::BTreeSet;
use std::fs;
use std::process::Command;
use tempfile::tempdir;

fn build_tree() -> tempfile::TempDir {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("src")).unwrap();
    fs::create_dir(root.path().join("docs")).unwrap();
    fs::write(root.path().join("src/lib.rs"), "pub fn lib() {}\n").unwrap();
    fs::write(root.path().join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(root.path().join("docs/spec.md"), "# spec\n").unwrap();
    root
}

fn lines(bytes: &[u8]) -> BTreeSet<String> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| line.to_string())
        .collect()
}

#[test]
fn readme_documents_worker_selection_contract() {
    let readme = fs::read_to_string("README.md").unwrap();

    assert!(readme.contains("FINDOXIDE_WORKERS"));
    assert!(readme.contains("GNU `find` syntax"));
}

#[test]
fn reports_unsupported_exec_during_planning() {
    let root = build_tree();
    let output = Command::cargo_bin("findoxide")
        .unwrap()
        .arg(root.path())
        .args(["-exec", "echo", "{}", ";"])
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("unsupported in read-only v0"));
}

#[test]
fn reports_parse_errors_nonzero() {
    let output = Command::cargo_bin("findoxide")
        .unwrap()
        .args(["(", "-name", "*.rs"])
        .output()
        .unwrap();

    assert_ne!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("expected `)`"));
}

#[test]
fn ordered_mode_matches_gnu_find_exactly() {
    let root = build_tree();
    let args = vec![
        root.path().to_string_lossy().to_string(),
        "-type".into(),
        "f".into(),
        "-name".into(),
        "*.rs".into(),
    ];

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "1")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(actual.stdout, expected.stdout);
}

#[test]
fn parallel_mode_matches_gnu_find_as_a_set() {
    let root = build_tree();
    let args = vec![
        root.path().to_string_lossy().to_string(),
        "(".into(),
        "-name".into(),
        "*.rs".into(),
        "-o".into(),
        "-name".into(),
        "*.md".into(),
        ")".into(),
        "-type".into(),
        "f".into(),
    ];

    let expected = Command::new("find").args(&args).output().unwrap();
    let actual = Command::cargo_bin("findoxide")
        .unwrap()
        .env("FINDOXIDE_WORKERS", "4")
        .args(&args)
        .output()
        .unwrap();

    assert_eq!(actual.status.code(), expected.status.code());
    assert_eq!(lines(&actual.stdout), lines(&expected.stdout));
}
