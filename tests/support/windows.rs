#![cfg(windows)]

pub(crate) fn normalize_stdout_path(text: &str) -> String {
    text.replace('/', "\\")
}

pub(crate) fn symlink_creation_available() -> bool {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("target.txt");
    let link = root.path().join("link.txt");
    std::fs::write(&target, b"target").unwrap();
    std::os::windows::fs::symlink_file(&target, &link).is_ok()
}
