use rushfind::entry::{EntryContext, EntryKind};
use rushfind::follow::FollowMode;
use rushfind::identity::FileIdentity;
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

    assert_eq!(identity.device_number(), metadata.dev());
    assert_eq!(identity.inode_number(), metadata.ino());
}

#[test]
fn active_directory_identity_uses_logical_root_in_command_line_only_mode() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("dir-link")).unwrap();

    let entry = EntryContext::new(root.path().join("dir-link"), 0, true);

    assert_eq!(entry.physical_kind().unwrap(), EntryKind::Symlink);
    assert_eq!(
        entry.active_kind(FollowMode::CommandLineOnly).unwrap(),
        EntryKind::Directory
    );
    assert_eq!(
        entry
            .active_directory_identity(FollowMode::CommandLineOnly)
            .unwrap(),
        entry.logical_identity()
    );
    assert_eq!(
        entry
            .active_directory_identity(FollowMode::Logical)
            .unwrap(),
        entry.logical_identity()
    );
    assert_eq!(
        entry
            .active_directory_identity(FollowMode::Physical)
            .unwrap(),
        None
    );
}

#[test]
fn non_root_symlink_is_not_followed_in_command_line_only_mode() {
    let root = tempdir().unwrap();
    fs::create_dir(root.path().join("real")).unwrap();
    unix_fs::symlink(root.path().join("real"), root.path().join("dir-link")).unwrap();

    let entry = EntryContext::new(root.path().join("dir-link"), 1, false);

    assert_eq!(entry.physical_kind().unwrap(), EntryKind::Symlink);
    assert_eq!(
        entry.active_kind(FollowMode::CommandLineOnly).unwrap(),
        EntryKind::Symlink
    );
    assert_eq!(
        entry
            .active_directory_identity(FollowMode::CommandLineOnly)
            .unwrap(),
        None
    );
}
