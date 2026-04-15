use findoxide::entry::{EntryContext, EntryKind};
use findoxide::follow::FollowMode;
use findoxide::identity::FileIdentity;
use std::fs;
use std::os::unix::fs::{self as unix_fs, MetadataExt};
use tempfile::tempdir;

#[test]
fn file_identity_matches_metadata_device_and_inode() {
    let root = tempdir().unwrap();
    let path = root.path().join("file.txt");
    fs::write(&path, "hello\n").unwrap();

    let metadata = fs::metadata(&path).unwrap();
    let identity = FileIdentity::from_metadata(&metadata);

    assert_eq!(identity.dev, metadata.dev());
    assert_eq!(identity.ino, metadata.ino());
}

#[test]
fn active_directory_identity_uses_logical_root_in_command_line_only_mode() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("dir-link")).unwrap();

    let physical = fs::symlink_metadata(root.path().join("dir-link")).unwrap();
    let logical = fs::metadata(root.path().join("dir-link")).ok();
    let entry = EntryContext::new(root.path().join("dir-link"), 0, true, physical, logical);

    assert_eq!(entry.physical_kind(), EntryKind::Symlink);
    assert_eq!(
        entry.active_kind(FollowMode::CommandLineOnly),
        EntryKind::Directory
    );
    assert_eq!(
        entry.active_directory_identity(FollowMode::CommandLineOnly),
        entry.logical_identity()
    );
    assert_eq!(
        entry.active_directory_identity(FollowMode::Logical),
        entry.logical_identity()
    );
    assert_eq!(entry.active_directory_identity(FollowMode::Physical), None);
}

#[test]
fn non_root_symlink_is_not_followed_in_command_line_only_mode() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("dir-link")).unwrap();

    let physical = fs::symlink_metadata(root.path().join("dir-link")).unwrap();
    let logical = fs::metadata(root.path().join("dir-link")).ok();
    let entry = EntryContext::new(root.path().join("dir-link"), 1, false, physical, logical);

    assert_eq!(entry.physical_kind(), EntryKind::Symlink);
    assert_eq!(
        entry.active_kind(FollowMode::CommandLineOnly),
        EntryKind::Symlink
    );
    assert_eq!(
        entry.active_directory_identity(FollowMode::CommandLineOnly),
        None
    );
}
